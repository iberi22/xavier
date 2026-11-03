# ADR-049 — Multimodal In-Process Embeddings (SigLIP Vision 768d & CLAP Audio 512d)

| Field | Value |
|---|---|
| **ID** | ADR-049 |
| **Status** | Accepted (2026-11-03) |
| **Date** | 2026-11-03 |
| **Authors** | ARCHITECT (🏛️) |
| **Related** | ADR-016 (canonical data dir), ADR-019 (plugin-first), ADR-033 (hexagonal fallback), ADR-042 (native EmbeddingGemma 2 in-process) |
| **Durable path** | `docs/adr/ADR-049-multimodal-embeddings.md` |

## Context

Xavier provides persistent memory search across vector stores. In ADR-042, in-process text embedding was standardized via `EmbeddingGemma 2` (768d) using ONNX Runtime in `crates/xavier-embed-ort/`. Furthermore, `MultimodalVecManager` (`src/memory/multimodal_vec_store.rs`) defines isolated virtual vector tables for distinct modalities:
- `vec_text` (768d)
- `vec_code` (384d / 1024d)
- `vec_legal` (1536d)
- `vec_image` (768d)
- `vec_video` (768d)
- `vec_audio` (512d)

However, until now, no multimodal embedding provider existed in Xavier to compute vector representations for images (diagrams, UI screenshots, architecture maps), audio clips (voice notes, operator calls), or video segments. Agents were forced to either discard binary attachments or rely on third-party cloud APIs that violate SWAL's local-first privacy requirements.

## Decision

1. **Vision Embeddings (768d):** Adopt **SigLIP / CLIP ViT-B/16 (768d)** in ONNX format as the canonical in-process image embedder. It matches the 768-dimensional space of `vec_image`, enabling cross-modal text-to-image semantic search with zero dimension mismatch.
2. **Audio Embeddings (512d):** Adopt **CLAP (Contrastive Language-Audio Pretraining, 512d)** in ONNX format for raw waveform / spectrogram embedding into `vec_audio`.
3. **Video Frame Sampling (768d):** Embed video files via uniform temporal keyframe sampling (1 frame per 2 seconds, up to 16 frames), generating 768d embeddings per frame using the vision encoder, aggregated via L2-normalized mean pooling into `vec_video`.
4. **Hexagonal Modality Routing:** Extend `ContentModality` in `src/embedding/selector.rs` to include `Image`, `Audio`, and `Video`. `EmbeddingSelector` routes content based on MIME types and file extensions (`.png`, `.jpg`, `.webp` -> Image; `.mp3`, `.wav`, `.ogg` -> Audio; `.mp4`, `.webm` -> Video).
5. **Zero-Daemon Offline Fallback:** If native ONNX weights are not yet downloaded locally, `EmbeddingSelector` logs a structured warning to `SYSTEM_ALERTS` and safely isolates queries to avoid crashing sqlite-vec virtual tables.

## Consequences

- **Positive:**
  - Complete local-first, zero-cloud multimodal memory for images, voice memos, and videos.
  - Zero dimension mismatch in `sqlite-vec` tables (`vec_image` 768d, `vec_audio` 512d).
  - Cross-modal search: operators can search images using natural language text queries due to joint contrastive embedding spaces.
- **Negative:**
  - Additional model weights (~300MB for vision, ~150MB for audio) when downloaded via `xavier models download`.
