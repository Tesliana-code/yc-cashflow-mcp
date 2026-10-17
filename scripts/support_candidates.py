#!/usr/bin/env python3
"""Recall-oriented candidate generation for support-tooling vendors over the FULL dataset.

Usage: support_candidates.py <snapshot all.json> > candidates.json

Deliberately broader than rank-v1 and rank-v2. It includes every status (Active,
Inactive, Acquired, Public) and vocabulary that neither scorer uses (CX, Zendesk,
Intercom, knowledge base, live chat, ...). Records each matched source field and
snippet. A candidate is NOT a label; labels are assigned separately against the
written criteria.
"""
import json
import re
import sys

TAGS = {"Customer Support", "Customer Service", "Customer Success", "Ticketing", "Call Center", "Chatbot", "Chatbots"}
RX = re.compile(
    r"\b(customer support|customer service|customer care|customer experience|cx|help ?desk|service desk|"
    r"support (tickets?|queues?|teams?|agents?|ops|operations|platform|desk|automation|inbox)|"
    r"tickets? (resolution|triage|routing|deflection)|contact cent(er|re)s?|call cent(er|re)s?|"
    r"incident (response|management)|on-?call|\bsre\b|zendesk|intercom|freshdesk|knowledge base|live chat|"
    r"it support|employee support|technical support)\b",
    re.I,
)


def snip(text, m, w=60):
    return text[max(0, m.start() - w):m.end() + w].replace("\r", " ").replace("\n", " ").strip()


def main():
    out = []
    for c in json.load(open(sys.argv[1], "rb")):
        ev = [{"field": "tags", "match": t} for t in c["tags"] if t in TAGS]
        for f in ("one_liner", "long_description"):
            t = c.get(f) or ""
            for m in list(RX.finditer(t))[:3]:
                ev.append({"field": f, "match": m.group(0), "snippet": snip(t, m)})
        if ev:
            out.append({"id": c["id"], "slug": c["slug"], "name": c["name"], "status": c["status"], "yc_page": c["url"],
                        "website": c.get("website"), "batch": c["batch"], "one_liner": c.get("one_liner"),
                        "tags": c["tags"], "has_description": bool((c.get("long_description") or "").strip()),
                        "evidence": ev})
    json.dump({"generator": "scripts/support_candidates.py", "size": len(out), "candidates": out}, sys.stdout, indent=1, ensure_ascii=False)


if __name__ == "__main__":
    main()
