# YC Cashflow MCP

**A Rust MCP server for exploring 6,307 Y Combinator startups with an AI coding assistant.**

Built with Claude Code as an experiment in startup discovery, explainable ranking, and AI-assisted evaluation.

## The experiment

The server indexes YC startup profiles and exposes four MCP tools for AI-assisted discovery.

The project explores three questions:

1. Can a Rust MCP server be used successfully inside Claude Code?
2. Can a transparent ranking algorithm produce useful company shortlists?
3. How reliable is the evaluation when the same AI agent builds the ranker and labels the benchmark?

## Recorded results

- 6,307 startup profiles in the recorded dataset
- 18 passing automated tests in the tested v0.2.0 workspace
- Strict precision@20: 16/20 to 17/20
- Support-vendor recall on the 70-company evaluation set: 9/14 to 13/14
- Hard-negative false positives: 12/13 to 0/13
- Full-dataset support-vendor recall@20: 2/61 for rank-v2

**Important:** Evaluation labels were produced by the same AI agent that developed the ranking system. They were frozen before rank-v2, but have not been independently annotated or validated.

These figures describe recorded local tests, not independently reproduced performance.

## Job discovery outcome

The recorded workflow identified 54 candidate companies and 49 provider-listed jobs. None of the jobs had been independently verified against the employer or confirmed as Serbia-eligible.

## Reproducing the experiment

The source code, benchmark inputs and reproducibility artifacts are undergoing licensing and provenance review.

This initial public repository is documentation-only. It does not yet provide an installable MCP server, a redistributable dataset or independently verified benchmark results.

## Data and licensing

The experiment used an external `yc-oss` dataset snapshot at commit `111ff1c`.

Dataset contents and third-party material are not redistributed here.

Code licensing and publication rights remain under review. No software license is granted by this repository at this stage.

## Status

Public project overview published. Source release pending review.
