//! MCP surface: four read-only tools over the cached yc-oss dataset.

use std::sync::Arc;

use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::SourceError;
use crate::model::Company;
use crate::rank::{self, RANK_VERSION, SearchFilter};
use crate::rank_v2::{self, RANK_V2_VERSION};
use crate::source::{COMPANIES_URL, Dataset, META_URL, Snapshot, Store, USER_AGENT};

pub const ATTRIBUTION: &str = "Company data: yc-oss/api (https://github.com/yc-oss/api), an unofficial third-party mirror of Y Combinator's public company directory. Not a first-party YC API; not affiliated with or endorsed by Y Combinator. The yc-oss repository declares no license (checked 2026-10-17): data is read on user request and cached locally only, not redistributed. Startup Jobs (https://api.startup.jobs/mcp) is a separate MCP service and is not queried by this server.";

const UNKNOWNS: &str = "unknown";
const MAX_QUERY_LEN: usize = 200;
const MAX_KEYWORDS: usize = 10;
const MAX_KEYWORD_LEN: usize = 60;
const SUMMARY_DESC_CHARS: usize = 400;
const V1_METHOD: &str = "term_weight (generic 1 / specific 2) x field_weight (tags/industries/subindustry 3, one_liner 2, long_description 1); each term counted once at its best field; each category capped at 12; +1 if isHiring=true and score>0";
const V2_METHOD: &str = "as rank-v1, except: generic terms ignored in long_description; b2b/enterprise/bare 'support' removed; developer_education_writing requires developer context; support_tooling points only when support_assessment.class == vendor (evidence: one_liner product statement 3, description product statement 2, support tag 2, adjacent tag 1; vendor needs >=3 incl. a text statement; use-case sentences and non-software 'incident' wording excluded); each category capped at 12; +1 if isHiring=true and score>0";

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// Free-text phrase matched (whole tokens) against name, one_liner, long_description, tags, industries, subindustry.
    pub query: Option<String>,
    /// Case-insensitive substring of industry / subindustry / industries, e.g. "B2B", "Infrastructure".
    pub industry: Option<String>,
    /// Batch, e.g. "Winter 2024" or "W24" (W/S/F, X=Spring).
    pub batch: Option<String>,
    /// Only companies whose source isHiring flag is true. This is a company-level flag, not a vacancy.
    pub hiring_only: Option<bool>,
    /// Max results (1..=configured max, default 10).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetArgs {
    /// YC company slug, e.g. "circuithub".
    pub slug: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RankArgs {
    /// Extra keywords scored in a "custom_keywords" category (max 10, 60 chars each).
    pub keywords: Option<Vec<String>>,
    /// Max results (1..=configured max, default 10).
    pub limit: Option<u32>,
    /// "rank-v1" (default, baseline) or "rank-v2" (field-aware, explicit support taxonomy).
    pub scorer: Option<String>,
}

#[derive(Clone)]
pub struct YcServer {
    store: Arc<Store>,
    tool_router: ToolRouter<Self>,
}

fn ok(v: Value) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::structured(v))
}

fn fail(e: SourceError) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::structured_error(e.to_json()))
}

fn truncate(s: &str, n: usize) -> (String, bool) {
    if s.chars().count() <= n {
        (s.to_string(), false)
    } else {
        (s.chars().take(n).collect::<String>() + "…", true)
    }
}

fn unknowns() -> Value {
    json!({
        "serbia_hiring_eligibility": UNKNOWNS,
        "remote_contractor_acceptance": UNKNOWNS,
        "payment_terms": UNKNOWNS,
        "contract_type": UNKNOWNS,
        "open_vacancy": "unknown — the source isHiring flag is company-level and is not evidence of a specific open role",
        "note": "None of these are inferred from location, regions, team_size, batch or isHiring."
    })
}

