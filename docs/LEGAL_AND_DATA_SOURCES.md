# Legal and data sources

Reviewed 2026-10-17. **This is not legal advice.**

## 1. What this repository licenses

- **MIT OR Apache-2.0:** the source code in `src/`, `examples/`, `tests/` and `scripts/`; the synthetic fixtures in `tests/fixtures/`; the documentation; and the project's own annotations in `benchmarks/` (labels, label reasons, decisions, notes).
- **Not licensed by this project:** any third-party data, including YC company records and the yc-oss dataset. None is shipped here.
- **Copyright:** "The yc-cashflow-mcp contributors" (see `LICENSE-MIT`).

## 2. YC company data (yc-oss/api)

| Item | Finding |
|---|---|
| Source | https://github.com/yc-oss/api, served at https://yc-oss.github.io/api/ |
| Nature | Unofficial third-party mirror, built daily from the search index behind YC's public company directory (per its README). Not a YC API; not endorsed by YC |
| License | None declared. GitHub API reports `license: null` (checked 2026-10-17). No reuse grant exists, so this project does **not** redistribute the data |
| YC website terms | https://www.ycombinator.com/legal (read 2026-10-17) prohibits scraping and redistributing site content without authorization. This project never accesses ycombinator.com programmatically; it only builds links to canonical YC company pages |
| Runtime use | The server fetches `companies/all.json` and `meta.json` only when a user calls a tool, caches them locally with a TTL, and attaches an attribution string to every response |
| Benchmark snapshot | Not in the repository. `scripts/fetch_dataset.sh` downloads it from the original source, pinned to yc-oss commit `111ff1c1cb7611b0857dd03900ef50fcc515140f`, and verifies sha256 `c3a6c33e33a362e2271901e779dd1af55c2d339874082c197c435e459aedecca` (10,617,050 bytes, 6,307 records). It is stored in the gitignored `data/` directory |

## 3. What `benchmarks/` contains

- Company **slugs and numeric ids** (identifiers needed to join labels to the dataset), eval strata, scores, ranks, matched scorer vocabulary terms and support-tag names that matched.
- The project's **own** judgments: relevance and support-class labels with short reasons written by the annotator.
- **Removed** by `scripts/strip_third_party.py`: company names, websites, YC page URLs, tag lists, batch, one-liners, descriptions and all matched snippets.
- In the annotators' notes, ten reasons that quoted or closely tracked source wording were paraphrased or shortened for publication. The labels themselves are unchanged, and the recorded metrics reproduce exactly (`scripts/reproduce_benchmarks.sh`).

## 4. Fixtures

Every record in `tests/fixtures/all.json` is invented: names begin with "Fixture" (two are named "Duplicate Slug" and "Duplicate Id"), and every website is `*.example` or null (logo URLs use `example.invalid`). Only the yc-oss field *names* are mirrored, not its content.

## 5. Dependencies

`Cargo.lock` pins all crates. A `cargo metadata --locked` license check on 2026-10-17 found only permissive licenses (MIT, Apache-2.0, ISC, BSD-3-Clause, Zlib, Unicode-3.0, CDLA-Permissive-2.0, Unlicense, CC0, 0BSD, or combinations). No crate is copyleft-only. The repository distributes source code, not binaries.

## 6. Personal data

The repository contains no personal data: no names, emails or contact details of individuals, and no local paths or session identifiers. Company records come from the public directory and are not stored here.
