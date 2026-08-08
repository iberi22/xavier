## 2026-07-18 - Fix Command Injection in memory scanner
**Vulnerability:** A `Command::new("cmd")` shell command was constructed using `format!` and the `session_db_path` dynamically in `src/maturity/scanner/memory_scanner.rs`, allowing Command Injection.
**Learning:** Shell commands should not be constructed dynamically using untrusted data or dynamically generated paths. Furthermore, relying on Windows-only binaries like `findstr` via `cmd /C` within a multi-platform app breaks the fallback mechanism on macOS and Linux.
**Prevention:** Avoid shelling out for tasks that can be performed safely via standard library functions. For example, falling back to scanning files using `std::fs::read` and inspecting bytes securely with `windows().filter()` avoids injecting external commands altogether and works across all platforms.

## 2026-07-19 - Removed Hardcoded JWT Secret Fallback
**Vulnerability:** The application was falling back to a hardcoded JWT secret ("default_secret_change_me") in `src/cli/handlers/auth.rs` and `src/cli/http_setup.rs` if the `XAVIER_JWT_SECRET` environment variable was not set, allowing an attacker to forge JWT tokens if the user forgot to configure it.
**Learning:** Fallbacks for cryptographic keys or secrets should never use a static hardcoded value. If a required secret is absent, the system should fail securely (e.g., returning an error or HTTP 500) rather than failing open with a known key.
**Prevention:** Always validate that environment variables containing cryptographic material are set before use, and raise a system error when they are missing instead of supplying a fallback string.

## 2026-07-22 - Fix XSS in panel-ui QrCodeDisplay
**Vulnerability:** The `<QrCodeDisplay>` component passed raw, unsanitized SVG strings directly to `dangerouslySetInnerHTML`.
**Learning:** Using `dangerouslySetInnerHTML` with untrusted SVG data allows XSS since SVG files can embed `<script>` elements and JavaScript event handlers (`onclick`, etc.).
**Prevention:** Always sanitize SVG before using it with `dangerouslySetInnerHTML`. Without external libraries like DOMPurify, use `DOMParser` to parse the SVG, enforce the root is an `svg`, reject `parsererror`, and recursively strip out `<script>` tags and `on*` event handlers.

