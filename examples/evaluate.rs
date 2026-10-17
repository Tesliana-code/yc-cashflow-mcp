//! Offline evaluation of rank-v1 vs rank-v2 against frozen judgment labels.
//!
//! cargo run --release --example evaluate -- <all.json> <benchmarks/inputs/eval_set.json> <benchmarks/inputs/eval_labels.json> <benchmarks/inputs/top20_decisions.json> <out.json> > report.md
//! (scripts/reproduce_benchmarks.sh runs this against the pinned snapshot.)
//!
//! Metric rules are fixed in benchmarks/METHODOLOGY.md. Labels are judgments, not ground truth.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};
use yc_cashflow_mcp::model::Company;
use yc_cashflow_mcp::rank;
use yc_cashflow_mcp::rank_v2::{self, SupportClass};
use yc_cashflow_mcp::source::parse_companies;

struct Row<'a> {
    c: &'a Company,
    stratum: String,
    relevance: String,
    support_label: String,
    v1: rank::ScoreBreakdown,
    v2: rank_v2::ScoreV2,
}

fn v1_support(s: &rank::ScoreBreakdown) -> bool {
    s.category_scores
        .iter()
        .any(|(k, p)| k == "support_tooling" && *p > 0)
}

fn v2_support(s: &rank_v2::ScoreV2) -> bool {
    s.support.class == SupportClass::Vendor
}

fn class_name(c: SupportClass) -> &'static str {
    match c {
        SupportClass::Vendor => "vendor",
        SupportClass::GenericAgent => "generic_agent",
        SupportClass::UsesVocabulary => "uses_vocabulary",
        SupportClass::None => "none",
    }
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 {
        "n/a (0/0)".into()
    } else {
        format!("{:.1}% ({n}/{d})", 100.0 * n as f64 / d as f64)
    }
}

fn top20<'a>(rows: &'a [Row<'a>], score: impl Fn(&Row) -> u32) -> Vec<&'a Row<'a>> {
    let mut v: Vec<&Row> = rows.iter().collect();
    v.sort_by(|a, b| {
        score(b)
            .cmp(&score(a))
            .then_with(|| a.c.slug.cmp(&b.c.slug))
    });
    v.truncate(20);
    v
}

