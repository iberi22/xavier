# FEATURE: On-Demand Modality Embedding Selector & Dimensional Fallback

**Status:** `planned` | **Category:** `core`

## Overview
Dynamic embedding selector directing unstructured Text to Gemma 2 (768d) and Source Code to Qwen (1024d), with automated circuit breaker fallback and transparent dimension-safe routing.

## Architecture & Design
- Extension and content heuristic classifier (`select_modality`).
- Health probe circuit breaker detecting Qwen backend availability.
- Fallback degradation: When Qwen is offline, code requests degrade gracefully to 768d text embeddings with system alert logging.
- `MultimodalVecManager`: Storage isolation across modality tables (`vec_text` 768d vs `vec_code` 1024d) with automatic fallback insertion routing.

## Implementation Paths
- `src/embedding/selector.rs` (`EmbeddingSelector` and modality classifiers)
- `src/memory/multimodal_vec_store.rs` (`MultimodalVecManager` table routing and validation)

## Test References
- `tests/embedding_selector_test.rs` covering modality classification, fallback alerts, dimension mismatch rejection, and fallback storage routing.
