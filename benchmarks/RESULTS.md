# Recorded results

Dataset: pinned yc-oss snapshot (see [METHODOLOGY.md](METHODOLOGY.md)). Machine-readable results: [`results/eval_results.json`](results/eval_results.json), [`results/vendor_ranks.json`](results/vendor_ranks.json). Labels are one AI annotator's judgments, not ground truth.

## Benchmark 1: eval set (n = 70)

### Precision@20

| Scorer | Strict (relevant) | Lenient (relevant + weak) | Zero-score in top 20 |
|---|---|---|---|
| rank-v1 | 16/20 (80.0%) | 17/20 (85.0%) | 0 |
| rank-v2 | 17/20 (85.0%) | 19/20 (95.0%) | 0 |

### Support-vendor detection (positives = 14)

| Scorer | Predicted positive | TP | FP | FN | Precision | Recall |
|---|---|---|---|---|---|---|
| rank-v1 | 30 | 9 | 21 | 5 | 9/30 (30.0%) | 9/14 (64.3%) |
| rank-v2 | 14 | 13 | 1 | 1 | 13/14 (92.9%) | 13/14 (92.9%) |

rank-v2's one FP is `sigmamind-ai`, the known annotation error (see methodology); its one FN is `leena-ai`.

### By stratum (TP / label positives · FP / label negatives)

| Stratum | n | Positives | v1 TP | v1 FP | v2 TP | v2 FP |
|---|---|---|---|---|---|---|
| devtools_infra_ai | 14 | 1 | 0/1 | 0/13 | 1/1 | 0/13 |
| support_tagged | 14 | 7 | 7/7 | 5/7 | 7/7 | 1/7 |
| incident_response | 6 | 3 | 0/3 | 0/3 | 3/3 | 0/3 |
| support_vendor_untagged | 6 | 2 | 2/2 | 4/4 | 2/2 | 0/4 |
| hard_negative_support_vocab | 14 | 1 | 0/1 | 12/13 | 0/1 | 0/13 |
| incomplete_or_ambiguous | 8 | 0 | 0/0 | 0/8 | 0/0 | 0/8 |
| random_active_control | 8 | 0 | 0/0 | 0/8 | 0/0 | 0/8 |

### Regression set (original top 20; not evidence of generalization)

- 14 of the 20 remain in rank-v2's top 20.
- Companies with rank-v1 support points: 6 of 20 under v1; 0 of 20 are classed `vendor` by v2.
- Weak-labeled companies: 4 of 20 in total; 2 in rank-v2's top 20.
- Population scored (score > 0): rank-v1 3,595; rank-v2 3,113.

## Benchmark 2: full-dataset support-vendor recall (61 labeled vendors, 47 Active)

| k | rank-v1 hits | rank-v2 hits |
|---|---|---|
| 20 | 0 | 2 |
| 50 | 2 | 6 |
| 100 | 4 | 11 |
| 250 | 19 | 22 |
| 500 | 27 | 35 |
| 1000 | 38 | 38 |

- Best-ranked labeled vendor: rank-v1 `open` at rank 36 (score 30); rank-v2 `brownie` at rank 1.
- 14 of the 61 vendors are not Active and cannot be ranked by either scorer; all 47 Active vendors get a non-zero rank-v1 score.

## Interpretation

- rank-v2's largest gain is fewer support false positives on strata that were not used as design examples (hard negatives 12/13 → 0/13).
- The incident-response gain (0/3 → 3/3) follows from a deliberately added rule, on a stratum selected with incident vocabulary.
- The annotator wrote rank-v2 after reading the eval descriptions. These results are a signal, not proof of generalization; a blind second annotator is the next step.
