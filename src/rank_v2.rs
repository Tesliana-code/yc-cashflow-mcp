//! rank-v2: field-aware scoring with an explicit support-tooling taxonomy.
//!
//! Differences from rank-v1 (which is kept unchanged in `rank.rs`):
//! - Generic terms (weight 1) only count in curated fields and the one-liner, never in
//!   long_description, where they mostly appear as prose.
//! - Non-discriminative terms are dropped (`b2b`, `enterprise`, bare `support`).
//! - Developer-education terms require a developer-context gate.
//! - Support tooling is a classification (`vendor`, `generic_agent`, `uses_vocabulary`,
//!   `none`) built from contextual evidence. Only `vendor` earns category points.
//! - Every match carries a source snippet and the rule that produced it. Suppressed
//!   matches are reported with the reason.

use serde::Serialize;

use crate::model::Company;
use crate::rank::tokenize;

pub const RANK_V2_VERSION: &str = "rank-v2";

const CATEGORY_CAP: u32 = 12;
const HIRING_FLAG_POINTS: u32 = 1;
const CUSTOM_TERM_WEIGHT: u32 = 2;
const SNIPPET_CHARS: usize = 50;

/// (field, weight, curated?)
const FIELDS: &[(&str, u32, bool)] = &[
    ("tags", 3, true),
    ("industries", 3, true),
    ("subindustry", 3, true),
    ("one_liner", 2, false),
    ("long_description", 1, false),
];

const CATEGORIES: &[(&str, &[(&str, u32)])] = &[
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
            ("developer docs", 2),
            ("education", 1),
            ("elearning", 1),
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
            ("databases", 1),
        ],
    ),
];

/// Phrases that state a support/incident product. Bare "support" or "ticketing" never count.
const SUPPORT_PHRASES: &[&str] = &[
    "customer support",
    "customer service",
    "customer care",
    "help desk",
    "helpdesk",
    "service desk",
    "support desk",
    "support ticket",
    "support tickets",
    "support queue",
    "support ops",
    "support operations",
    "support platform",
    "support agent",
    "support agents",
    "support team",
    "support teams",
    "contact center",
    "contact centers",
    "call center",
    "call centers",
];
/// Incident phrases only count for software companies (see `software_context`).
const INCIDENT_PHRASES: &[&str] = &[
    "incident response",
    "incident management",
    "production incidents",
    "on call",
    "sre",
];
const SUPPORT_TAGS_STRONG: &[&str] = &["Customer Support", "Customer Service", "Ticketing"];
const SUPPORT_TAGS_WEAK: &[&str] = &["Call Center", "Customer Success"];
const SOFTWARE_TAGS: &[&str] = &[
    "Developer Tools",
    "DevOps",
    "AIOps",
    "Infrastructure",
    "Monitoring",
    "DevSecOps",
];
const DEV_CONTEXT_TAGS: &[&str] = &["Developer Tools", "API", "APIs", "Open Source", "DevOps"];
const DEV_CONTEXT_TOKENS: &[&str] = &["developer", "developers", "api", "apis", "sdk", "sdks"];
/// Sentence markers that turn a phrase into an example use case rather than a product claim.
const USE_CASE_MARKERS: &[&str] = &[
    "ideal for",
    "such as",
    "for example",
    "e g",
    "including",
    "use cases",
    "use case",
    "have used",
    "can be used",
    "used to",
];
const AGENT_PLATFORM_PHRASES: &[&str] = &[
    "ai agents",
    "ai agent",
    "voice agents",
    "voice ai agents",
    "agent platform",
    "build agents",
    "deploy agents",
    "memory layer",
];

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupportClass {
    Vendor,
    GenericAgent,
    UsesVocabulary,
    None,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Evidence {
    pub rule: &'static str,
    pub field: &'static str,
    pub term: String,
    pub points: u32,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Suppressed {
    pub term: String,
    pub field: &'static str,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SupportAssessment {
    pub class: SupportClass,
    pub evidence_points: u32,
    pub evidence: Vec<Evidence>,
    pub rule: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ScoreV2 {
    pub score: u32,
    pub category_scores: Vec<(String, u32)>,
    pub matches: Vec<Evidence>,
    pub suppressed: Vec<Suppressed>,
    pub support: SupportAssessment,
    pub signals: Vec<String>,
}

/// A text unit (one tag, or one sentence) with its token positions in the original string.
struct Unit<'a> {
    text: &'a str,
    toks: Vec<(String, usize, usize)>,
}

fn unit(text: &str) -> Unit<'_> {
    let mut toks = Vec::new();
    let mut start = None;
    for (i, ch) in text.char_indices() {
        if ch.is_alphanumeric() {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            toks.push((text[s..i].to_lowercase(), s, i));
        }
    }
    if let Some(s) = start {
        toks.push((text[s..].to_lowercase(), s, text.len()));
    }
    Unit { text, toks }
}

impl Unit<'_> {
    /// Byte span of the first occurrence of the token phrase.
    fn find(&self, phrase: &[String]) -> Option<(usize, usize)> {
        if phrase.is_empty() || self.toks.len() < phrase.len() {
            return None;
        }
        (0..=self.toks.len() - phrase.len())
            .find(|&i| {
                self.toks[i..i + phrase.len()]
                    .iter()
                    .zip(phrase)
                    .all(|(t, p)| &t.0 == p)
            })
            .map(|i| (self.toks[i].1, self.toks[i + phrase.len() - 1].2))
    }

    fn contains(&self, phrase: &str) -> bool {
        self.find(&tokenize(phrase)).is_some()
    }

    fn snippet(&self, (s, e): (usize, usize)) -> String {
        let before: String = {
            let rev: String = self.text[..s].chars().rev().take(SNIPPET_CHARS).collect();
            rev.chars().rev().collect()
        };
        let after: String = self.text[e..].chars().take(SNIPPET_CHARS).collect();
        format!("{before}[{}]{after}", &self.text[s..e]).replace(['\r', '\n'], " ")
    }
}

