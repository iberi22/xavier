# FEATURE: CodeGraph Walk Hardening (tgrep-extracted)

**Status:** `planned` | **Score:** 0% | **Last Tested:** 2026-09-08

## Overview
Harden the `code-graph` file walk with three cheap techniques extracted from `microsoft/tgrep` (MIT, v1.0.5, evaluated 2026-09-08): binary-extension rejection, 8 KiB NUL-content sniff, and a 64 MiB size cap — plus per-reason skip counters. No index format change, no new dependency, no daemon: same walk, fewer pathological reads.

## Architecture & Design
- `code-graph/src/indexer/mod.rs`: `DEFAULT_MAX_FILE_SIZE` (64 MiB), `BINARY_SNIFF_LEN` (8 KiB), `BINARY_EXTENSIONS` (tgrep list), `WalkSkipStats { skipped_binary_ext, skipped_binary_content, skipped_too_large, skipped_errors }`, `is_binary_content()`.
- `collect_files` returns `(Vec<PathBuf>, WalkSkipStats)`; order per file: excludes → binary-ext → size cap (metadata only) → NUL sniff → language filter.
- `parse_file` enforces size + sniff guards so explicit deltas (`apply_paths` / `xavier code sync --git`) that bypass the walk are covered too.
- `code-graph/src/error.rs`: new `GraphError::Skipped(String)`; `parse_and_persist` already warns + continues on per-file errors, so a skip never fails a batch.

## Implementation Paths
- `code-graph/src/indexer/mod.rs` (walk, guards, 5 new tests)
- `code-graph/src/error.rs` (`Skipped` variant)
- `CHANGELOG.md` ([Unreleased] entry)

## Test References
- `walk_skips_binary_extension_before_any_io`
- `walk_skips_nul_masquerading_as_source`
- `walk_skips_oversize_files_without_reading_them` (sparse `set_len`, no 65 MB write)
- `walk_reports_zero_skips_for_clean_tree`
- `parse_file_rejects_oversize_explicit_paths`
- Verify: `cargo test -p code-graph --lib indexer`, `cargo fmt -p code-graph -- --check`, `cargo clippy -p code-graph --all-targets -- -D warnings`

## Known Issues & Notes
- 2 pre-existing failures in `plugin::manager/types` fixture tests reproduce on clean HEAD (verified via `git stash`); unrelated to this feature, not introduced here.
- Deliberately NOT ported (kept simple): trigram index engine, server/watcher, `core.ignorecase` handling (no-op on case-sensitive Linux FS). A tgrep sidecar for lexical search stays a separate proposal.
