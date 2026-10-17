#!/usr/bin/env bash
# Clean-checkout install smoke test using git + standard Rust tooling.
# 1. `git clone` the committed HEAD into an empty temp dir (uncommitted files are NOT included)
#    and check that the checkout contains no dataset copy and no private evidence
# 2. run `cargo fmt --check`, `cargo clippy` and `cargo test --locked` there with a fresh target dir
# 3. `cargo install --locked --root <tmp>` from the extracted source (fresh target dir)
# 4. drive the installed binary over stdio (initialize -> tools/list -> tools/call) in fixture mode
# 5. exercise the documented one-command launch: `cargo run --release --locked --quiet`
# Nothing is installed outside the temp dir. No network fetch of YC data (fixture mode).
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
FIX_ENV=(env YC_MCP_FIXTURE_DIR="$TMP/src/tests/fixtures" YC_MCP_CACHE_DIR="$TMP/cache")
step() { printf '\n== %s (%s)\n' "$1" "$(date -u +%T)"; }

step "clean checkout (git clone of HEAD $(git -C "$REPO" rev-parse --short HEAD))"
git clone --quiet --no-hardlinks "file://$REPO" "$TMP/src"
echo "files=$(git -C "$TMP/src" ls-files | wc -l)"
test -f "$TMP/src/Cargo.lock" && test -f "$TMP/src/tests/fixtures/all.json"
for p in evidence data OUTCOME.md; do test ! -e "$TMP/src/$p" || { echo "unexpected path in checkout: $p"; exit 1; }; done
BIG=$(git -C "$TMP/src" ls-files -z | xargs -0 -I{} stat -c '%s {}' "$TMP/src/{}" | sort -n | tail -1)
echo "largest tracked file: $BIG"
test "$(echo "$BIG" | cut -d' ' -f1)" -lt 2000000 || { echo "unexpectedly large tracked file (dataset copy?)"; exit 1; }

step "cargo fmt --check + cargo clippy (clean checkout, fresh target dir)"
(cd "$TMP/src" && cargo fmt --check && echo "fmt ok")
(cd "$TMP/src" && CARGO_TARGET_DIR="$TMP/test-target" cargo clippy --all-targets --locked --quiet -- -D warnings && echo "clippy ok")

step "cargo test --locked (clean checkout, fresh target dir)"
(cd "$TMP/src" && CARGO_TARGET_DIR="$TMP/test-target" cargo test --locked 2>&1 | grep -E '^test result|^error' )

step "cargo install --locked --root $TMP/root"
CARGO_TARGET_DIR="$TMP/install-target" cargo install --path "$TMP/src" --locked --root "$TMP/root" --quiet
BIN="$TMP/root/bin/yc-cashflow-mcp"
test -x "$BIN" && echo "installed: $BIN ($(stat -c %s "$BIN") bytes)"

step "stdio smoke on installed binary"
"${FIX_ENV[@]}" python3 -I "$REPO/scripts/jsonrpc_harness.py" "$BIN" \
  source_status '{}' rank_yc_prospects '{"limit":2}' rank_yc_prospects '{"limit":2,"scorer":"rank-v2"}' get_yc_company '{"slug":"fixture-docsmith"}' | grep '^## harness'

step "one-command launch: cargo run --release --locked --quiet"
rm -rf "$TMP/cache"
"${FIX_ENV[@]}" CARGO_TARGET_DIR="$TMP/install-target" python3 -I "$REPO/scripts/jsonrpc_harness.py" \
  "cargo run --release --locked --quiet --manifest-path $TMP/src/Cargo.toml" \
  search_yc_companies '{"query":"documentation"}' | grep '^## harness'

step "SMOKE PASS"
