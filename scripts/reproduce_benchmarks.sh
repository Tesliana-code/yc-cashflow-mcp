#!/usr/bin/env bash
# Reproduce the recorded benchmark results from the pinned upstream snapshot.
#
# Usage: scripts/reproduce_benchmarks.sh [dataset-dir]
#
# 1. fetch + sha256-verify the pinned yc-oss snapshot (scripts/fetch_dataset.sh)
# 2. re-run the deterministic eval-set selection and support-candidate generation and
#    check they select exactly the slugs in benchmarks/inputs/
# 3. run examples/evaluate.rs and examples/support_recall.rs
# 4. strip third-party text (scripts/strip_third_party.py) and compare with benchmarks/results/
# 5. serve the snapshot through the MCP server (fixture mode) and check rank-v1 top-20 = the audited top 20
# Full (unstripped) outputs stay in the temp dir only and are deleted on exit.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
DS="${1:-$REPO/data/yc-oss-111ff1c}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
B="$REPO/benchmarks"
PY=(python3 -I)
step() { printf '\n== %s\n' "$1"; }

step "dataset"
"$REPO/scripts/fetch_dataset.sh" "$DS"

step "eval-set selection reproduces benchmarks/inputs/eval_set.json"
"${PY[@]}" "$REPO/scripts/eval_select.py" "$DS/all.json" "$B/inputs/top20_decisions.json" > "$TMP/eval_set.json"
"${PY[@]}" "$REPO/scripts/strip_third_party.py" "$TMP/eval_set.json" | cmp - "$B/inputs/eval_set.json" && echo "identical"

step "support candidates reproduce the labeled slug list"
"${PY[@]}" "$REPO/scripts/support_candidates.py" "$DS/all.json" > "$TMP/cand.json"
"${PY[@]}" - "$TMP/cand.json" "$B/inputs/support_vendor_labels.json" <<'PY'
import json, sys
a = [c["slug"] for c in json.load(open(sys.argv[1]))["candidates"]]
b = [c["slug"] for c in json.load(open(sys.argv[2]))["candidates"]]
assert a == b, f"candidate slugs differ ({len(a)} generated vs {len(b)} labeled)"
print(f"identical ({len(a)} candidates)")
PY

step "evaluate + support_recall"
(cd "$REPO" && cargo run --release --locked --quiet --example evaluate -- "$DS/all.json" \
  "$B/inputs/eval_set.json" "$B/inputs/eval_labels.json" "$B/inputs/top20_decisions.json" \
  "$TMP/eval_results.json" > "$TMP/report.md")
(cd "$REPO" && cargo run --release --locked --quiet --example support_recall -- "$DS/all.json" \
  "$B/inputs/support_vendor_labels.json" "$TMP/vendor_ranks.json" > /dev/null)
for f in eval_results.json vendor_ranks.json; do
  "${PY[@]}" "$REPO/scripts/strip_third_party.py" "$TMP/$f" | cmp - "$B/results/$f" && echo "$f identical"
done

step "MCP server on the pinned snapshot: rank-v1 top 20 = audited top 20"
(cd "$REPO" && cargo build --release --locked --quiet)
BIN="${CARGO_TARGET_DIR:-$REPO/target}/release/yc-cashflow-mcp"
YC_MCP_FIXTURE_DIR="$DS" YC_MCP_CACHE_DIR="$TMP/cache" "${PY[@]}" "$REPO/scripts/jsonrpc_harness.py" "$BIN" \
  rank_yc_prospects '{"limit":20}' > "$TMP/rank20.txt"
"${PY[@]}" - "$TMP/rank20.txt" "$B/inputs/top20_decisions.json" <<'PY'
import json, sys
rs = [json.loads(l[3:]) for l in open(sys.argv[1]) if l.startswith("<< ")]
sc = [r["result"]["structuredContent"] for r in rs if "structuredContent" in r.get("result", {})][0]
got = {x["slug"] for x in sc["results"]}
want = set(json.load(open(sys.argv[2]))["decisions"])
assert got == want, f"top-20 differs: +{sorted(got - want)} -{sorted(want - got)}"
print(f"identical ({len(got)} slugs, total_scored={sc['total_scored']})")
PY

step "REPRODUCE PASS"
