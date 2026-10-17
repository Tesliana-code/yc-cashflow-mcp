# Benchmark methodology

Two offline benchmarks compare the scorers `rank-v1` (default) and `rank-v2` (opt-in). Both are deterministic for a given dataset, so every recorded number is reproducible with:

```bash
scripts/reproduce_benchmarks.sh
```

## Dataset (not redistributed)

| Field | Value |
|---|---|
| Source | yc-oss/api `companies/all.json` + `meta.json` (unofficial mirror, no license) |
| Pinned upstream commit | `111ff1c1cb7611b0857dd03900ef50fcc515140f` (2026-10-16T02:20:43Z) |
| `all.json` | sha256 `c3a6c33e33a362e2271901e779dd1af55c2d339874082c197c435e459aedecca`, 10,617,050 bytes, 6,307 records |
| `meta.json` | sha256 `6cad1aa3d59d8a6ebaa5badd0d6c18bc0305e5f08084229725de0f977e4f15cc`, `last_updated` 2026-10-16T02:20:38.136Z |
| Retrieval | `scripts/fetch_dataset.sh` (raw.githubusercontent.com at the pinned commit; sha256-verified) into gitignored `data/` |

yc-oss updates daily. A live run of the server reproduces these rankings only when the live sha256 matches.

## Annotation

- **Annotator:** one AI agent, Claude Code (claude-opus-5-5), 2026-10-17. Not a human. No inter-annotator agreement was measured. Labels are **judgments, not ground truth**.
- Labels were written from source fields only (one_liner, long_description, tags, subindustry), without looking at any scorer output.
- The same agent later wrote rank-v2, after having read the eval-set descriptions.
- Known annotation error: under the written support definition, `sigmamind-ai` (eval label `generic_agent`) is contact-center software and should be `vendor`. The frozen label was not changed after results were seen.

## Benchmark 1: independent eval set (n = 70)

Inputs: [`inputs/eval_set.json`](inputs/eval_set.json), [`inputs/eval_labels.json`](inputs/eval_labels.json), [`inputs/top20_decisions.json`](inputs/top20_decisions.json).

**Selection** (`scripts/eval_select.py`, seed `cashflow-eval-2026-10-17:`): Active companies only, excluding the 20 companies of the original rank-v1 top-20 audit. Within each stratum, candidates are ordered by `sha256(seed + slug)`; a company is used once (first stratum wins). It uses source fields only, never scores.

| Stratum | n | Rule (abridged) |
|---|---|---|
| devtools_infra_ai | 14 | tag in a DevTools/infra/ML set |
| support_tagged | 14 | tag in a support set |
| incident_response | 6 | "incident" in text and an ops/dev/security tag |
| support_vendor_untagged | 6 | no support tag, vendor-like support vocabulary in text |
| hard_negative_support_vocab | 14 | support vocabulary, no support or dev tag |
| incomplete_or_ambiguous | 8 | description < 100 chars or no tags |
| random_active_control | 8 | any remaining Active company |

Label counts — relevance: relevant 23, weak 11, irrelevant 35, insufficient_info 1. Support class: vendor 14, generic_agent 6, uses_vocabulary 17, none 32, insufficient_info 1.

**Metric rules (declared before scoring):**

1. **Precision@20:** order all 70 by `score desc, slug asc` (zero scores kept), take the top 20. Strict = relevant / 20; lenient = (relevant + weak) / 20. Report zero-score count.
2. **Support-vendor detection:** positive label `support_class == vendor` (n = 14). rank-v1 predicts positive when it awards `support_tooling` points > 0; rank-v2 when its support class is `vendor`. Precision = TP / predicted positives; recall = TP / 14.
3. **Regression set:** the original top 20 (`top20_decisions.json`), reported separately. These were visible while rank-v2 was developed, so they do not show generalization.

**Known selection bias:** the support strata were chosen with curated tags and support vocabulary, which favours scorers that use the same signals. The hard-negative, incomplete and random strata measure that effect.

## Benchmark 2: full-dataset support-vendor recall

Inputs: [`inputs/support_vendor_labels.json`](inputs/support_vendor_labels.json), [`inputs/coverage_sample_slugs.json`](inputs/coverage_sample_slugs.json).

- **Candidates:** `scripts/support_candidates.py` — recall-oriented, all statuses, vocabulary broader than either scorer. 234 candidates of 6,307 records.
- **Labels:** vendor 61 (47 Active), not_vendor 125, adjacent 26, generic_agent 17, security_incident_response 3, insufficient_info 2. Definitions are in the labels file.
- **Coverage check:** a deterministic sample of 150 non-candidates was reviewed; 0/150 were vendors. By the rule of three, the 95% upper bound on the miss rate among the 6,073 non-candidates is about 2% (up to about 121 companies), so the true vendor count may exceed 61.
- **Metric:** recall@k over all 61 vendors and over the 47 Active vendors (scorers rank Active companies only).

## Reproduction check

`scripts/reproduce_benchmarks.sh` verifies, in order:

1. the downloaded snapshot matches both sha256 values;
2. `eval_select.py` regenerates `inputs/eval_set.json` byte-for-byte;
3. `support_candidates.py` regenerates exactly the 234 labeled slugs, in order;
4. `examples/evaluate.rs` and `examples/support_recall.rs` outputs, after `strip_third_party.py`, equal `results/*.json` byte-for-byte;
5. the MCP server in fixture mode on the snapshot returns the same rank-v1 top-20 slugs as the original audit.

## Publication note

The public files contain slugs, ids, scores, ranks, matched scorer terms and this project's annotations only; third-party text is stripped (see [`../docs/LEGAL_AND_DATA_SOURCES.md`](../docs/LEGAL_AND_DATA_SOURCES.md)). The eval set was frozen and evaluated once, before rank-v2 code existed, in the maintainers' development history; that history is not part of this public repository, so the ordering is a statement, not something this repository lets you verify.
