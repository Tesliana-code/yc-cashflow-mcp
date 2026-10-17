//! Deterministic, explainable keyword scoring. No LLM, no network, no randomness.

use serde::Serialize;

use crate::model::Company;

pub const RANK_VERSION: &str = "rank-v1";

/// Source fields that are scored, with their weight. Curated fields (tags, industries)
/// weigh more than free text.
const FIELDS: &[(&str, u32)] = &[
    ("tags", 3),
    ("industries", 3),
    ("subindustry", 3),
    ("one_liner", 2),
    ("long_description", 1),
];

/// (category, &[(term, term_weight)]). Specific terms weigh 2, generic ones 1.
pub const CATEGORIES: &[(&str, &[(&str, u32)])] = &[
    (
        "python_ai_engineering",
        &[
            ("python", 2),
            ("machine learning", 2),
            ("llm", 2),
            ("llms", 2),
            ("ai agents", 2),
            ("generative ai", 2),
            ("mlops", 2),
            ("rag", 2),
            ("fine tuning", 2),
            ("data engineering", 2),
            ("artificial intelligence", 1),
            ("ai", 1),
            ("inference", 1),
        ],
    ),
    (
        "developer_education_writing",
        &[
            ("developer education", 2),
            ("technical writing", 2),
            ("documentation", 2),
            ("tutorials", 2),
            ("developer relations", 2),
            ("devrel", 2),
            ("elearning", 1),
            ("education", 1),
            ("courses", 1),
            ("docs", 1),
        ],
    ),
    (
        "devtools",
        &[
            ("developer tools", 2),
            ("devtools", 2),
            ("devops", 2),
            ("sdk", 2),
            ("open source", 2),
            ("observability", 2),
            ("ci cd", 2),
            ("api", 1),
            ("apis", 1),
            ("developers", 1),
            ("engineering product and design", 1),
        ],
    ),
    (
        "b2b_infra_saas",
        &[
            ("infrastructure", 2),
            ("saas", 2),
            ("cloud computing", 2),
            ("enterprise software", 2),
            ("aiops", 2),
            ("b2b", 1),
            ("enterprise", 1),
            ("databases", 1),
        ],
    ),
    (
        "support_tooling",
        &[
            ("customer support", 2),
            ("customer success", 2),
            ("customer service", 2),
            ("helpdesk", 2),
            ("ticketing", 2),
            ("contact center", 2),
            ("support", 1),
        ],
    ),
];

