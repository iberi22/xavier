## 2026-08-26 - Insecure fallback for WebAuthn PRF
**Vulnerability:** Used `Math.random()` as a fallback mechanism for key generation when Web Crypto or CSPRNG wasn't available.
**Learning:** This approach produced deterministic and highly guessable `device_key` sequences in Node/headless mode.
**Prevention:** Key generation libraries should always fail hard and throw an error when a cryptographically secure random number generator is unavailable instead of trying to fall back to weaker pseudorandom mechanisms.

## 2026-09-04 - SQL Injection Vulnerability in hermes_importer
**Vulnerability:** A dynamically generated SQL query used unescaped table names to pull rows from SQLite. A maliciously named table could break out of the string boundary and inject arbitrary SQL commands.
**Learning:** Even internal queries iterating over schema artifacts (e.g., `sqlite_master`) must assume inputs (like table names) might be tainted. SQLite identifier injection is distinct from value injection.
**Prevention:** Always escape identifiers (tables, columns) by quoting them in double quotes and replacing `"` with `""` if parameterization is not supported for identifiers in the database driver.

## 2026-09-08 - Path Traversal Vulnerability in F12 Routes
**Vulnerability:** The F12 API handlers in `src/server/f12_routes.rs` validate path parameters like `id` and `repo` using `.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))`. This check permits strings containing `.` which allows directory traversal payloads like `..`
**Learning:** Checking character by character (whitelisting alphanumeric + `.` + `-` + `_`) isn't sufficient when the order of those characters can create dangerous strings like `..` which can be used to escape the intended directory.
**Prevention:** Always check for `..` specifically when `.` is allowed in a path parameter, or even better, use safe path resolution methods like `std::path::Path::canonicalize` and check if it starts with the expected base directory.
