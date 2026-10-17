#!/usr/bin/env python3
"""Deterministic selection of the independent evaluation batch.

Usage: eval_select.py <snapshot all.json> <top-20 exclusions> > eval_set.json

<top-20 exclusions> is either benchmarks/inputs/top20_decisions.json (its `decisions` keys)
or the original harness transcript of the top-20 audit (lines prefixed "<< ").

Uses source fields only (never rank-v1 or rank-v2 scores). Strata are filled in order;
within a stratum, candidates are ordered by sha256(SEED + slug). A company is used once
(first stratum wins). Companies from the original top-20 audit are excluded.
"""
import hashlib
import json
import re
import sys

SEED = "cashflow-eval-2026-10-17:"
SUPPORT_TAGS = {"Customer Support", "Customer Service", "Customer Success", "Ticketing", "Call Center"}
DEV_TAGS = {"Developer Tools", "Infrastructure", "DevOps", "Open Source", "AIOps", "Machine Learning", "Monitoring"}
SUPPORT_VOCAB = re.compile(r"\b(support|customer service|customer success|help ?desk|ticket(s|ing)?)\b", re.I)
VENDOR_VOCAB = re.compile(r"\b(help ?desk|ticketing|support (team|agents?|operations|platform)|customer support (platform|software|automation|ai))\b", re.I)
INCIDENT = re.compile(r"\bincidents?\b", re.I)

STRATA = [
    ("devtools_infra_ai", 14, lambda c: bool(DEV_TAGS & set(c["tags"]))),
    ("support_tagged", 14, lambda c: bool(SUPPORT_TAGS & set(c["tags"]))),
    ("incident_response", 6, lambda c: bool(INCIDENT.search(text(c))) and bool({"DevOps", "Monitoring", "Developer Tools", "Infrastructure", "Security"} & set(c["tags"]))),
    ("support_vendor_untagged", 6, lambda c: not (SUPPORT_TAGS & set(c["tags"])) and bool(VENDOR_VOCAB.search(text(c)))),
    ("hard_negative_support_vocab", 14, lambda c: bool(SUPPORT_VOCAB.search(text(c))) and not (SUPPORT_TAGS & set(c["tags"])) and not (DEV_TAGS & set(c["tags"]))),
    ("incomplete_or_ambiguous", 8, lambda c: len(c.get("long_description") or "") < 100 or not c["tags"]),
    ("random_active_control", 8, lambda c: True),
]


def text(c):
    return " ".join(filter(None, [c.get("one_liner"), c.get("long_description")]))


def key(c):
    return hashlib.sha256((SEED + c["slug"]).encode()).hexdigest()


def top20_slugs(path):
    if path.endswith(".json"):
        return set(json.load(open(path))["decisions"])
    lines = [json.loads(l[3:]) for l in open(path) if l.startswith("<< ")]
    rank = [r["result"]["structuredContent"] for r in lines if "structuredContent" in r.get("result", {})][0]
    return {x["slug"] for x in rank["results"]}


def main():
    companies = json.load(open(sys.argv[1], "rb"))
    excluded = top20_slugs(sys.argv[2])
    pool = sorted((c for c in companies if c.get("status") == "Active" and c["slug"] not in excluded), key=key)
    used, out = set(), []
    for name, n, pred in STRATA:
        picked = [c for c in pool if c["slug"] not in used and pred(c)][:n]
        for c in picked:
            used.add(c["slug"])
            out.append({"id": c["id"], "slug": c["slug"], "stratum": name})
    json.dump({"seed": SEED, "excluded_original_top20": sorted(excluded), "size": len(out), "companies": out}, sys.stdout, indent=1)


if __name__ == "__main__":
    main()
