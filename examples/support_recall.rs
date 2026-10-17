//! Full-population support-vendor recall for rank-v1 (and rank-v2 for reference).
//!
//! cargo run --release --example support_recall -- <all.json> <benchmarks/inputs/support_vendor_labels.json> <out.json>

use std::collections::HashMap;

use serde_json::{Value, json};
use yc_cashflow_mcp::{rank, rank_v2, source::parse_companies};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let parsed = parse_companies(&std::fs::read(&a[1]).unwrap()).unwrap();
    let labels: Value = serde_json::from_slice(&std::fs::read(&a[2]).unwrap()).unwrap();
    let v1 = rank::rank(&parsed.companies, &[]);
    let v2 = rank_v2::rank(&parsed.companies, &[]);
    let pos = |list: Vec<(&str, u32)>| -> HashMap<String, (usize, u32)> {
        list.into_iter()
            .enumerate()
            .map(|(i, (s, sc))| (s.to_string(), (i + 1, sc)))
            .collect()
    };
    let p1 = pos(v1.iter().map(|(c, s)| (c.slug.as_str(), s.score)).collect());
    let p2 = pos(v2.iter().map(|(c, s)| (c.slug.as_str(), s.score)).collect());
    let by_slug: HashMap<&str, _> = parsed
        .companies
        .iter()
        .map(|c| (c.slug.as_str(), c))
        .collect();
    let mut rows = Vec::new();
    for l in labels["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["label"] == "vendor")
    {
        let slug = l["slug"].as_str().unwrap();
        let c = by_slug[slug];
        let active = c.status.as_deref() == Some("Active");
        let s1 = rank::score_company(c, &[]);
        let bucket = if !active {
            "population_exclusion: status != Active (scorers rank Active only)"
        } else if !p1.contains_key(slug) {
            "ranking_failure: Active but rank-v1 score 0"
        } else if p1[slug].0 > 20 {
            "ranking_failure: Active, scored, rank > 20"
        } else {
            "hit: rank-v1 top 20"
        };
        rows.push(json!({
            "slug": slug, "name": c.name, "status": c.status, "yc_page": c.url, "website": c.website,
            "one_liner": c.one_liner, "tags": c.tags, "label_reason": l["label_reason"], "candidate_evidence": l["evidence"],
            "v1_rank": p1.get(slug).map(|x| x.0), "v1_score": s1.score,
            "v1_support_points": s1.category_scores.iter().find(|(k, _)| k == "support_tooling").map(|x| x.1).unwrap_or(0),
            "v1_matches": s1.matches,
            "v2_rank": p2.get(slug).map(|x| x.0), "v2_score": p2.get(slug).map(|x| x.1).unwrap_or(0),
            "bucket": bucket,
        }));
    }
    rows.sort_by_key(|r| r["v1_rank"].as_u64().unwrap_or(u64::MAX));
    let n = rows.len();
    let active = rows.iter().filter(|r| r["status"] == "Active").count();
    let at = |k: &str, cut: u64| {
        rows.iter()
            .filter(|r| r[k].as_u64().is_some_and(|x| x <= cut))
            .count()
    };
    let recall_at: Vec<Value> = [20u64, 50, 100, 250, 500, 1000]
        .iter()
        .map(|&k| json!({"k": k, "v1_hits": at("v1_rank", k), "v2_hits": at("v2_rank", k), "denominator_all": n, "denominator_active": active}))
        .collect();
    let bucket_names = [
        "hit: rank-v1 top 20",
        "ranking_failure: Active, scored, rank > 20",
        "ranking_failure: Active but rank-v1 score 0",
        "population_exclusion: status != Active (scorers rank Active only)",
    ];
    let buckets: Vec<Value> = bucket_names
        .iter()
        .map(|b| json!({"bucket": b, "count": rows.iter().filter(|r| r["bucket"] == *b).count()}))
        .collect();
    let summary = json!({
        "vendors_total": n, "vendors_active": active,
        "v1_population_scored": v1.len(), "v2_population_scored": v2.len(),
        "v1_highest_ranked_vendor": rows.first().map(|r| json!({"slug": r["slug"], "rank": r["v1_rank"], "score": r["v1_score"]})),
        "recall_at": recall_at,
        "buckets": buckets,
    });
    let mut v2_sorted: Vec<&Value> = rows.iter().filter(|r| r["v2_rank"].is_u64()).collect();
    v2_sorted.sort_by_key(|r| r["v2_rank"].as_u64());
    let out = json!({"summary": summary, "v2_highest_ranked_vendor": v2_sorted.first().map(|r| json!({"slug": r["slug"], "rank": r["v2_rank"]})), "vendors": rows});
    std::fs::write(&a[3], serde_json::to_vec_pretty(&out).unwrap()).unwrap();
    println!("{}", serde_json::to_string_pretty(&out["summary"]).unwrap());
    println!("v2_highest: {}", out["v2_highest_ranked_vendor"]);
}
