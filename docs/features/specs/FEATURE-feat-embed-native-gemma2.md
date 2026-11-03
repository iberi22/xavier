# FEATURE: In-Process Native Gemma 2 ONNX Embeddings

**Status:** `planned` | **Category:** `core`

## Overview
Autonomous, in-process vector embedding generation using Google Gemma 2 (768 dimensions) executed directly via ONNX Runtime without requiring external Ollama, Python runtime, or cloud API endpoints.

## Architecture & Design
- Crate `crates/xavier-embed-ort`: High-performance embedding library wrapping ONNX Runtime with dynamic symbol loading (`load-dynamic`).
- Architecture: Mean pooling and L2 normalization over token embeddings producing normalized 768-dimensional float32 vectors.
- Zero external processes: Runs directly within Xavier worker processes without spawning sub-processes or incurring IPC overhead.

## Implementation Paths
- `crates/xavier-embed-ort/` (core ONNX runtime logic, tokenizer parser, and L2 normalization)
- `src/embedding/native.rs` (native Xavier embedder implementation bridging `Embedder` trait)

## Test References
- `crates/xavier-embed-ort/src/pooling.rs` unit tests for mean pooling and L2 normalization.
- `crates/xavier-embed-ort/src/tokenizer.rs` unit tests for BPE tokenizer parsing.
- Parity cosine similarity tests verifying vector accuracy against canonical fixture pairs ($\ge 0.9999$).