/// Each category contributes at most this much, so one area cannot dominate.
const CATEGORY_CAP: u32 = 12;
const CUSTOM_CATEGORY: &str = "custom_keywords";
const CUSTOM_TERM_WEIGHT: u32 = 2;
const HIRING_FLAG_POINTS: u32 = 1;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Match {
    pub category: String,
    pub term: String,
    /// Source field the term matched in (best-weighted field only).
    pub field: &'static str,
    pub points: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ScoreBreakdown {
    pub score: u32,
    pub category_scores: Vec<(String, u32)>,
    pub matches: Vec<Match>,
    /// Non-keyword signals and their points, with their limits stated.
    pub signals: Vec<String>,
}

/// Lowercase alphanumeric tokens; punctuation separates (so "CI/CD" -> ["ci","cd"]).
pub fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

fn contains_phrase(hay: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Tokenized text of each scored field. List fields are tokenized per element and
/// separated so phrases cannot span two tags.
fn field_tokens(c: &Company) -> Vec<(&'static str, u32, Vec<Vec<String>>)> {
    let opt = |s: &Option<String>| s.iter().map(|v| tokenize(v)).collect::<Vec<_>>();
    let list = |v: &Vec<String>| v.iter().map(|x| tokenize(x)).collect::<Vec<_>>();
    FIELDS
        .iter()
        .map(|&(name, w)| {
            let toks = match name {
                "tags" => list(&c.tags),
                "industries" => list(&c.industries),
                "subindustry" => opt(&c.subindustry),
                "one_liner" => opt(&c.one_liner),
                _ => opt(&c.long_description),
            };
            (name, w, toks)
        })
        .collect()
}

fn best_field(
    fields: &[(&'static str, u32, Vec<Vec<String>>)],
    term: &[String],
) -> Option<(&'static str, u32)> {
    // FIELDS is ordered by descending weight, so the first hit is the best.
    fields
        .iter()
        .find(|(_, _, parts)| parts.iter().any(|p| contains_phrase(p, term)))
        .map(|(n, w, _)| (*n, *w))
}

pub fn score_company(c: &Company, custom_keywords: &[String]) -> ScoreBreakdown {
    let fields = field_tokens(c);
    let mut matches = Vec::new();
    let mut category_scores = Vec::new();
    let custom: Vec<(String, u32)> = custom_keywords
        .iter()
        .map(|k| (k.clone(), CUSTOM_TERM_WEIGHT))
        .collect();
    let mut cats: Vec<(&str, Vec<(String, u32)>)> = CATEGORIES
        .iter()
        .map(|(n, terms)| (*n, terms.iter().map(|(t, w)| (t.to_string(), *w)).collect()))
        .collect();
    if !custom.is_empty() {
        cats.push((CUSTOM_CATEGORY, custom));
    }
    for (cat, terms) in cats {
        let mut cat_total = 0;
        for (term, tw) in terms {
            let toks = tokenize(&term);
            if let Some((field, fw)) = best_field(&fields, &toks) {
                let points = tw * fw;
                cat_total += points;
                matches.push(Match {
                    category: cat.to_string(),
                    term: toks.join(" "),
                    field,
                    points,
                });
            }
        }
        if cat_total > 0 {
            category_scores.push((cat.to_string(), cat_total.min(CATEGORY_CAP)));
        }
    }
    let mut score: u32 = category_scores.iter().map(|(_, s)| s).sum();
    let mut signals = Vec::new();
    if score > 0 && c.is_hiring == Some(true) {
        score += HIRING_FLAG_POINTS;
        signals.push(format!(
            "+{HIRING_FLAG_POINTS} source isHiring=true (company-level flag; not a vacancy, not location/visa/contract eligibility)"
        ));
    }
    ScoreBreakdown {
        score,
        category_scores,
        matches,
        signals,
    }
}

/// Ranks active companies; ties broken by slug for stable output.
pub fn rank<'a>(
    companies: &'a [Company],
    custom_keywords: &[String],
) -> Vec<(&'a Company, ScoreBreakdown)> {
    let mut scored: Vec<_> = companies
        .iter()
        .filter(|c| c.status.as_deref() == Some("Active"))
        .map(|c| (c, score_company(c, custom_keywords)))
        .filter(|(_, s)| s.score > 0)
        .collect();
    scored.sort_by(|(a, sa), (b, sb)| sb.score.cmp(&sa.score).then_with(|| a.slug.cmp(&b.slug)));
    scored
}

/// Accepts "Winter 2024" or short forms like "W24", "S25", "F24", "X25" (Spring).
pub fn normalize_batch(input: &str) -> String {
    let s = input.trim();
    let mut chars = s.chars();
    if s.len() == 3
        && let (Some(season), true) = (chars.next(), s[1..].chars().all(|c| c.is_ascii_digit()))
    {
        let name = match season.to_ascii_uppercase() {
            'W' => Some("winter"),
            'S' => Some("summer"),
            'F' => Some("fall"),
            'X' => Some("spring"),
            _ => None,
        };
        if let Some(name) = name {
            return format!("{name} 20{}", &s[1..]);
        }
    }
    s.to_lowercase()
}

#[derive(Debug, Clone, Default)]
pub struct SearchFilter {
    pub query: Option<String>,
    pub industry: Option<String>,
    pub batch: Option<String>,
    pub hiring_only: bool,
}

/// Returns the source fields that matched the free-text query (empty when no query).
pub fn query_match_fields(c: &Company, query_tokens: &[String]) -> Vec<&'static str> {
    let mut out = Vec::new();
    let name = tokenize(&c.name);
    if contains_phrase(&name, query_tokens) {
        out.push("name");
    }
    for (field, _, parts) in field_tokens(c) {
        if parts.iter().any(|p| contains_phrase(p, query_tokens)) {
            out.push(field);
        }
    }
    out
}

/// Filters locally; results ordered by number of matched fields, then slug.
pub fn search<'a>(
    companies: &'a [Company],
    f: &SearchFilter,
) -> Vec<(&'a Company, Vec<&'static str>)> {
    let q = f.query.as_deref().map(tokenize).unwrap_or_default();
    let industry = f.industry.as_deref().map(|s| s.trim().to_lowercase());
    let batch = f.batch.as_deref().map(normalize_batch);
    let mut out: Vec<_> = companies
        .iter()
        .filter(|c| !f.hiring_only || c.is_hiring == Some(true))
        .filter(|c| match &batch {
            Some(b) => c.batch.as_deref().map(|x| x.to_lowercase()) == Some(b.clone()),
            None => true,
        })
        .filter(|c| match &industry {
            Some(i) => c
                .industry
                .iter()
                .chain(c.subindustry.iter())
                .chain(c.industries.iter())
                .any(|x| x.to_lowercase().contains(i.as_str())),
            None => true,
        })
        .filter_map(|c| {
            if q.is_empty() {
                return Some((c, Vec::new()));
            }
            let m = query_match_fields(c, &q);
            (!m.is_empty()).then_some((c, m))
        })
        .collect();
    out.sort_by(|(a, ma), (b, mb)| mb.len().cmp(&ma.len()).then_with(|| a.slug.cmp(&b.slug)));
    out
}
