//! Public-data acquisition: allowlisted HTTP fetch, on-disk cache with TTL, and parsing.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::error::{ErrorCategory, SourceError};
use crate::model::{Company, ParsedCompanies, SourceMeta};

pub const SOURCE_BASE: &str = "https://yc-oss.github.io/api/";
pub const COMPANIES_URL: &str = "https://yc-oss.github.io/api/companies/all.json";
pub const META_URL: &str = "https://yc-oss.github.io/api/meta.json";
pub const ALLOWED_HOST: &str = "yc-oss.github.io";
pub const USER_AGENT: &str = concat!(
    "yc-cashflow-mcp/",
    env!("CARGO_PKG_VERSION"),
    " (local read-only MCP server; user-initiated reads)"
);
/// Hard ceiling regardless of configuration.
pub const HARD_MAX_RESULTS: usize = 50;
/// If more than this fraction of records fail to parse, treat the payload as schema drift.
const MAX_INVALID_FRACTION: f64 = 0.05;

#[derive(Debug, Clone)]
pub struct Config {
    pub cache_dir: PathBuf,
    pub ttl: Duration,
    pub max_results: usize,
    pub timeout: Duration,
    pub max_retries: u32,
    pub max_bytes: usize,
    /// When set, data is read from fixture files instead of the network.
    pub fixture_dir: Option<PathBuf>,
}

impl Config {
    pub fn from_env() -> Self {
        fn num(key: &str, default: u64) -> u64 {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        }
        let cache_dir = std::env::var_os("YC_MCP_CACHE_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("XDG_CACHE_HOME").map(|p| PathBuf::from(p).join("yc-cashflow-mcp"))
            })
            .or_else(|| {
                std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache/yc-cashflow-mcp"))
            })
            .unwrap_or_else(|| PathBuf::from(".yc-cashflow-mcp-cache"));
        Self {
            cache_dir,
            ttl: Duration::from_secs(num("YC_MCP_TTL_SECS", 86_400)),
            max_results: (num("YC_MCP_MAX_RESULTS", 25) as usize).clamp(1, HARD_MAX_RESULTS),
            timeout: Duration::from_secs(num("YC_MCP_TIMEOUT_SECS", 30)),
            max_retries: num("YC_MCP_MAX_RETRIES", 2).min(5) as u32,
            max_bytes: num("YC_MCP_MAX_BYTES", 32 * 1024 * 1024) as usize,
            fixture_dir: std::env::var_os("YC_MCP_FIXTURE_DIR").map(PathBuf::from),
        }
    }
}

/// Only `https://yc-oss.github.io/api/...` may be fetched.
pub fn check_url_allowed(raw: &str) -> Result<url::Url, SourceError> {
    let parsed = url::Url::parse(raw).map_err(|e| {
        SourceError::new(ErrorCategory::UrlNotAllowed, format!("bad url {raw}: {e}"))
    })?;
    let ok = parsed.scheme() == "https"
        && parsed.host_str() == Some(ALLOWED_HOST)
        && parsed.port().is_none()
        && parsed.path().starts_with("/api/")
        && parsed.username().is_empty()
        && parsed.password().is_none();
    if ok {
        Ok(parsed)
    } else {
        Err(SourceError::new(
            ErrorCategory::UrlNotAllowed,
            format!("url not in allowlist ({SOURCE_BASE}*): {raw}"),
        ))
    }
}

pub type FetchFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>, SourceError>> + Send + 'a>>;

/// Byte source for the dataset; swapped for fixtures or failure injection in tests.
pub trait Fetcher: Send + Sync {
    fn fetch<'a>(&'a self, url: &'a str) -> FetchFuture<'a>;
    /// "live" or "fixture"; reported verbatim by `source_status`.
    fn mode(&self) -> &'static str;
}

pub struct HttpFetcher {
    client: reqwest::Client,
    max_retries: u32,
    max_bytes: usize,
}