fn sentences(text: &str) -> Vec<&str> {
    text.split(['.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Text units per field: one per tag/industry, one per sentence for prose.
fn field_units(c: &Company) -> Vec<(&'static str, u32, bool, Vec<Unit<'_>>)> {
    FIELDS
        .iter()
        .map(|&(name, w, curated)| {
            let units = match name {
                "tags" => c.tags.iter().map(|t| unit(t)).collect(),
                "industries" => c.industries.iter().map(|t| unit(t)).collect(),
                "subindustry" => c.subindustry.iter().map(|t| unit(t)).collect(),
                "one_liner" => c.one_liner.iter().map(|t| unit(t)).collect(),
                _ => c
                    .long_description
                    .as_deref()
                    .map(|d| sentences(d).into_iter().map(unit).collect())
                    .unwrap_or_default(),
            };
            (name, w, curated, units)
        })
        .collect()
}

fn has_tag(c: &Company, set: &[&str]) -> bool {
    c.tags.iter().any(|t| set.contains(&t.as_str()))
}

fn dev_context(c: &Company) -> bool {
    has_tag(c, DEV_CONTEXT_TAGS)
        || [c.one_liner.as_deref(), c.long_description.as_deref()]
            .into_iter()
            .flatten()
            .any(|t| {
                tokenize(t)
                    .iter()
                    .any(|x| DEV_CONTEXT_TOKENS.contains(&x.as_str()))
            })
}

fn software_context(c: &Company) -> bool {
    has_tag(c, SOFTWARE_TAGS)
}

fn is_use_case(u: &Unit<'_>) -> bool {
    USE_CASE_MARKERS.iter().any(|m| u.contains(m))
}

pub fn assess_support(c: &Company) -> SupportAssessment {
    let fields = field_units(c);
    let software = software_context(c);
    let mut evidence = Vec::new();
    let mut use_case_mentions = 0;
    let mut vocab_mentions = 0;
    for tag in &c.tags {
        let (rule, points) = if SUPPORT_TAGS_STRONG.contains(&tag.as_str()) {
            ("support_tag", 2)
        } else if SUPPORT_TAGS_WEAK.contains(&tag.as_str()) {
            ("support_adjacent_tag", 1)
        } else {
            continue;
        };
        evidence.push(Evidence {
            rule,
            field: "tags",
            term: tag.clone(),
            points,
            snippet: tag.clone(),
        });
    }
    for (field, _, curated, units) in &fields {
        if *curated {
            continue;
        }
        for u in units {
            let phrases = SUPPORT_PHRASES
                .iter()
                .map(|p| (*p, "support_phrase"))
                .chain(INCIDENT_PHRASES.iter().map(|p| (*p, "incident_phrase")));
            for (phrase, kind) in phrases {
                let toks = tokenize(phrase);
                let Some(span) = u.find(&toks) else { continue };
                if kind == "incident_phrase" && !software {
                    vocab_mentions += 1;
                    continue;
                }
                if is_use_case(u) {
                    use_case_mentions += 1;
                    continue;
                }
                let (rule, points) = match *field {
                    "one_liner" => ("product_statement_one_liner", 3),
                    _ => ("product_statement_description", 2),
                };
                evidence.push(Evidence {
                    rule,
                    field,
                    term: toks.join(" "),
                    points,
                    snippet: u.snippet(span),
                });
            }
        }
    }
    let evidence_points: u32 = evidence.iter().map(|e| e.points).sum();
    let text_evidence = evidence.iter().any(|e| e.field != "tags");
    let agent_platform = fields
        .iter()
        .filter(|(_, _, curated, _)| !curated)
        .flat_map(|(_, _, _, units)| units)
        .any(|u| AGENT_PLATFORM_PHRASES.iter().any(|p| u.contains(p)));
    let (class, rule) = if evidence_points >= 3 && text_evidence {
        (
            SupportClass::Vendor,
            "evidence_points>=3 including a product statement",
        )
    } else if agent_platform && (use_case_mentions > 0 || !evidence.is_empty()) {
        (
            SupportClass::GenericAgent,
            "agent platform; support appears only as a use case or weak evidence",
        )
    } else if !evidence.is_empty() || use_case_mentions > 0 || vocab_mentions > 0 {
        (
            SupportClass::UsesVocabulary,
            "support vocabulary without a sufficient product statement",
        )
    } else {
        (SupportClass::None, "no support evidence")
    };
    SupportAssessment {
        class,
        evidence_points,
        evidence,
        rule,
    }
}

pub fn score_company(c: &Company, custom_keywords: &[String]) -> ScoreV2 {
    let fields = field_units(c);
    let dev = dev_context(c);
    let mut matches = Vec::new();
    let mut suppressed = Vec::new();
    let mut category_scores = Vec::new();
    let mut cats: Vec<(&str, Vec<(String, u32)>)> = CATEGORIES
        .iter()
        .map(|(n, terms)| (*n, terms.iter().map(|(t, w)| (t.to_string(), *w)).collect()))
        .collect();
    if !custom_keywords.is_empty() {
        cats.push((
            "custom_keywords",
            custom_keywords
                .iter()
                .map(|k| (k.clone(), CUSTOM_TERM_WEIGHT))
                .collect(),
        ));
    }
    for (cat, terms) in cats {
        let mut total = 0;
        for (term, tw) in terms {
            let toks = tokenize(&term);
            // FIELDS is ordered by weight; the first eligible field wins.
            let mut hit = None;
            for (field, fw, _, units) in &fields {
                let Some((u, span)) = units.iter().find_map(|u| u.find(&toks).map(|s| (u, s)))
                else {
                    continue;
                };
                if tw == 1 && *field == "long_description" && cat != "custom_keywords" {
                    suppressed.push(Suppressed {
                        term: toks.join(" "),
                        field,
                        reason: "generic term in long_description",
                    });
                    continue;
                }
                hit = Some((*field, *fw, u.snippet(span)));
                break;
            }
            let Some((field, fw, snippet)) = hit else {
                continue;
            };
            if cat == "developer_education_writing" && !dev {
                suppressed.push(Suppressed {
                    term: toks.join(" "),
                    field,
                    reason: "no developer context (tag or developer/api/sdk token)",
                });
                continue;
            }
            let points = tw * fw;
            total += points;
            matches.push(Evidence {
                rule: cat_rule(cat),
                field,
                term: toks.join(" "),
                points,
                snippet,
            });
        }
        if total > 0 {
            category_scores.push((cat.to_string(), total.min(CATEGORY_CAP)));
        }
    }
    let support = assess_support(c);
    if support.class == SupportClass::Vendor {
        category_scores.push((
            "support_tooling".into(),
            (support.evidence_points * 2).min(CATEGORY_CAP),
        ));
    }
    let mut score: u32 = category_scores.iter().map(|(_, s)| s).sum();
    let mut signals = Vec::new();
    if score > 0 && c.is_hiring == Some(true) {
        score += HIRING_FLAG_POINTS;
        signals.push(format!(
            "+{HIRING_FLAG_POINTS} source isHiring=true (company-level flag; not a vacancy, not location/visa/contract eligibility)"
        ));
    }
    ScoreV2 {
        score,
        category_scores,
        matches,
        suppressed,
        support,
        signals,
    }
}

fn cat_rule(cat: &str) -> &'static str {
    match cat {
        "python_ai_engineering" => "term:python_ai_engineering",
        "developer_education_writing" => "term:developer_education_writing",
        "devtools" => "term:devtools",
        "b2b_infra_saas" => "term:b2b_infra_saas",
        _ => "term:custom_keywords",
    }
}

/// Ranks Active companies with rank-v2; ties broken by slug.
pub fn rank<'a>(
    companies: &'a [Company],
    custom_keywords: &[String],
) -> Vec<(&'a Company, ScoreV2)> {
    let mut scored: Vec<_> = companies
        .iter()
        .filter(|c| c.status.as_deref() == Some("Active"))
        .map(|c| (c, score_company(c, custom_keywords)))
        .filter(|(_, s)| s.score > 0)
        .collect();
    scored.sort_by(|(a, sa), (b, sb)| sb.score.cmp(&sa.score).then_with(|| a.slug.cmp(&b.slug)));
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    fn co(one_liner: &str, desc: &str, tags: &[&str]) -> Company {
        serde_json::from_value(serde_json::json!({
            "id": 1, "name": "T", "slug": "t", "status": "Active",
            "one_liner": one_liner, "long_description": desc, "tags": tags,
        }))
        .unwrap()
    }

    #[test]
    fn vendor_needs_product_statement() {
        let c = co("AI-native customer support platform", "", &[]);
        assert_eq!(assess_support(&c).class, SupportClass::Vendor);
        // tag alone is not enough
        let c = co("Marketing on WeChat", "", &["Customer Support"]);
        assert_eq!(assess_support(&c).class, SupportClass::UsesVocabulary);
    }

    #[test]
    fn use_case_mentions_do_not_make_a_vendor() {
        let c = co(
            "The memory layer for AI agents",
            "Ideal for customer support chatbots and assistants.",
            &[],
        );
        assert_eq!(assess_support(&c).class, SupportClass::GenericAgent);
        let c = co(
            "Payroll",
            "We provide expert support. Native support for PPTX.",
            &[],
        );
        assert_eq!(assess_support(&c).class, SupportClass::None);
    }

    #[test]
    fn incident_phrases_need_software_context() {
        let c = co(
            "Security",
            "We provide incident response services for harassment victims.",
            &["Security"],
        );
        assert_eq!(assess_support(&c).class, SupportClass::UsesVocabulary);
        let c = co("On call and incident management", "", &["DevOps"]);
        assert_eq!(assess_support(&c).class, SupportClass::Vendor);
    }

    #[test]
    fn generic_terms_suppressed_in_description_and_snippets_present() {
        let c = co("Bread delivery", "We use AI and APIs everywhere.", &[]);
        let s = score_company(&c, &[]);
        assert_eq!(s.score, 0);
        assert!(
            s.suppressed
                .iter()
                .any(|x| x.term == "ai" && x.field == "long_description")
        );
        let c = co("Python SDK for LLM apps", "", &["Developer Tools"]);
        let s = score_company(&c, &[]);
        assert!(s.matches.iter().all(|m| m.snippet.contains('[')));
    }

    #[test]
    fn dev_education_requires_developer_context() {
        let c = co(
            "Instructional videos and documentation for employee training",
            "",
            &["SaaS"],
        );
        let s = score_company(&c, &[]);
        assert!(
            !s.category_scores
                .iter()
                .any(|(k, _)| k == "developer_education_writing")
        );
        let c = co("Documentation platform for API companies", "", &[]);
        let s = score_company(&c, &[]);
        assert!(
            s.category_scores
                .iter()
                .any(|(k, _)| k == "developer_education_writing")
        );
    }
}