fn company_view(c: &Company, ds: &Dataset, full: bool) -> Value {
    let (desc, truncated) = match &c.long_description {
        Some(d) if !full => {
            let (t, cut) = truncate(d, SUMMARY_DESC_CHARS);
            (Some(t), cut)
        }
        other => (other.clone(), false),
    };
    json!({
        "slug": c.slug,
        "name": c.name,
        "links": {
            "yc_page": c.url,
            "website": c.website,
            "source_record": c.api,
        },
        "source_fields": {
            "id": c.id,
            "one_liner": c.one_liner,
            "long_description": desc,
            "long_description_truncated": truncated,
            "batch": c.batch,
            "status": c.status,
            "stage": c.stage,
            "industry": c.industry,
            "subindustry": c.subindustry,
            "industries": c.industries,
            "tags": c.tags,
            "regions": c.regions,
            "all_locations": c.all_locations,
            "team_size": c.team_size,
            "isHiring": c.is_hiring,
            "launched_at": c.launched_at,
            "former_names": c.former_names,
        },
        "provenance": {
            "source_fields_from": COMPANIES_URL,
            "record_endpoint": c.api,
            "source_last_updated": ds.source_last_updated,
            "fetched_at": ds.fetched_at,
            "data_mode": ds.data_mode,
            "note": "Everything under source_fields and links is copied from the source record. Nothing there is derived."
        },
        "unknowns": unknowns(),
    })
}

impl YcServer {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            tool_router: Self::tool_router(),
        }
    }

    fn limit(&self, requested: Option<u32>) -> Result<usize, SourceError> {
        let max = self.store.cfg.max_results;
        match requested {
            None => Ok(10.min(max)),
            Some(0) => Err(SourceError::invalid("limit must be >= 1")),
            Some(n) if n as usize > max => Err(SourceError::invalid(format!(
                "limit {n} exceeds configured max {max}"
            ))),
            Some(n) => Ok(n as usize),
        }
    }

    fn envelope(&self, snap: &Snapshot) -> Value {
        let ds = &snap.dataset;
        json!({
            "ok": true,
            "data_mode": ds.data_mode,
            "stale": snap.stale,
            "refresh_error": snap.refresh_error,
            "source_last_updated": ds.source_last_updated,
            "fetched_at": ds.fetched_at,
            "attribution": ATTRIBUTION,
        })
    }

    fn merge(mut base: Value, extra: Value) -> Value {
        if let (Some(b), Value::Object(e)) = (base.as_object_mut(), extra) {
            b.extend(e);
        }
        base
    }
}

