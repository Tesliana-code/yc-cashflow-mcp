//! Library-level tests against synthetic fixtures and scripted fetchers. No network.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use yc_cashflow_mcp::error::{ErrorCategory, SourceError};
use yc_cashflow_mcp::rank::{self, SearchFilter};
use yc_cashflow_mcp::source::{
    COMPANIES_URL, Config, FetchFuture, Fetcher, FixtureFetcher, META_URL, Store,
    check_url_allowed, parse_companies,
};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture_bytes() -> Vec<u8> {
    std::fs::read(fixture_dir().join("all.json")).unwrap()
}

fn cfg(cache: &std::path::Path, ttl: Duration) -> Config {
    Config {
        cache_dir: cache.to_path_buf(),
        ttl,
        max_results: 25,
        timeout: Duration::from_secs(5),
        max_retries: 0,
        max_bytes: 1 << 20,
        fixture_dir: None,
    }
}

/// Serves the fixture for the first `ok_calls` companies requests, then fails with `err`.
struct Scripted {
    ok_calls: usize,
    calls: AtomicUsize,
    err: ErrorCategory,
}

impl Fetcher for Scripted {
    fn fetch<'a>(&'a self, url: &'a str) -> FetchFuture<'a> {
        Box::pin(async move {
            if url == META_URL {
                return Err(SourceError::new(
                    ErrorCategory::NotFound,
                    "no meta in script",
                ));
            }
            assert_eq!(url, COMPANIES_URL);
            if self.calls.fetch_add(1, Ordering::SeqCst) < self.ok_calls {
                Ok(fixture_bytes())
            } else {
                Err(SourceError::new(self.err, "scripted failure"))
            }
        })
    }
    fn mode(&self) -> &'static str {
        "fixture"
    }
}

#[test]
fn parses_schema_nulls_and_duplicates() {
    let p = parse_companies(&fixture_bytes()).unwrap();
    assert_eq!(p.companies.len(), 8);
    assert_eq!(p.duplicate_slugs, vec!["fixture-pyagents".to_string()]);
    assert_eq!(p.duplicate_ids, vec![101]);
    let nulls = p
        .companies
        .iter()
        .find(|c| c.slug == "fixture-nulls")
        .unwrap();
    assert!(
        nulls.website.is_none() && nulls.long_description.is_none() && nulls.team_size.is_none()
    );
    assert!(nulls.top_company.is_none());
    // first occurrence wins for duplicate slug
    let first = p
        .companies
        .iter()
        .find(|c| c.slug == "fixture-pyagents")
        .unwrap();
    assert_eq!(first.name, "Fixture PyAgents");
}

