use serde::{Deserialize, Serialize};

/// One record of yc-oss `companies/all.json`, field names verbatim from the source.
/// Fields observed as nullable on 2026-10-17 are `Option`; list fields default to empty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Company {
    pub id: i64,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub former_names: Vec<String>,
    #[serde(default)]
    pub small_logo_thumb_url: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub all_locations: Option<String>,
    #[serde(default)]
    pub long_description: Option<String>,
    #[serde(default)]
    pub one_liner: Option<String>,
    #[serde(default)]
    pub team_size: Option<i64>,
    #[serde(default)]
    pub industry: Option<String>,
    #[serde(default)]
    pub subindustry: Option<String>,
    #[serde(default)]
    pub launched_at: Option<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub top_company: Option<bool>,
    #[serde(rename = "isHiring", default)]
    pub is_hiring: Option<bool>,
    #[serde(default)]
    pub nonprofit: Option<bool>,
    #[serde(default)]
    pub batch: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub industries: Vec<String>,
    #[serde(default)]
    pub regions: Vec<String>,
    #[serde(default)]
    pub stage: Option<String>,
    /// YC canonical company page.
    #[serde(default)]
    pub url: Option<String>,
    /// yc-oss per-company record endpoint.
    #[serde(default)]
    pub api: Option<String>,
}

/// Subset of yc-oss `meta.json` that we rely on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceMeta {
    pub last_updated: Option<String>,
    #[serde(default)]
    pub readme: Option<String>,
}

/// Result of parsing a companies payload: valid records plus drift diagnostics.
#[derive(Debug, Clone, Default)]
pub struct ParsedCompanies {
    pub companies: Vec<Company>,
    pub skipped_invalid: usize,
    pub duplicate_slugs: Vec<String>,
    pub duplicate_ids: Vec<i64>,
}