#[tool_router(router = tool_router)]
impl YcServer {
    #[tool(
        name = "search_yc_companies",
        description = "Filter public YC company profiles (yc-oss mirror, cached locally). Returns at most `limit` source-linked profiles. `derived.query_match_fields` lists which source fields matched; it is not a relevance score. isHiring is a company-level flag, not a vacancy.",
        annotations(read_only_hint = true, open_world_hint = true, idempotent_hint = true)
    )]
    pub async fn search_yc_companies(
        &self,
        params: Parameters<SearchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let a = params.0;
        let limit = match self.limit(a.limit) {
            Ok(l) => l,
            Err(e) => return fail(e),
        };
        for (name, v) in [
            ("query", &a.query),
            ("industry", &a.industry),
            ("batch", &a.batch),
        ] {
            if v.as_ref()
                .is_some_and(|s| s.chars().count() > MAX_QUERY_LEN)
            {
                return fail(SourceError::invalid(format!(
                    "{name} longer than {MAX_QUERY_LEN} chars"
                )));
            }
        }
        if a.query
            .as_ref()
            .is_some_and(|q| !q.trim().is_empty() && rank::tokenize(q).is_empty())
        {
            return fail(SourceError::invalid("query has no alphanumeric tokens"));
        }
        let snap = match self.store.snapshot(false).await {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        let filter = SearchFilter {
            query: a.query.clone().filter(|q| !q.trim().is_empty()),
            industry: a.industry.clone().filter(|s| !s.trim().is_empty()),
            batch: a.batch.clone().filter(|s| !s.trim().is_empty()),
            hiring_only: a.hiring_only.unwrap_or(false),
        };
        let hits = rank::search(&snap.dataset.companies, &filter);
        let results: Vec<Value> = hits
            .iter()
            .take(limit)
            .map(|(c, fields)| {
                Self::merge(
                    company_view(c, &snap.dataset, false),
                    json!({ "derived": { "query_match_fields": fields, "derived_by": "yc-cashflow-mcp search" } }),
                )
            })
            .collect();
        ok(Self::merge(
            self.envelope(&snap),
            json!({
                "filters": { "query": filter.query, "industry": filter.industry, "batch": filter.batch.as_deref().map(rank::normalize_batch), "hiring_only": filter.hiring_only },
                "total_matches": hits.len(),
                "returned": results.len(),
                "limit": limit,
                "results": results,
            }),
        ))
    }

    #[tool(
        name = "get_yc_company",
        description = "Get one public YC company profile by slug, with its YC canonical page and website link. Unknown slugs return a structured not_found result.",
        annotations(read_only_hint = true, open_world_hint = true, idempotent_hint = true)
    )]
    pub async fn get_yc_company(
        &self,
        params: Parameters<GetArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let slug = params.0.slug.trim().to_lowercase();
        let valid = !slug.is_empty()
            && slug.len() <= 100
            && slug
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !valid {
            return fail(SourceError::invalid(
                "slug must be 1-100 chars of [a-z0-9-_.]",
            ));
        }
        let snap = match self.store.snapshot(false).await {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        match snap.dataset.get(&slug) {
            Some(c) => ok(Self::merge(
                self.envelope(&snap),
                json!({ "found": true, "company": company_view(c, &snap.dataset, true) }),
            )),
            None => ok(Self::merge(
                self.envelope(&snap),
                json!({
                    "found": false,
                    "result": "not_found",
                    "slug": slug,
                    "message": "No company with this slug in the cached yc-oss dataset."
                }),
            )),
        }
    }

    #[tool(
        name = "rank_yc_prospects",
        description = "Deterministic keyword ranking (scorer \"rank-v1\" default, or \"rank-v2\") of Active YC companies for Python/AI engineering, developer education/writing, DevTools, B2B infrastructure/SaaS and support tooling. Every score lists the matched term, category and source field. Does not assess hiring eligibility (Serbia or otherwise), vacancies, payment or contract terms.",
        annotations(read_only_hint = true, open_world_hint = true, idempotent_hint = true)
    )]
    pub async fn rank_yc_prospects(
        &self,
        params: Parameters<RankArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let a = params.0;
        let limit = match self.limit(a.limit) {
            Ok(l) => l,
            Err(e) => return fail(e),
        };
        let scorer = a.scorer.as_deref().unwrap_or(RANK_VERSION);
        if scorer != RANK_VERSION && scorer != RANK_V2_VERSION {
            return fail(SourceError::invalid(format!(
                "scorer must be \"{RANK_VERSION}\" or \"{RANK_V2_VERSION}\""
            )));
        }
        let keywords = a.keywords.unwrap_or_default();
        if keywords.len() > MAX_KEYWORDS {
            return fail(SourceError::invalid(format!(
                "at most {MAX_KEYWORDS} keywords"
            )));
        }
        let mut cleaned = Vec::new();
        for k in &keywords {
            if k.chars().count() > MAX_KEYWORD_LEN || rank::tokenize(k).is_empty() {
                return fail(SourceError::invalid(format!(
                    "keyword {k:?} must be 1-{MAX_KEYWORD_LEN} chars with an alphanumeric token"
                )));
            }
            cleaned.push(k.clone());
        }
        let snap = match self.store.snapshot(false).await {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        let ds = &snap.dataset;
        let (version, method, total, results): (&str, &str, usize, Vec<Value>) = if scorer
            == RANK_V2_VERSION
        {
            let ranked = rank_v2::rank(&ds.companies, &cleaned);
            let results = ranked
                .iter()
                .take(limit)
                .enumerate()
                .map(|(i, (c, s))| {
                    Self::merge(
                        company_view(c, ds, false),
                        json!({
                            "derived": {
                                "rank": i + 1,
                                "relevance_score": s.score,
                                "category_scores": s.category_scores.iter().map(|(k, v)| json!({"category": k, "points": v})).collect::<Vec<_>>(),
                                "explanation": s.matches,
                                "suppressed_matches": s.suppressed,
                                "support_assessment": s.support,
                                "signals": s.signals,
                                "derived_by": RANK_V2_VERSION,
                                "note": "Derived by this server from source_fields; not a YC or company statement."
                            }
                        }),
                    )
                })
                .collect();
            (RANK_V2_VERSION, V2_METHOD, ranked.len(), results)
        } else {
            let ranked = rank::rank(&ds.companies, &cleaned);
            let results = ranked
                .iter()
                .take(limit)
                .enumerate()
                .map(|(i, (c, s))| {
                    Self::merge(
                        company_view(c, ds, false),
                        json!({
                            "derived": {
                                "rank": i + 1,
                                "relevance_score": s.score,
                                "category_scores": s.category_scores.iter().map(|(k, v)| json!({"category": k, "points": v})).collect::<Vec<_>>(),
                                "explanation": s.matches,
                                "signals": s.signals,
                                "derived_by": RANK_VERSION,
                                "note": "Derived by this server from source_fields; not a YC or company statement."
                            }
                        }),
                    )
                })
                .collect();
            (RANK_VERSION, V1_METHOD, ranked.len(), results)
        };
        ok(Self::merge(
            self.envelope(&snap),
            json!({
                "method": {
                    "version": version,
                    "population": "companies with source status == \"Active\"",
                    "points": method,
                    "tie_break": "slug ascending",
                    "custom_keywords": cleaned,
                },
                "total_scored": total,
                "returned": results.len(),
                "limit": limit,
                "results": results,
            }),
        ))
    }

    #[tool(
        name = "source_status",
        description = "Dataset origin, source last-update timestamp, last successful fetch, record count, cache age, staleness, version and transport health. Does not trigger network access.",
        annotations(read_only_hint = true, open_world_hint = false, idempotent_hint = true)
    )]
    pub async fn source_status(&self) -> Result<CallToolResult, ErrorData> {
        let (ds, last_error) = self.store.peek().await;
        let cfg = &self.store.cfg;
        let dataset = ds.as_ref().map(|d| {
            json!({
                "data_mode": d.data_mode,
                "records": d.companies.len(),
                "skipped_invalid_records": d.skipped_invalid,
                "duplicate_slugs_dropped": d.duplicate_slugs,
                "duplicate_ids_dropped": d.duplicate_ids,
                "source_last_updated": d.source_last_updated,
                "last_successful_fetch": d.fetched_at,
                "cache_age_secs": self.store.cache_age_secs(d),
                "stale": self.store.is_stale(d),
            })
        });
        ok(json!({
            "ok": true,
            "server": { "name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION"), "default_scorer": RANK_VERSION, "available_scorers": [RANK_VERSION, RANK_V2_VERSION] },
            "transport": { "kind": "stdio", "health": "ok (this response was delivered)" },
            "origin": {
                "companies_url": COMPANIES_URL,
                "meta_url": META_URL,
                "repository": "https://github.com/yc-oss/api",
                "kind": "unofficial third-party mirror of YC public Algolia index",
                "license": "none declared in repository (checked 2026-10-17)",
                "fetcher_mode": self.store.fetcher_mode(),
            },
            "dataset": dataset.unwrap_or_else(|| json!({ "loaded": false, "message": "No data loaded yet; the first search/get/rank call fetches it." })),
            "last_error": last_error,
            "limits": {
                "cache_ttl_secs": cfg.ttl.as_secs(),
                "max_results": cfg.max_results,
                "timeout_secs": cfg.timeout.as_secs(),
                "max_retries": cfg.max_retries,
                "max_response_bytes": cfg.max_bytes,
                "user_agent": USER_AGENT,
                "url_allowlist": "https://yc-oss.github.io/api/*",
                "cache_dir": cfg.cache_dir,
            },
            "attribution": ATTRIBUTION,
        }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for YcServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Read-only access to public YC company profiles from the unofficial yc-oss mirror. \
                 Scores are deterministic keyword matches with per-field explanations. \
                 Hiring eligibility, vacancies, payment and contract terms are always unknown.",
            )
    }
}