#[test]
fn missing_optional_fields_are_tolerated() {
    let p = parse_companies(br#"[{"id":1,"name":"A","slug":"a"}]"#).unwrap();
    assert_eq!(p.companies.len(), 1);
    assert!(p.companies[0].tags.is_empty());
}

#[test]
fn schema_drift_detected() {
    let e = parse_companies(br#"{"companies":[]}"#).unwrap_err();
    assert_eq!(e.category, ErrorCategory::SchemaDrift);
    let e = parse_companies(br#"[{"id":"x","slug":1},{"name":"no id"}]"#).unwrap_err();
    assert_eq!(e.category, ErrorCategory::SchemaDrift);
    let e = parse_companies(b"<html>").unwrap_err();
    assert_eq!(e.category, ErrorCategory::SchemaDrift);
}

#[test]
fn url_allowlist() {
    assert!(check_url_allowed(COMPANIES_URL).is_ok());
    for bad in [
        "http://yc-oss.github.io/api/companies/all.json",
        "https://evil.example/api/companies/all.json",
        "https://yc-oss.github.io/other/x.json",
        "https://yc-oss.github.io:8443/api/x.json",
        "https://user@yc-oss.github.io/api/x.json",
    ] {
        assert_eq!(
            check_url_allowed(bad).unwrap_err().category,
            ErrorCategory::UrlNotAllowed,
            "{bad}"
        );
    }
}

#[test]
fn ranking_is_stable_and_explained() {
    let p = parse_companies(&fixture_bytes()).unwrap();
    let a: Vec<_> = rank::rank(&p.companies, &[])
        .iter()
        .map(|(c, s)| (c.slug.clone(), s.score))
        .collect();
    let b: Vec<_> = rank::rank(&p.companies, &[])
        .iter()
        .map(|(c, s)| (c.slug.clone(), s.score))
        .collect();
    assert_eq!(a, b);
    assert_eq!(a[0], ("fixture-docsmith".to_string(), 30));
    assert_eq!(a[1], ("fixture-pyagents".to_string(), 28));
    // Inactive and irrelevant companies are excluded
    assert!(
        !a.iter()
            .any(|(s, _)| s == "fixture-deadco" || s == "fixture-bakery")
    );
    // tie broken by slug ascending
    let ia = a.iter().position(|(s, _)| s == "fixture-tie-a").unwrap();
    let ib = a.iter().position(|(s, _)| s == "fixture-tie-b").unwrap();
    assert_eq!(a[ia].1, a[ib].1);
    assert!(ia < ib);
    // every point is backed by a matched field
    for (_, s) in rank::rank(&p.companies, &[]) {
        assert!(!s.matches.is_empty());
        assert!(
            s.matches
                .iter()
                .all(|m| !m.field.is_empty() && m.points > 0)
        );
    }
}

#[test]
fn custom_keywords_and_token_boundaries() {
    let p = parse_companies(&fixture_bytes()).unwrap();
    let r = rank::rank(&p.companies, &["bread".into()]);
    assert!(r.iter().any(|(c, s)| c.slug == "fixture-bakery"
        && s.matches.iter().any(|m| m.category == "custom_keywords")));
    // "ai" must not match inside other words such as "Fixture HelpdeskAI" one_liner tokens
    assert!(
        rank::tokenize("Containers & Maintain")
            .iter()
            .all(|t| t != "ai")
    );
}

#[test]
fn query_and_filters() {
    let p = parse_companies(&fixture_bytes()).unwrap();
    let f = |q: Option<&str>, ind: Option<&str>, batch: Option<&str>, hiring: bool| {
        rank::search(
            &p.companies,
            &SearchFilter {
                query: q.map(Into::into),
                industry: ind.map(Into::into),
                batch: batch.map(Into::into),
                hiring_only: hiring,
            },
        )
        .into_iter()
        .map(|(c, _)| c.slug.clone())
        .collect::<Vec<_>>()
    };
    assert_eq!(
        f(Some("customer support"), None, None, false),
        vec!["fixture-helpdeskai"]
    );
    assert_eq!(
        f(None, Some("consumer"), None, false),
        vec!["fixture-bakery"]
    );
    assert_eq!(f(None, None, Some("S23"), false), vec!["fixture-docsmith"]);
    assert_eq!(
        f(None, None, Some("summer 2023"), false),
        vec!["fixture-docsmith"]
    );
    assert_eq!(
        f(None, None, None, true),
        vec!["fixture-docsmith", "fixture-pyagents"]
    );
    assert!(f(Some("nonexistentterm"), None, None, false).is_empty());
}

#[tokio::test]
async fn first_run_offline_returns_source_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let f = Arc::new(Scripted {
        ok_calls: 0,
        calls: AtomicUsize::new(0),
        err: ErrorCategory::SourceUnavailable,
    });
    let store = Store::new(cfg(dir.path(), Duration::from_secs(3600)), f);
    let e = store.snapshot(false).await.err().unwrap();
    assert_eq!(e.category, ErrorCategory::SourceUnavailable);
    let (ds, last) = store.peek().await;
    assert!(ds.is_none());
    assert_eq!(last.unwrap().category, ErrorCategory::SourceUnavailable);
}

#[tokio::test]
async fn stale_cache_served_when_refresh_fails() {
    let dir = tempfile::tempdir().unwrap();
    let f = Arc::new(Scripted {
        ok_calls: 1,
        calls: AtomicUsize::new(0),
        err: ErrorCategory::RateLimited,
    });
    let store = Store::new(cfg(dir.path(), Duration::from_millis(1)), f);
    let s1 = store.snapshot(false).await.unwrap();
    assert!(!s1.stale);
    assert!(
        s1.dataset.source_last_updated.is_none(),
        "meta failure is non-fatal"
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    let s2 = store.snapshot(false).await.unwrap();
    assert!(s2.stale);
    assert_eq!(
        s2.refresh_error.unwrap().category,
        ErrorCategory::RateLimited
    );
    assert_eq!(s2.dataset.companies.len(), 8);
}

#[tokio::test]
async fn disk_cache_survives_restart_offline() {
    let dir = tempfile::tempdir().unwrap();
    let online = Arc::new(Scripted {
        ok_calls: 1,
        calls: AtomicUsize::new(0),
        err: ErrorCategory::SourceUnavailable,
    });
    Store::new(cfg(dir.path(), Duration::from_secs(3600)), online)
        .snapshot(false)
        .await
        .unwrap();
    let offline = Arc::new(Scripted {
        ok_calls: 0,
        calls: AtomicUsize::new(0),
        err: ErrorCategory::SourceUnavailable,
    });
    let s = Store::new(cfg(dir.path(), Duration::from_secs(3600)), offline.clone())
        .snapshot(false)
        .await
        .unwrap();
    assert!(!s.stale);
    assert_eq!(
        offline.calls.load(Ordering::SeqCst),
        0,
        "fresh disk cache must not hit network"
    );
}

#[tokio::test]
async fn fixture_fetcher_reports_fixture_mode() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(
        cfg(dir.path(), Duration::from_secs(3600)),
        Arc::new(FixtureFetcher { dir: fixture_dir() }),
    );
    let s = store.snapshot(false).await.unwrap();
    assert_eq!(s.dataset.data_mode, "fixture");
    assert_eq!(
        s.dataset.source_last_updated.as_deref(),
        Some("2026-01-01T00:00:00.000Z")
    );
}

#[test]
fn rank_v2_is_stable_and_classifies_fixture_support() {
    use yc_cashflow_mcp::rank_v2::{self, SupportClass};
    let p = parse_companies(&fixture_bytes()).unwrap();
    let a: Vec<_> = rank_v2::rank(&p.companies, &[])
        .iter()
        .map(|(c, s)| (c.slug.clone(), s.score))
        .collect();
    let b: Vec<_> = rank_v2::rank(&p.companies, &[])
        .iter()
        .map(|(c, s)| (c.slug.clone(), s.score))
        .collect();
    assert_eq!(a, b);
    let get = |slug: &str| p.companies.iter().find(|c| c.slug == slug).unwrap();
    assert_eq!(
        rank_v2::assess_support(get("fixture-helpdeskai")).class,
        SupportClass::Vendor
    );
    assert_eq!(
        rank_v2::assess_support(get("fixture-pyagents")).class,
        SupportClass::None
    );
    // Inactive companies stay excluded
    assert!(!a.iter().any(|(s, _)| s == "fixture-deadco"));
}