fn read_json(path: &str) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() == 6,
        "usage: evaluate <all.json> <eval_set.json> <labels.json> <decisions.json> <out.json>"
    );
    let parsed = parse_companies(&std::fs::read(&args[1]).unwrap()).unwrap();
    let by_slug: HashMap<&str, &Company> = parsed
        .companies
        .iter()
        .map(|c| (c.slug.as_str(), c))
        .collect();
    let labels = read_json(&args[3]);
    let decisions = read_json(&args[4]);

    let rows: Vec<Row> = labels["labels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            let c = by_slug[l["slug"].as_str().unwrap()];
            assert_eq!(
                c.id,
                l["id"].as_i64().unwrap(),
                "frozen id mismatch for {}",
                c.slug
            );
            Row {
                c,
                stratum: l["stratum"].as_str().unwrap().into(),
                relevance: l["campaign_relevance"].as_str().unwrap().into(),
                support_label: l["support_class"].as_str().unwrap().into(),
                v1: rank::score_company(c, &[]),
                v2: rank_v2::score_company(c, &[]),
            }
        })
        .collect();
    let eval_set = read_json(&args[2]);
    assert_eq!(eval_set["size"].as_u64().unwrap() as usize, rows.len());

    let mut md = String::new();
    let mut out = json!({});
    macro_rules! w { ($($t:tt)*) => { md.push_str(&format!($($t)*)); md.push('\n'); } }

    w!("# Independent evaluation: rank-v1 vs rank-v2\n");
    w!(
        "Labels are **one annotator's judgment, not ground truth** (see `labels.json`). The eval set had n = {} companies. It was frozen in commit `54f2943`, before rank-v2 existed. rank-v2 was frozen in commit `3b5d059`, before this evaluation ran. Both scorers ran on the same snapshot (sha256 `c3a6c33e…`).\n",
        rows.len()
    );
    w!(
        "Development-knowledge caveat: the annotator also wrote rank-v2 and had read these 70 descriptions while labeling. rank-v2 was designed from the original top-20 findings, and its rules were not tuned after this evaluation ran. Even so, the developer had seen these descriptions, so this evaluation is not blind to the developer.\n"
    );

    // Precision@20
    w!("## Precision@20 (eval set, 70 companies)\n");
    w!(
        "| scorer | strict (relevant) | lenient (relevant+weak) | zero-score in top 20 |\n|---|---|---|---|"
    );
    let mut p20 = BTreeMap::new();
    for (name, list) in [
        ("rank-v1", top20(&rows, |r| r.v1.score)),
        ("rank-v2", top20(&rows, |r| r.v2.score)),
    ] {
        let strict = list.iter().filter(|r| r.relevance == "relevant").count();
        let lenient = list
            .iter()
            .filter(|r| r.relevance == "relevant" || r.relevance == "weak")
            .count();
        let zeros = list
            .iter()
            .filter(|r| {
                if name == "rank-v1" {
                    r.v1.score == 0
                } else {
                    r.v2.score == 0
                }
            })
            .count();
        w!(
            "| {name} | {} | {} | {zeros} |",
            pct(strict, 20),
            pct(lenient, 20)
        );
        p20.insert(name, json!({
            "strict": strict, "lenient": lenient, "denominator": 20, "zero_score": zeros,
            "top20": list.iter().map(|r| json!({"slug": r.c.slug, "label": r.relevance, "v1": r.v1.score, "v2": r.v2.score})).collect::<Vec<_>>()
        }));
    }
    let total_relevant = rows.iter().filter(|r| r.relevance == "relevant").count();
    w!(
        "\nThe eval set contains {total_relevant} companies labeled relevant, so the best possible strict P@20 is {}.\n",
        pct(total_relevant.min(20), 20)
    );
    for name in ["rank-v1", "rank-v2"] {
        let list = p20[name]["top20"].as_array().unwrap();
        w!(
            "- {name} top 20: {}",
            list.iter()
                .map(|x| format!(
                    "{}({}, {})",
                    x["slug"].as_str().unwrap(),
                    x["label"].as_str().unwrap(),
                    x[if name == "rank-v1" { "v1" } else { "v2" }]
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    out["precision_at_20"] = json!(p20);

    // Support tooling
    let positives: Vec<&Row> = rows
        .iter()
        .filter(|r| r.support_label == "vendor")
        .collect();
    w!(
        "\n## Support-tooling detection (positive label = `vendor`, n = {})\n",
        positives.len()
    );
    w!(
        "| scorer | predicted positive | TP | FP | FN | precision | recall |\n|---|---|---|---|---|---|---|"
    );
    let mut sup = BTreeMap::new();
    for name in ["rank-v1", "rank-v2"] {
        let pred = |r: &Row| {
            if name == "rank-v1" {
                v1_support(&r.v1)
            } else {
                v2_support(&r.v2)
            }
        };
        let tp: Vec<&str> = rows
            .iter()
            .filter(|r| pred(r) && r.support_label == "vendor")
            .map(|r| r.c.slug.as_str())
            .collect();
        let fp: Vec<(&str, &str)> = rows
            .iter()
            .filter(|r| pred(r) && r.support_label != "vendor")
            .map(|r| (r.c.slug.as_str(), r.support_label.as_str()))
            .collect();
        let fneg: Vec<&str> = rows
            .iter()
            .filter(|r| !pred(r) && r.support_label == "vendor")
            .map(|r| r.c.slug.as_str())
            .collect();
        let pp = tp.len() + fp.len();
        w!(
            "| {name} | {pp} | {} | {} | {} | {} | {} |",
            tp.len(),
            fp.len(),
            fneg.len(),
            pct(tp.len(), pp),
            pct(tp.len(), positives.len())
        );
        sup.insert(name, json!({"tp": tp, "fp": fp, "fn": fneg, "predicted_positive": pp, "label_positive": positives.len()}));
    }
    for name in ["rank-v1", "rank-v2"] {
        let s = &sup[name];
        w!("\n**{name}**");
        w!(
            "- false positives (slug, label): {}",
            s["fp"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| format!("{} ({})", x[0].as_str().unwrap(), x[1].as_str().unwrap()))
                .collect::<Vec<_>>()
                .join(", ")
        );
        w!(
            "- false negatives: {}",
            s["fn"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    out["support_tooling"] = json!(sup);

    // V2 support class confusion
    w!("\n## rank-v2 support class vs label (all 70)\n");
    let label_classes = [
        "vendor",
        "generic_agent",
        "uses_vocabulary",
        "none",
        "insufficient_info",
    ];
    let pred_classes = [
        SupportClass::Vendor,
        SupportClass::GenericAgent,
        SupportClass::UsesVocabulary,
        SupportClass::None,
    ];
    w!(
        "| label \\ v2 | {} |\n|---|---|---|---|---|",
        pred_classes.map(class_name).join(" | ")
    );
    let mut agree = 0;
    for lc in label_classes {
        let cells: Vec<String> = pred_classes
            .iter()
            .map(|pc| {
                rows.iter()
                    .filter(|r| r.support_label == lc && r.v2.support.class == *pc)
                    .count()
                    .to_string()
            })
            .collect();
        agree += rows
            .iter()
            .filter(|r| r.support_label == lc && class_name(r.v2.support.class) == lc)
            .count();
        w!("| {lc} | {} |", cells.join(" | "));
    }
    w!("\nExact class agreement: {}", pct(agree, rows.len()));

    // Per stratum
    w!("\n## Support detection by stratum (TP/label positives · FP/label negatives)\n");
    w!(
        "| stratum | n | positives | v1 TP | v1 FP | v2 TP | v2 FP |\n|---|---|---|---|---|---|---|"
    );
    let mut strata: Vec<&str> = rows.iter().map(|r| r.stratum.as_str()).collect();
    strata.dedup();
    for st in &strata {
        let rs: Vec<&Row> = rows.iter().filter(|r| r.stratum == *st).collect();
        let pos = rs.iter().filter(|r| r.support_label == "vendor").count();
        let c = |f: &dyn Fn(&Row) -> bool, want: bool| {
            rs.iter()
                .filter(|r| f(r) && (r.support_label == "vendor") == want)
                .count()
        };
        let f1 = |r: &Row| v1_support(&r.v1);
        let f2 = |r: &Row| v2_support(&r.v2);
        w!(
            "| {st} | {} | {pos} | {}/{pos} | {}/{} | {}/{pos} | {}/{} |",
            rs.len(),
            c(&f1, true),
            c(&f1, false),
            rs.len() - pos,
            c(&f2, true),
            c(&f2, false),
            rs.len() - pos
        );
    }

    // Score changes
    w!("\n## Score changes v1 → v2 (eval set, sorted by |Δ|; full list in eval_results.json)\n");
    w!(
        "| slug | label | v1 | v2 | Δ | v2 support class | main reasons |\n|---|---|---|---|---|---|---|"
    );
    let mut changes: Vec<(&Row, i64)> = rows
        .iter()
        .map(|r| (r, r.v2.score as i64 - r.v1.score as i64))
        .collect();
    changes.sort_by(|a, b| {
        b.1.abs()
            .cmp(&a.1.abs())
            .then_with(|| a.0.c.slug.cmp(&b.0.c.slug))
    });
    let reason = |r: &Row| -> String {
        let mut v = Vec::new();
        let v1c: HashMap<&str, u32> =
            r.v1.category_scores
                .iter()
                .map(|(k, p)| (k.as_str(), *p))
                .collect();
        let v2c: HashMap<&str, u32> =
            r.v2.category_scores
                .iter()
                .map(|(k, p)| (k.as_str(), *p))
                .collect();
        let mut keys: Vec<&str> = v1c.keys().chain(v2c.keys()).copied().collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            let (a, b) = (
                v1c.get(k).copied().unwrap_or(0),
                v2c.get(k).copied().unwrap_or(0),
            );
            if a != b {
                v.push(format!("{k} {a}→{b}"));
            }
        }
        if r.v1.matches.iter().any(|m| m.term == "b2b") {
            v.push("b2b removed".into());
        }
        let sup = r.v2.suppressed.len();
        if sup > 0 {
            v.push(format!("{sup} suppressed"));
        }
        v.join("; ")
    };
    for (r, d) in changes.iter().take(25) {
        w!(
            "| {} | {} | {} | {} | {d:+} | {} | {} |",
            r.c.slug,
            r.relevance,
            r.v1.score,
            r.v2.score,
            class_name(r.v2.support.class),
            reason(r)
        );
    }
    out["score_changes"] = json!(changes.iter().map(|(r, d)| json!({
        "slug": r.c.slug, "id": r.c.id, "stratum": r.stratum, "label": r.relevance, "support_label": r.support_label,
        "v1": r.v1.score, "v2": r.v2.score, "delta": d, "reasons": reason(r),
        "v1_categories": r.v1.category_scores, "v2_categories": r.v2.category_scores,
        "v2_support": r.v2.support, "v2_suppressed": r.v2.suppressed
    })).collect::<Vec<_>>());

    // Regression: original top 20
    w!(
        "\n## Regression set: original top 20 (seen during rank-v2 development, so this does NOT show generalization)\n"
    );
    let all_v2 = rank_v2::rank(&parsed.companies, &[]);
    let v2_pos: HashMap<&str, usize> = all_v2
        .iter()
        .enumerate()
        .map(|(i, (c, _))| (c.slug.as_str(), i + 1))
        .collect();
    let all_v1 = rank::rank(&parsed.companies, &[]);
    let v1_pos: HashMap<&str, usize> = all_v1
        .iter()
        .enumerate()
        .map(|(i, (c, _))| (c.slug.as_str(), i + 1))
        .collect();
    w!(
        "| slug | audit label | v1 rank | v2 rank (of {} scored) | v1 support pts | v2 support class |\n|---|---|---|---|---|---|",
        all_v2.len()
    );
    let dec = decisions["decisions"].as_object().unwrap();
    let mut reg = Vec::new();
    let (mut v1_sup_fp, mut v2_sup_fp, mut weak_in_v2_top20, mut weak_total) = (0, 0, 0, 0);
    for (slug, d) in dec {
        let c = by_slug[slug.as_str()];
        let s1 = rank::score_company(c, &[]);
        let s2 = rank_v2::score_company(c, &[]);
        let sp = s1
            .category_scores
            .iter()
            .find(|(k, _)| k == "support_tooling")
            .map(|x| x.1)
            .unwrap_or(0);
        v1_sup_fp += (sp > 0) as usize;
        v2_sup_fp += v2_support(&s2) as usize;
        let label = d[0].as_str().unwrap();
        let r2 = v2_pos.get(slug.as_str()).copied();
        if label == "weak" {
            weak_total += 1;
            weak_in_v2_top20 += r2.is_some_and(|p| p <= 20) as usize;
        }
        w!(
            "| {slug} | {label} | {} | {} | {sp} | {} |",
            v1_pos[slug.as_str()],
            r2.map(|p| p.to_string()).unwrap_or("unscored".into()),
            class_name(s2.support.class)
        );
        reg.push(json!({"slug": slug, "label": label, "v1_rank": v1_pos[slug.as_str()], "v2_rank": r2, "v1_support_points": sp, "v2_support_class": class_name(s2.support.class)}));
    }
    w!("\nNone of the original top 20 was labeled a support vendor in the audit.");
    w!(
        "- Companies predicted as support positives: rank-v1 {v1_sup_fp}/20, rank-v2 {v2_sup_fp}/20."
    );
    w!(
        "- Audit-weak companies that stay in the full-population top 20: rank-v1 {weak_total}/{weak_total} (by construction), rank-v2 {weak_in_v2_top20}/{weak_total}."
    );
    out["regression_top20"] = json!({"rows": reg, "v1_support_positive": v1_sup_fp, "v2_support_positive": v2_sup_fp, "weak_total": weak_total, "weak_in_v2_top20": weak_in_v2_top20, "v2_population_scored": all_v2.len(), "v1_population_scored": all_v1.len()});

    std::fs::write(&args[5], serde_json::to_vec_pretty(&out).unwrap()).unwrap();
    print!("{md}");
}
