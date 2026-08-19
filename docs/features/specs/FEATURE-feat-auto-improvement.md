# FEATURE: Auto-Improvement Loop

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
A continuous self-optimizing engine that automatically measures, analyzes, and refines system performance. It implements a closed loop: benchmarking retrieval recall, executing gap analysis, generating candidate hyperparameter experiments, validating outcomes, and merging optimal parameters (e.g., RRF weights).

## Architecture & Design
The `AutoImprovementEngine` uses live query logs and pre-defined test suites to measure retrieval quality metrics such as Recall@K and MRR. If a regression is detected or quality drops below configured thresholds, the engine runs local experiments in an isolated context to find better settings before applying them globally.

## Implementation Paths
- `src/memory/auto_improvement/` (AutoImprovementEngine, gap analysis, and parameter tuning structures)
- `src/cli/handlers/improve.rs` (CLI command hooks for triggering analysis)

## Sub-features
- **Automated Benchmark Runner:** Measures Recall@K, precision, and latency on representative test questions.
- **Gap Analysis Engine:** Detects quality drops or regression boundaries.
- **Experiment Generator:** Formulates parameter adjustments (e.g., altering RRF weights, chunk sizes, and overlaps) capped at 3 simultaneous candidates.
- **Validation & Merging Loop:** Validates generated settings and commits optimal configurations to live memory settings.

## Test References
- `test_analyze_gaps_identifies_low_recall` verifying sensitivity to retrieval degradation.
- `test_analyze_gaps_detects_regression` checking quality safety rails.
- `test_generate_experiments_caps_at_3` asserting limit safeguards on experiment bounds.

## Known Issues & Notes
- Phase 1 core engine is fully stable. Additional hyperparameter search strategies are added iteratively as backlogs.
