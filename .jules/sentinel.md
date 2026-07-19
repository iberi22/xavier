## 2026-07-18 - Fix Command Injection in memory scanner
**Vulnerability:** A `Command::new("cmd")` shell command was constructed using `format!` and the `session_db_path` dynamically in `src/maturity/scanner/memory_scanner.rs`, allowing Command Injection.
**Learning:** Shell commands should not be constructed dynamically using untrusted data or dynamically generated paths. Furthermore, relying on Windows-only binaries like `findstr` via `cmd /C` within a multi-platform app breaks the fallback mechanism on macOS and Linux.
**Prevention:** Avoid shelling out for tasks that can be performed safely via standard library functions. For example, falling back to scanning files using `std::fs::read` and inspecting bytes securely with `windows().filter()` avoids injecting external commands altogether and works across all platforms.

## 2026-07-19 - Removed Hardcoded JWT Secret Fallback
**Vulnerability:** The application was falling back to a hardcoded JWT secret ("default_secret_change_me") in `src/cli/handlers/auth.rs` and `src/cli/http_setup.rs` if the `XAVIER_JWT_SECRET` environment variable was not set, allowing an attacker to forge JWT tokens if the user forgot to configure it.
**Learning:** Fallbacks for cryptographic keys or secrets should never use a static hardcoded value. If a required secret is absent, the system should fail securely (e.g., returning an error or HTTP 500) rather than failing open with a known key.
**Prevention:** Always validate that environment variables containing cryptographic material are set before use, and raise a system error when they are missing instead of supplying a fallback string.
