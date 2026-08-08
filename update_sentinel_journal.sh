#!/bin/bash
cat << 'INNER_EOF' >> .jules/sentinel.md
## 2026-12-07 - [Missing Auth Middleware on Memory Sync Endpoints]
**Vulnerability:** The `/v1/memory/pull`, `/v1/memory/push`, and `/api/v1/memory/sync/...` endpoints were added directly to the main `app` router in `src/cli/server.rs` instead of being added to the `protected_routes` or merged behind the `auth_middleware` layer. This exposed the entire memory storage to unauthenticated actors, allowing them to pull, push, and overwrite arbitrary chunks of memory data.
**Learning:** Route registration ordering in axum matters. Endpoints must be explicitly placed behind layers or within protected router sub-scopes. When adding new "Sync" endpoints, they must inherit the same authentication scopes as the base system.
**Prevention:** Always verify that newly added HTTP routes (especially those handling sensitive data) are merged into `protected_routes` or have `.layer(auth_middleware)` explicitly applied.

## 2026-12-07 - [Path Traversal in v1_memories_add user_id]
**Vulnerability:** In `src/server/v1_api.rs`, the `v1_memories_add` endpoint used the `payload.user_id` directly as the `path` string parameter for `add_document_typed` and `ensure_within_storage_limit`. A malicious actor could provide a `user_id` like `../../../etc/passwd` to bypass workspace isolation or cause path traversal directory errors.
**Learning:** Fields representing namespaces or "users" often end up being used as file paths or database keys under the hood. They must always be sanitized against `..`, `/`, `\`, and null bytes, ideally enforcing a strict alphanumeric `[a-zA-Z0-9._-]` allowlist.
**Prevention:** Always sanitize dynamic user inputs that are mapped to storage paths or namespaces using explicit character retain filters (e.g., `is_ascii_alphanumeric`) before passing them to storage layer functions.
INNER_EOF
