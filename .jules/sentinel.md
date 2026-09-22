## 2026-08-26 - Insecure fallback for WebAuthn PRF
**Vulnerability:** Used `Math.random()` as a fallback mechanism for key generation when Web Crypto or CSPRNG wasn't available.
**Learning:** This approach produced deterministic and highly guessable `device_key` sequences in Node/headless mode.
**Prevention:** Key generation libraries should always fail hard and throw an error when a cryptographically secure random number generator is unavailable instead of trying to fall back to weaker pseudorandom mechanisms.

## 2026-09-04 - SQL Injection Vulnerability in hermes_importer
**Vulnerability:** A dynamically generated SQL query used unescaped table names to pull rows from SQLite. A maliciously named table could break out of the string boundary and inject arbitrary SQL commands.
**Learning:** Even internal queries iterating over schema artifacts (e.g., `sqlite_master`) must assume inputs (like table names) might be tainted. SQLite identifier injection is distinct from value injection.
**Prevention:** Always escape identifiers (tables, columns) by quoting them in double quotes and replacing `"` with `""` if parameterization is not supported for identifiers in the database driver.
## 2026-09-05 - [Path Traversal in API Handlers]
**Vulnerability:** Directory traversal allowed via path parameters. The validation logic only ensured that characters were alphanumeric, '.', '_', or '-'.
**Learning:** Checking character sets that include a dot ('.') without explicitly rejecting consecutive dots ('..') enables path traversal payloads.
**Prevention:** Always check for '..' when '.' is an allowed character in file or directory name parameters before concatenating them to a file path.

## 2026-09-08 - Path Traversal Vulnerability in F12 Routes
**Vulnerability:** The F12 API handlers in `src/server/f12_routes.rs` validate path parameters like `id` and `repo` using `.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))`. This check permits strings containing `.` which allows directory traversal payloads like `..`
**Learning:** Checking character by character (whitelisting alphanumeric + `.` + `-` + `_`) isn't sufficient when the order of those characters can create dangerous strings like `..` which can be used to escape the intended directory.
**Prevention:** Always check for `..` specifically when `.` is allowed in a path parameter, or even better, use safe path resolution methods like `std::path::Path::canonicalize` and check if it starts with the expected base directory.
## 2026-09-11 - Fixed path traversal vulnerability in API path variables
**Vulnerability:** Multiple API routes using `Path(id)` and `Path(session_id)` allowed `..` in the `is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-\)` checks, which could lead to path traversal.
**Learning:** When validating IDs that can be mapped to files or paths, checking for valid characters (like `.`) is not enough; explicit checks for path traversal sequences like `..` must be included.
**Prevention:** Ensure all custom string validation for path identifiers explicitly reject `..` (e.g., `id.contains("..")`).
## 2026-09-14 - [Fix timing attack in auth token comparison]\n**Vulnerability:** Found a timing attack vulnerability in `src/adapters/inbound/http/state.rs` where the `check_auth` function compared the provided token with the expected token using the `==` operator.\n**Learning:** The `==` operator for strings in Rust performs a short-circuiting comparison, which can leak the length of the matching prefix and allow an attacker to guess the token character by character. This is a common pattern to look out for in authentication and authorization flows.\n**Prevention:** Used `subtle::ConstantTimeEq` to perform a constant-time comparison of the byte arrays, ensuring the comparison time is independent of the input values. Always use constant-time comparisons for secrets and tokens.
## 2026-09-15 - Fixed DoS timing attack in API authorization checks
**Vulnerability:** A previous patch applied `subtle::ConstantTimeEq` using `ct_eq` on token slices of differing lengths without manually checking the lengths first. In Rust's `subtle` crate, calling `ct_eq` on slices of differing lengths causes a panic, introducing a critical Denial of Service (DoS) vulnerability.
**Learning:** Constant-time string comparisons on arbitrary length inputs are tricky. If standard functions panic on length mismatch, attackers can crash the server by supplying tokens of incorrect lengths.
**Prevention:** Explicitly check lengths first. If lengths differ, perform a dummy constant-time calculation on the correct length token to normalize computation time without panicking.

## 2026-09-17 - Fix timing attack in auth and CSRF token comparison
**Vulnerability:** Found timing attack vulnerabilities in `src/cli/handlers/memory.rs` and `src/server/auth_routes.rs` where the `==` operator was used to compare the provided tokens (auth token and CSRF state token) with the expected tokens.
**Learning:** The `==` operator for strings performs a short-circuiting comparison, leaking timing information that an attacker can use to incrementally guess the token character by character. Although a previous fix addressed this in some files, other areas like the memory CLI handlers and OAuth callback were still vulnerable.
**Prevention:** Always use constant-time comparison mechanisms (e.g., `xavier::server::http::api::constant_time_compare`) to compare secrets, tokens, or hashes to prevent timing side channels.
