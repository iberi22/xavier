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
## 2026-07-23 - Fix XSS bypass in panel-ui QrCodeDisplay
**Vulnerability:** The `<QrCodeDisplay>` component used `.trim()` to remove whitespaces before checking if an attribute value starts with `javascript:`. This misses embedded control characters and spaces like tabs or newlines inside the `javascript:` payload itself.
**Learning:** Standard `.trim()` fails to remove embedded tabs or newlines inside the string, allowing XSS bypasses in SVG sanitization.
**Prevention:** Attribute values parsed by `DOMParser` must be stripped of all control characters and whitespaces (using `replace(/[\u0000-\u0020]/g, '')`) before validating against malicious schemes like `javascript:`.
