# yc-cashflow-mcp

A local, read-only [Model Context Protocol](https://modelcontextprotocol.io) server in Rust. It filters public Y Combinator company profiles and ranks them with deterministic, explained keyword scoring. The target areas are Python/AI engineering, developer education and writing, DevTools, B2B infrastructure/SaaS, and support tooling.

Version 0.2.0. Tested on WSL2 Ubuntu x86_64 only (see [Tested platforms](#tested-platforms)).

## Built for Technical Prospect Discovery

YC Cashflow MCP brings Y Combinator company discovery directly into Claude Code and other MCP clients.

Four Rust-powered MCP tools let AI agents search startup profiles, inspect company records, rank prospects with explainable scores, and track data freshness.

The server runs locally, uses deterministic ranking, and keeps source evidence separate from computed results. Every recommendation can be inspected, reproduced, and challenged.

Designed for developers exploring AI tooling, technical markets, and automated research workflows.

## Tools

| Tool | Args | Returns |
|---|---|---|
| `search_yc_companies` | `query?`, `industry?`, `batch?` (`"Winter 2024"` or `"W24"`; `X`=Spring), `hiring_only?`, `limit?` | Source-linked profiles plus `derived.query_match_fields`. That field shows which fields matched; it is not a score |
| `get_yc_company` | `slug` | The full profile, or `{"found":false,"result":"not_found"}` |
| `rank_yc_prospects` | `keywords?` (up to 10), `limit?`, `scorer?` (`"rank-v1"` is the default, or `"rank-v2"`) | Active companies ranked by `relevance_score`. Every point is explained with term, field and rule |
| `source_status` | none | Origin, license note, source `last_updated`, last fetch, record count, cache age, staleness, limits and scorers. It makes no network call |

Every response keeps verbatim source data (`source_fields`, `links`) separate from data computed here (`derived`) and from `unknowns`. Each response also carries `data_mode` (`live` or `fixture`), `stale`, `source_last_updated`, `fetched_at` and an `attribution` string. Errors return `isError: true` with `error.category`, which is one of `rate_limited`, `not_found`, `source_unavailable`, `schema_drift`, `invalid_arguments`, `response_too_large` or `url_not_allowed`.

### Scorers

- **rank-v1** is the default and the reproducible baseline.
  - Points per term are `term_weight` (1 or 2) × `field_weight` (tags, industries and subindustry 3; one_liner 2; long_description 1).
  - Each category is capped at 12. Companies with `isHiring=true` get +1.
- **rank-v2** is opt-in. It changes v1 in four ways:
  - Generic terms are ignored in long_description.
  - `b2b`, `enterprise` and bare `support` are removed.
  - Developer-education terms need developer context.
  - Support tooling becomes an explicit classification: `vendor`, `generic_agent`, `uses_vocabulary` or `none`, built from product-statement evidence. Only `vendor` earns support points. Suppressed matches are listed with the reason.

## Install

Requires Rust 1.88 or later (tested with 1.92.0) and `git`. The smoke and benchmark scripts also need `bash`, `curl` and `python3` (standard library only).

```bash
git clone <repository-url> yc-cashflow-mcp
cd yc-cashflow-mcp
cargo install --path . --locked          # installs ~/.cargo/bin/yc-cashflow-mcp
```

Or run from the checkout without installing:

```bash
cargo run --release --locked --quiet
```

The first build takes 1–2 minutes and can exceed an MCP client's startup timeout, so build once before registering. On WSL with the checkout on a Windows drive (`/mnt/<drive>`), set `CARGO_TARGET_DIR` to a Linux-filesystem path; builds there are much faster.

### Register with Claude Code

```bash
claude mcp add yc-cashflow -- ~/.cargo/bin/yc-cashflow-mcp
claude mcp get yc-cashflow           # expect: Status: ✔ Connected
```

Or in a project `.mcp.json`:

```json
{ "mcpServers": { "yc-cashflow": { "command": "yc-cashflow-mcp", "env": { "YC_MCP_TTL_SECS": "86400" } } } }
```

Then ask Claude Code, for example: *"Use rank_yc_prospects with scorer rank-v2, limit 5, and explain the top result's score."*

To try it with no network access, point it at the synthetic fixtures:

```bash
claude mcp add yc-cashflow-fixture -e YC_MCP_FIXTURE_DIR="$PWD/tests/fixtures" -- ~/.cargo/bin/yc-cashflow-mcp
```

### Uninstall

```bash
claude mcp remove yc-cashflow
cargo uninstall yc-cashflow-mcp
rm -rf ~/.cache/yc-cashflow-mcp            # local data cache (or $YC_MCP_CACHE_DIR)
```

## Configuration (env)

| Var | Default | Notes |
|---|---|---|
| `YC_MCP_CACHE_DIR` | `$XDG_CACHE_HOME/yc-cashflow-mcp` or `~/.cache/yc-cashflow-mcp` | Local cache only |
| `YC_MCP_TTL_SECS` | 86400 | After the TTL, the server refetches. If the refetch fails, it serves stale data with `stale:true` |
| `YC_MCP_MAX_RESULTS` | 25 (hard cap 50) | The default `limit` is 10 |
| `YC_MCP_TIMEOUT_SECS` / `YC_MCP_MAX_RETRIES` / `YC_MCP_MAX_BYTES` | 30 / 2 / 32 MiB | Retries apply only to transient failures and 5xx. A 429 is not retried |
| `YC_MCP_FIXTURE_DIR` | unset | Reads `all.json` + `meta.json` from this directory instead of the network. Results are labelled `data_mode:"fixture"` |

Fetch safety: the URL allowlist is `https://yc-oss.github.io/api/*`; HTTPS only; redirects are not followed; a named User-Agent is sent; only `companies/all.json` and `meta.json` are fetched.

## Test

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
scripts/clean_install_smoke.sh     # clean git clone -> fmt/clippy/test -> install -> MCP stdio calls -> cargo run
```

The tests and the smoke script use **synthetic** fixtures only ([`tests/fixtures/`](tests/fixtures/README.md)): ten invented companies on `*.example` domains. They need no network. Coverage:

- schema parsing and null handling; duplicate ids and slugs; schema drift
- the URL allowlist
- deterministic ranking for both scorers and the rank-v2 taxonomy rules
- query filters
- offline first run, stale cache and restart cache
- MCP `initialize`, `tools/list` and `tools/call` over stdio (all four tools)

For a manual protocol check, `scripts/jsonrpc_harness.py` speaks newline-delimited JSON-RPC to any server command:

```bash
YC_MCP_FIXTURE_DIR=tests/fixtures python3 scripts/jsonrpc_harness.py "cargo run --release --quiet" \
  source_status '{}' rank_yc_prospects '{"limit":3,"scorer":"rank-v2"}'
```

## Benchmarks

Methodology: [`benchmarks/METHODOLOGY.md`](benchmarks/METHODOLOGY.md). Recorded results: [`benchmarks/RESULTS.md`](benchmarks/RESULTS.md).

All labels are **one annotator's judgments, not ground truth**. The annotator is the AI coding agent Claude Code (claude-opus-5-5), not a human, and the same agent wrote rank-v2. Every metric is reported with its denominator.

| Metric | rank-v1 | rank-v2 |
|---|---|---|
| Eval set (n=70) P@20, strict | 16/20 | 17/20 |
| Eval set P@20, lenient | 17/20 | 19/20 |
| Eval set support-vendor precision | 9/30 | 13/14 |
| Eval set support-vendor recall | 9/14 | 13/14 |
| Full-dataset vendor recall@20 (61 labeled vendors; 47 Active) | 0/61 | 2/61 |
| Best-ranked labeled vendor, full dataset | rank 36 | rank 1 |

These numbers do not show that rank-v2 generalizes: the annotator had read the eval descriptions before writing it. A second, blind annotator is needed.

Reproduce every recorded number from the original source (downloads ~10.6 MB from GitHub, pinned to one upstream commit and sha256-verified):

```bash
scripts/reproduce_benchmarks.sh     # ends with "REPRODUCE PASS"
```

## Tested platforms

| Platform | Status |
|---|---|
| WSL2 Ubuntu on Windows, x86_64, Linux 6.18, rustc 1.92.0 | Tested: build, tests, clean-checkout smoke, benchmark reproduction, Claude Code 2.1.302 connection |
| Native Windows, macOS, other Linux distributions, ARM | Not tested. The code has no OS-specific parts, but no claim is made |

## Troubleshooting

- `source_unavailable` on the first call means there is no network and no cache. Run once online; after that the cache serves offline, marked stale after the TTL.
- `schema_drift` means yc-oss changed its format: the payload is not an array, or more than 5% of records are invalid. The previous cache stays in use.
- If the MCP handshake fails, something wrote to stdout. The server logs to stderr only.

## Data, provenance and licensing

- **Code and synthetic fixtures:** licensed under **MIT OR Apache-2.0** ([`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE)), at your option.
- **YC company data is not included and not licensed by this project.** The server reads it from yc-oss on your request and caches it on your machine. `scripts/fetch_dataset.sh` downloads the pinned benchmark snapshot to the gitignored `data/` directory.
- **Benchmark files** contain only company slugs and ids, scores, ranks, matched scorer terms and this project's own labels and notes. Third-party text (names, websites, descriptions, one-liners, tag lists, snippets) is stripped by `scripts/strip_third_party.py`.

Details: [`docs/LEGAL_AND_DATA_SOURCES.md`](docs/LEGAL_AND_DATA_SOURCES.md).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
