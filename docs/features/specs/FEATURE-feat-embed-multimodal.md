# FEATURE: Multimodal In-Process Embeddings Pipeline (SigLIP 768d & CLAP 512d)

**Status:** `planned` | **Category:** `core`

## Overview
Comprehensive in-process multimodal embedding subsystem implementing SigLIP/CLIP vision encoding (768d) and CLAP audio encoding (512d) with uniform temporal frame sampling for video (768d), integrated into Xavier's isolated SQLite virtual vector storage.

## Architecture & Design
- **Vision Model (768d):** Contrastive vision transformer (SigLIP/CLIP ViT-B/16 in ONNX) computing 768-dimensional normalized vectors from raw image pixels.
- **Audio Model (512d):** CLAP audio transformer in ONNX format computing 512-dimensional normalized vectors from waveform / spectrogram inputs.
- **Video Sampling (768d):** Uniform temporal keyframe extractor yielding 768d vectors aggregated via L2-normalized mean pooling.
- **Modality-Aware Router:** `EmbeddingSelector::select_modality` parses MIME types and extensions (`png`, `jpg`, `webp`, `mp3`, `wav`, `ogg`, `mp4`, `webm`) into typed `ContentModality::{Image, Audio, Video}`.
- **Storage Isolation:** `MultimodalVecManager` isolates vectors into `vec_image` (768d), `vec_audio` (512d), and `vec_video` (768d) virtual tables, preventing dimension-mismatch panics.

## Implementation Paths
- `docs/adr/ADR-049-multimodal-embeddings.md` (Architecture specification)
- `src/embedding/selector.rs` (Modality classification and embedder selection)
- `src/memory/multimodal_vec_store.rs` (`MultimodalVecManager` virtual tables and queries)

## Test References
- `tests/suite_multimodal_embeddings.rs` covering image vector dimension parity (768d), audio vector dimension parity (512d), cross-modal cosine similarity, and isolated storage insertion.
