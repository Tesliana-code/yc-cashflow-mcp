#!/usr/bin/env bash
# Fetch the yc-oss snapshot that the recorded benchmarks were run on, from its original
# source, pinned to one upstream git commit, and verify it byte-for-byte.
#
# Usage: scripts/fetch_dataset.sh [out-dir]      (default: data/yc-oss-111ff1c, gitignored)
#
# The dataset is NOT part of this repository and NOT covered by its license: yc-oss/api
# declares no license. This script only downloads it to your machine for local use.
# The output dir has the layout YC_MCP_FIXTURE_DIR expects (all.json + meta.json).
set -euo pipefail
COMMIT=111ff1c1cb7611b0857dd03900ef50fcc515140f
BASE="https://raw.githubusercontent.com/yc-oss/api/$COMMIT"
ALL_SHA256=c3a6c33e33a362e2271901e779dd1af55c2d339874082c197c435e459aedecca
META_SHA256=6cad1aa3d59d8a6ebaa5badd0d6c18bc0305e5f08084229725de0f977e4f15cc
REPO="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$REPO/data/yc-oss-${COMMIT:0:7}}"
mkdir -p "$OUT"

check() { echo "$2  $1" | sha256sum -c --status; }
fetch() {
  local name="$1" path="$2" sum="$3"
  if [ -f "$OUT/$name" ] && check "$OUT/$name" "$sum"; then echo "ok (cached) $name"; return; fi
  curl --fail --silent --show-error --location --proto '=https' --max-time 120 \
    -o "$OUT/$name.part" "$BASE/$path"
  check "$OUT/$name.part" "$sum" || { echo "sha256 mismatch for $name (expected $sum)" >&2; rm -f "$OUT/$name.part"; exit 1; }
  mv "$OUT/$name.part" "$OUT/$name"
  echo "ok $name"
}
fetch all.json companies/all.json "$ALL_SHA256"
fetch meta.json meta.json "$META_SHA256"
echo "yc-oss/api@$COMMIT -> $OUT"