## 2026-07-22 - Fix XSS in panel-ui QrCodeDisplay (Follow up)
**Vulnerability:** A custom SVG sanitizer was implemented for `<QrCodeDisplay>` but it missed some important vectors like `href="javascript:alert(1)"` and `<foreignObject>` unconstrained HTML.
**Learning:** Writing a secure custom SVG sanitizer is hard and error-prone. One must also prevent `javascript:` URIs anywhere, and disallow `<foreignObject>` completely, on top of stripping `<script>` and `on*` events.
**Prevention:** In the sanitizer using `DOMParser`, recursively remove `foreignObject` nodes and check if any attribute value starts with `javascript:` to drop it.
## 2025-02-14 - [SVG Sanitization XSS Bypass via Control Characters]
**Vulnerability:** XSS bypass in SVG sanitization where malicious actors could inject javascript schemes by embedding control characters (like tabs or newlines) that bypass basic `.trim()` sanitization logic.
**Learning:** Standard `.trim()` only removes whitespace from the ends of strings. It fails to remove embedded control characters or whitespaces within attribute values, which browsers will often ignore when parsing URIs, thereby executing malicious code.
**Prevention:** Always strip all control characters and whitespaces across the entire string (e.g., `replace(/[\u0000-\u0020]/g, '')`) before validating attribute values against restricted schemes.
## 2025-02-23 - Prevent SQL Injection via `format!`
**Vulnerability:** SQL queries in `src/enterprise/persistence.rs` and `src/notifications/mod.rs` were built using string interpolation (`format!`), which can lead to SQL Injection if variables become non-constant.
**Learning:** String interpolation for SQL queries should always be avoided, even for seemingly safe values like constants or integers (`usize`), as they can be refactored later into dynamic inputs without developers noticing the interpolation risk.
**Prevention:** Always use parameterized SQL queries (e.g. `rusqlite`'s `params![]`) for variable values (like `LIMIT ?`), and use string literals directly for static queries instead of injecting constants via `format!`. Use `LIMIT -1` as a parameter when the limit is 0 to signify 'no limit' in SQLite.

## 2026-12-06 - [SQL Injection via Dynamic Table Names in count_rows]
**Vulnerability:** The internal `count_rows` helper function in `src/codebase/db.rs` was formatting a raw string argument directly into a SQL query using `format!("SELECT COUNT(*) FROM {}", table)`. While not currently exposed to user input, this creates a latent SQL injection risk if the function were ever reused in a broader context.
**Learning:** Even internal helper methods should employ strict allowlists for structural SQL components (like table names) when parameterization isn't possible, rather than relying on callers to always pass safe literal strings.
**Prevention:** Validate dynamically formatted SQL structural parameters against a predefined array/allowlist of known, safe values before executing the query.
## 2026-12-06 - [SQL Injection via Dynamic Table/Column Names in PRAGMA table_info]
**Vulnerability:** The internal `table_has_column` helper function in `src/storage/mod.rs` and `ensure_column` in `code-graph/src/db/mod.rs` were formatting raw string arguments directly into SQL queries (e.g., `PRAGMA table_info({})`, `ALTER TABLE {} ADD COLUMN {}`). This creates a latent SQL injection risk if the functions were ever reused in a broader context or populated dynamically.
**Learning:** Even internal helper methods should employ strict allowlists for structural SQL components (like table names and column names) when parameterization isn't possible (e.g., in `PRAGMA` or `ALTER TABLE` statements), rather than relying on callers to always pass safe literal strings. Returning silent fallback values (like `Ok(false)` or `Ok(())`) upon check failure can introduce severe logic bugs, especially in migration scripts where developers might not notice silently skipped operations.
**Prevention:** Validate dynamically formatted SQL structural parameters against a predefined explicit array/allowlist of known, safe values before executing the query. Always use explicit error bubbling (e.g., `anyhow::bail!` or `return Err(...)`) instead of silent suppression when an invalid identifier is passed to avoid burying schema bugs.
## 2026-12-07 - [Remove Hardcoded Cryptographic Key Fallback in Maintainer Node]
**Vulnerability:** The application was falling back to a hardcoded private key seed (`"xavier_local_maintainer_dev_secr"`) in `src/data_commons/maintainer.rs` if the `XAVIER_MAINTAINER_PRIVATE_KEY_HEX` environment variable was not set. This allows an attacker to decrypt Data Commons telemetry logs or forge valid ECIES cryptographic payloads if they know the fallback key and the operator forgot to configure it.
**Learning:** Fallbacks for cryptographic keys or secrets should never use a static hardcoded value. If a required secret is absent, the system should fail securely (e.g., returning an error, HTTP 500, or panic) rather than failing open with a known key.
**Prevention:** Always validate that environment variables containing cryptographic material are set before use, and raise a system error (e.g., using `anyhow::Result` bubbling) when they are missing instead of supplying a static fallback string.
## 2026-12-07 - [Missing Auth Middleware on Memory Sync Endpoints]
**Vulnerability:** The `/v1/memory/pull`, `/v1/memory/push`, and `/api/v1/memory/sync/...` endpoints were added directly to the main `app` router in `src/cli/server.rs` instead of being added to the `protected_routes` or merged behind the `auth_middleware` layer. This exposed the entire memory storage to unauthenticated actors, allowing them to pull, push, and overwrite arbitrary chunks of memory data.
**Learning:** Route registration ordering in axum matters. Endpoints must be explicitly placed behind layers or within protected router sub-scopes. When adding new "Sync" endpoints, they must inherit the same authentication scopes as the base system.
**Prevention:** Always verify that newly added HTTP routes (especially those handling sensitive data) are merged into `protected_routes` or have `.layer(auth_middleware)` explicitly applied.

## 2026-12-07 - [Path Traversal in v1_memories_add user_id]
**Vulnerability:** In `src/server/v1_api.rs`, the `v1_memories_add` endpoint used the `payload.user_id` directly as the `path` string parameter for `add_document_typed` and `ensure_within_storage_limit`. A malicious actor could provide a `user_id` like `../../../etc/passwd` to bypass workspace isolation or cause path traversal directory errors.
**Learning:** Fields representing namespaces or "users" often end up being used as file paths or database keys under the hood. They must always be sanitized against `..`, `/`, `\`, and null bytes, ideally enforcing a strict alphanumeric `[a-zA-Z0-9._-]` allowlist.
**Prevention:** Always sanitize dynamic user inputs that are mapped to storage paths or namespaces using explicit character retain filters (e.g., `is_ascii_alphanumeric`) before passing them to storage layer functions.
## 2026-12-07 - [Path Traversal in memory.rs add and update handlers]
**Vulnerability:** In `src/cli/handlers/memory.rs`, the `add_handler` and `update_handler` used the raw user-provided `payload.path` string. A malicious actor could provide a path like `../../../etc/passwd` to bypass workspace isolation or cause path traversal directory errors when those documents were subsequently synced or exported as files.
**Learning:** Fields representing file paths or memory IDs often end up being used as literal storage paths or exported data on disk. They must always be sanitized against `..`, `/`, `\`, and null bytes, ideally enforcing a strict alphanumeric `[a-zA-Z0-9._-]` allowlist, across all API boundaries (HTTP, CLI, GUI), not just in specific versions of the API (like `v1_api.rs`).
**Prevention:** Always sanitize dynamic user inputs that are mapped to storage paths or namespaces using explicit character retain filters (e.g., `is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'`) before passing them to storage layer functions, across all input adapters.
