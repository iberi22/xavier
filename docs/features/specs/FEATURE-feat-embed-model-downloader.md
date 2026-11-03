# FEATURE: Self-Managed Embedding Model Downloader

**Status:** `planned` | **Category:** `core`

## Overview
Autonomous model downloader with atomic extraction, SHA-256 verification, and disk capacity guardrails, exposed via CLI (`xavier models fetch`) and HTTP endpoints.

## Architecture & Design
- Streaming chunked download with backpressure via `reqwest`.
- Pre-download disk capacity checks ensuring minimum free disk threshold before fetching model weights.
- Multi-file SHA-256 integrity verification against checksum manifests (`sha256sums.txt`).
- Atomic rename to target model directory upon successful validation to prevent corrupted half-downloads.

## Implementation Paths
- `src/embedding/downloader.rs` (`ModelDownloader` engine)
- `src/cli/handlers/offline_models.rs` (CLI command handler and REST API route)

## Test References
- `tests/model_downloader_test.rs` integration tests with mock HTTP server, atomic rename, and hash integrity checks.
