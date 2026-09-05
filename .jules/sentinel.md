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