impl HttpFetcher {
    pub fn new(cfg: &Config) -> Result<Self, SourceError> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(cfg.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .https_only(true)
            .build()
            .map_err(|e| SourceError::new(ErrorCategory::SourceUnavailable, e.to_string()))?;
        Ok(Self {
            client,
            max_retries: cfg.max_retries,
            max_bytes: cfg.max_bytes,
        })
    }

    async fn fetch_once(&self, url: &str) -> Result<Vec<u8>, SourceError> {
        let unavailable =
            |e: reqwest::Error| SourceError::new(ErrorCategory::SourceUnavailable, e.to_string());
        let mut resp = self.client.get(url).send().await.map_err(unavailable)?;
        let status = resp.status();
        match status.as_u16() {
            200 => {}
            404 => {
                return Err(SourceError::new(
                    ErrorCategory::NotFound,
                    format!("404 from {url}"),
                ));
            }
            429 => {
                let retry_after = resp
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("unspecified")
                    .to_string();
                return Err(SourceError::new(
                    ErrorCategory::RateLimited,
                    format!("429 from {url}; retry-after: {retry_after}"),
                ));
            }
            _ => {
                return Err(SourceError::new(
                    ErrorCategory::SourceUnavailable,
                    format!("HTTP {status} from {url}"),
                ));
            }
        }
        if let Some(len) = resp.content_length()
            && len as usize > self.max_bytes
        {
            return Err(too_large(url, self.max_bytes));
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(unavailable)? {
            if body.len() + chunk.len() > self.max_bytes {
                return Err(too_large(url, self.max_bytes));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

fn too_large(url: &str, cap: usize) -> SourceError {
    SourceError::new(
        ErrorCategory::ResponseTooLarge,
        format!("response from {url} exceeds byte cap {cap}"),
    )
}

impl Fetcher for HttpFetcher {
    fn fetch<'a>(&'a self, url: &'a str) -> FetchFuture<'a> {
        Box::pin(async move {
            check_url_allowed(url)?;
            let mut attempt = 0;
            loop {
                match self.fetch_once(url).await {
                    Err(e) if e.is_retryable() && attempt < self.max_retries => {
                        attempt += 1;
                        tracing::warn!(%url, attempt, error = %e.message, "retrying fetch");
                        tokio::time::sleep(Duration::from_millis(500 * 2u64.pow(attempt))).await;
                    }
                    other => return other,
                }
            }
        })
    }

    fn mode(&self) -> &'static str {
        "live"
    }
}

/// Serves `<dir>/meta.json` and `<dir>/all.json` for the allowlisted URLs. Never touches the network.
pub struct FixtureFetcher {
    pub dir: PathBuf,
}

impl Fetcher for FixtureFetcher {
    fn fetch<'a>(&'a self, url: &'a str) -> FetchFuture<'a> {
        Box::pin(async move {
            check_url_allowed(url)?;
            let file = match url {
                COMPANIES_URL => "all.json",
                META_URL => "meta.json",
                _ => {
                    return Err(SourceError::new(
                        ErrorCategory::NotFound,
                        format!("no fixture for {url}"),
                    ));
                }
            };
            tokio::fs::read(self.dir.join(file)).await.map_err(|e| {
                SourceError::new(
                    ErrorCategory::SourceUnavailable,
                    format!("fixture {file}: {e}"),
                )
            })
        })
    }

    fn mode(&self) -> &'static str {
        "fixture"
    }
}

pub fn parse_meta(bytes: &[u8]) -> Result<SourceMeta, SourceError> {
    serde_json::from_slice(bytes)
        .map_err(|e| SourceError::new(ErrorCategory::SchemaDrift, format!("meta.json: {e}")))
}

/// Parses `companies/all.json`. Individual malformed records are skipped and counted;
/// a non-array payload or a high invalid fraction is reported as schema drift.
/// Duplicate slugs/ids keep the first occurrence.
pub fn parse_companies(bytes: &[u8]) -> Result<ParsedCompanies, SourceError> {
    let raw: Value = serde_json::from_slice(bytes).map_err(|e| {
        SourceError::new(
            ErrorCategory::SchemaDrift,
            format!("companies payload is not JSON: {e}"),
        )
    })?;
    let Value::Array(items) = raw else {
        return Err(SourceError::new(
            ErrorCategory::SchemaDrift,
            "companies payload is not a JSON array",
        ));
    };
    let total = items.len();
    let mut out = ParsedCompanies::default();
    let mut seen_slugs = HashSet::new();
    let mut seen_ids = HashSet::new();
    for item in items {
        let Ok(c) = serde_json::from_value::<Company>(item) else {
            out.skipped_invalid += 1;
            continue;
        };
        if !seen_slugs.insert(c.slug.clone()) {
            out.duplicate_slugs.push(c.slug);
            continue;
        }
        if !seen_ids.insert(c.id) {
            out.duplicate_ids.push(c.id);
            continue;
        }
        out.companies.push(c);
    }
    if total > 0 && (out.skipped_invalid as f64 / total as f64) > MAX_INVALID_FRACTION {
        return Err(SourceError::new(
            ErrorCategory::SchemaDrift,
            format!(
                "{} of {} records failed to parse against the expected schema",
                out.skipped_invalid, total
            ),
        ));
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FetchRecord {
    fetched_at: DateTime<Utc>,
    source_last_updated: Option<String>,
    data_mode: String,
    bytes: usize,
}

#[derive(Debug)]
pub struct Dataset {
    pub companies: Vec<Company>,
    pub by_slug: HashMap<String, usize>,
    pub fetched_at: DateTime<Utc>,
    pub source_last_updated: Option<String>,
    pub data_mode: String,
    pub skipped_invalid: usize,
    pub duplicate_slugs: Vec<String>,
    pub duplicate_ids: Vec<i64>,
}

impl Dataset {
    fn build(parsed: ParsedCompanies, rec: &FetchRecord) -> Self {
        let by_slug = parsed
            .companies
            .iter()
            .enumerate()
            .map(|(i, c)| (c.slug.clone(), i))
            .collect();
        Self {
            companies: parsed.companies,
            by_slug,
            fetched_at: rec.fetched_at,
            source_last_updated: rec.source_last_updated.clone(),
            data_mode: rec.data_mode.clone(),
            skipped_invalid: parsed.skipped_invalid,
            duplicate_slugs: parsed.duplicate_slugs,
            duplicate_ids: parsed.duplicate_ids,
        }
    }

    pub fn get(&self, slug: &str) -> Option<&Company> {
        self.by_slug.get(slug).map(|&i| &self.companies[i])
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LastError {
    pub category: ErrorCategory,
    pub message: String,
    pub at: DateTime<Utc>,
}

#[derive(Default)]
struct State {
    dataset: Option<Arc<Dataset>>,
    last_error: Option<LastError>,
}

/// A loaded dataset plus whether it is past TTL (served because a refresh failed).
pub struct Snapshot {
    pub dataset: Arc<Dataset>,
    pub stale: bool,
    pub refresh_error: Option<LastError>,
}

pub struct Store {
    pub cfg: Config,
    fetcher: Arc<dyn Fetcher>,
    state: Mutex<State>,
}

impl Store {
    pub fn new(cfg: Config, fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            cfg,
            fetcher,
            state: Mutex::new(State::default()),
        }
    }

    pub fn from_config(cfg: Config) -> Result<Self, SourceError> {
        let fetcher: Arc<dyn Fetcher> = match &cfg.fixture_dir {
            Some(dir) => Arc::new(FixtureFetcher { dir: dir.clone() }),
            None => Arc::new(HttpFetcher::new(&cfg)?),
        };
        Ok(Self::new(cfg, fetcher))
    }

    pub fn fetcher_mode(&self) -> &'static str {
        self.fetcher.mode()
    }

    fn cache_paths(&self) -> (PathBuf, PathBuf) {
        let mode = self.fetcher.mode();
        (
            self.cfg.cache_dir.join(format!("{mode}-all.json")),
            self.cfg.cache_dir.join(format!("{mode}-fetch.json")),
        )
    }

    fn age(&self, ds: &Dataset) -> Duration {
        (Utc::now() - ds.fetched_at).to_std().unwrap_or_default()
    }

    pub fn is_stale(&self, ds: &Dataset) -> bool {
        self.age(ds) > self.cfg.ttl
    }

    pub fn cache_age_secs(&self, ds: &Dataset) -> u64 {
        self.age(ds).as_secs()
    }

    async fn read_disk_cache(&self) -> Option<Dataset> {
        let (data_path, rec_path) = self.cache_paths();
        let rec: FetchRecord =
            serde_json::from_slice(&tokio::fs::read(&rec_path).await.ok()?).ok()?;
        let bytes = tokio::fs::read(&data_path).await.ok()?;
        let parsed = parse_companies(&bytes).ok()?;
        Some(Dataset::build(parsed, &rec))
    }

    async fn write_disk_cache(&self, bytes: &[u8], rec: &FetchRecord) -> std::io::Result<()> {
        let (data_path, rec_path) = self.cache_paths();
        tokio::fs::create_dir_all(&self.cfg.cache_dir).await?;
        write_atomic(&data_path, bytes).await?;
        write_atomic(
            &rec_path,
            &serde_json::to_vec_pretty(rec).unwrap_or_default(),
        )
        .await
    }

    async fn fetch_fresh(&self) -> Result<Dataset, SourceError> {
        let bytes = self.fetcher.fetch(COMPANIES_URL).await?;
        let parsed = parse_companies(&bytes)?;
        // meta.json only supplies the source's own update timestamp; its absence is not fatal.
        let source_last_updated = match self
            .fetcher
            .fetch(META_URL)
            .await
            .and_then(|b| parse_meta(&b))
        {
            Ok(m) => m.last_updated,
            Err(e) => {
                tracing::warn!(error = %e, "meta.json unavailable");
                None
            }
        };
        let rec = FetchRecord {
            fetched_at: Utc::now(),
            source_last_updated,
            data_mode: self.fetcher.mode().to_string(),
            bytes: bytes.len(),
        };
        if let Err(e) = self.write_disk_cache(&bytes, &rec).await {
            tracing::warn!(error = %e, "could not write cache");
        }
        Ok(Dataset::build(parsed, &rec))
    }

    /// Returns a dataset, refreshing when past TTL. A failed refresh falls back to
    /// stale data (flagged); with no data at all the error is returned.
    pub async fn snapshot(&self, force_refresh: bool) -> Result<Snapshot, SourceError> {
        let mut st = self.state.lock().await;
        if st.dataset.is_none()
            && let Some(ds) = self.read_disk_cache().await
        {
            st.dataset = Some(Arc::new(ds));
        }
        if let Some(ds) = &st.dataset
            && !force_refresh
            && !self.is_stale(ds)
        {
            return Ok(Snapshot {
                dataset: ds.clone(),
                stale: false,
                refresh_error: None,
            });
        }
        match self.fetch_fresh().await {
            Ok(ds) => {
                let ds = Arc::new(ds);
                st.dataset = Some(ds.clone());
                st.last_error = None;
                Ok(Snapshot {
                    dataset: ds,
                    stale: false,
                    refresh_error: None,
                })
            }
            Err(e) => {
                let le = LastError {
                    category: e.category,
                    message: e.message.clone(),
                    at: Utc::now(),
                };
                st.last_error = Some(le.clone());
                match &st.dataset {
                    Some(ds) => Ok(Snapshot {
                        dataset: ds.clone(),
                        stale: self.is_stale(ds),
                        refresh_error: Some(le),
                    }),
                    None => Err(e),
                }
            }
        }
    }

    /// Current state without triggering network access.
    pub async fn peek(&self) -> (Option<Arc<Dataset>>, Option<LastError>) {
        let mut st = self.state.lock().await;
        if st.dataset.is_none()
            && let Some(ds) = self.read_disk_cache().await
        {
            st.dataset = Some(Arc::new(ds));
        }
        (st.dataset.clone(), st.last_error.clone())
    }
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, bytes).await?;
    tokio::fs::rename(&tmp, path).await
}
