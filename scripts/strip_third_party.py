#!/usr/bin/env python3
"""Remove third-party text from benchmark JSON so the result can be committed.

Usage: strip_third_party.py <in.json> > out.json

Drops every key whose value is copied from the yc-oss dataset (names, websites,
YC page URLs, tags, batch, one-liners, descriptions, matched snippets). What remains
is slugs, ids, strata, scores, ranks, matched scorer terms, and this project's own
labels and notes. Output is deterministic (sorted keys, indent=1, trailing newline).
"""
import json
import sys

THIRD_PARTY_KEYS = {"name", "website", "yc_page", "tags", "batch", "one_liner", "long_description", "snippet"}


def strip(x):
    if isinstance(x, dict):
        return {k: strip(v) for k, v in x.items() if k not in THIRD_PARTY_KEYS}
    if isinstance(x, list):
        return [strip(v) for v in x]
    return x


def main():
    data = json.load(open(sys.argv[1], "rb"))
    json.dump(strip(data), sys.stdout, indent=1, sort_keys=True, ensure_ascii=False)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
