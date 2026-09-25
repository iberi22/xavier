# Xavier REST API - User Guide & Documentation

Welcome to the official REST API documentation for **Xavier**, the centralized semantic context engine, persistent memory integration and federated P2P mesh network.

This directory contains the specifications and guides needed to integrate with Xavier:
- [OpenAPI Specification 3.1.0 (YAML)](./openapi.yaml)
- [Postman Collection v2.1.0](./xavier.postman_collection.json)

---

## 1. Global Concepts

### Base URL
By default, the Xavier HTTP server runs at:
```http
http://localhost:8006
```
The port can be changed when starting the server using:
```bash
xavier http <port>
```

### API Versioning
Canonical Xavier routes use the `/v1/` prefix to guarantee stability and future compatibility. Example: `/v1/memories`.
Legacy routes like `/memory/add` remain supported for CLI compatibility but migrating to `/v1/` is recommended.

### Configuration Environment Variables
- `XAVIER_PORT`: HTTP port (default `8006`).
- `XAVIER_TOKEN`: Static token for basic endpoint protection.
- `XAVIER_JWT_SECRET`: Secret key for generation and validation of JWT tokens in the user session flow.
- `XAVIER_STATE_DIR`: Persistent directory where `.xavier/auth.db` and the rest of Xavier's state live.
- `XAVIER_AUTH_RATE_LIMIT`: Max requests per minute accepted by the whole `/auth/*` nest, per caller (default `20`). See [Rate Limiting](#3-rate-limiting).

See [`docs/reference/ENV_VARS.md`](../reference/ENV_VARS.md) for the full list.

---

## 2. Authentication Protocol

Xavier supports two independent authentication mechanisms depending on the use case:

### A. Server Token Basic Authentication (`X-Xavier-Token`)
Designed for direct integrations between local agents, CLI and scripts.
- **Required Header:** `X-Xavier-Token: <static_token>`
- **Behavior:** If the header does not match the `XAVIER_TOKEN` configured on the server, access is denied with `401 Unauthorized`.

### B. Full Session-based Authentication with JWT + 2FA

> **Note:** the canonical, working user-auth API is mounted at **`/auth/*`** (implemented in
> `src/auth2/`, backed by `.xavier/auth.db`, populated by `xavier users create`). Earlier
> revisions of this document (and of `openapi.yaml` / the Postman collection) described a
> parallel `/v1/auth/*` API that was never actually reachable — nothing in the codebase ever
> wrote a user into the store it read from, so every login attempt against it failed forever.
> That dead API was removed in favor of clear `308`/`410` responses pointing here — see
> [GH #2545](https://github.com/iberi22/xavier/issues/2545). If you have old client code or
> docs referencing `/v1/auth/login`, `/v1/auth/register`, `/v1/auth/refresh`,
> `/v1/auth/logout`, `/v1/auth/totp/verify` or `/v1/auth/totp/setup`, update it to the paths
> below (`/v1/auth/sessions` and `/v1/auth/session`, the root-token session endpoints, are
> unrelated and still work as documented in [`openapi.yaml`](./openapi.yaml)).

Designed for advanced user interfaces (like `panel-ui`) and end users. The full flow comprises:

1. **User Registration — `POST /auth/register`**
   - Body: `{ "email": "...", "password": "...", "name": "..." }`. Password must be at least
     12 characters (max 200); email must look like `user@domain.tld`.
   - Passwords are hashed with Argon2id. The response includes a one-time-shown 24-word Spanish
     BIP39 recovery seed phrase (`seed_phrase`) — store it now, it is never shown again and is
     required by the recovery flow below.
   - `409 Conflict` (`email_taken`) if the email is already registered.
2. **Login — `POST /auth/login`**
   - Body: `{ "email": "...", "password": "...", "totp_code": "123456" }` (`totp_code` is
     optional and only required when the account has TOTP enabled).
   - If the account has TOTP enabled and `totp_code` is missing or wrong, the server currently
     returns a plain `401 Unauthorized` (there is no distinct `mfa_required` / `mfa_invalid`
     status in the response body yet, and login does not accept a backup code as a substitute
     for `totp_code` — only `POST /auth/recovery`, below, consumes the recovery seed to reset a
     lost account). On success: `{ access_token, refresh_token, user, requires_2fa }`.
3. **Token Refresh — `POST /auth/refresh`**
   - Body: `{ "refresh_token": "..." }`. Rotates the refresh token (theft detection: reusing an
     already-rotated token revokes the whole chain and returns `403 Forbidden`). Returns a new
     `{ access_token, refresh_token }` pair.
4. **Logout — `POST /auth/logout`**
   - Body: `{ "refresh_token": "..." }`. Revokes that refresh token. Always `200 OK`, even if the
     token was already invalid/expired (idempotent).
5. **2FA Enrollment — `POST /auth/2fa/setup` (JWT-protected) + `POST /auth/2fa/verify` (JWT-protected)**
   - `2fa/setup` generates a new TOTP secret, a Unicode-rendered QR code (`qr_code`, an
     `otpauth://` URL rendered directly as text — not a PNG/SVG image) and 10 one-time backup
     codes (`backup_codes`), and persists the (not-yet-enabled) secret + hashed backup codes.
   - `2fa/verify` takes `{ "code": "123456" }`, checks it against the pending secret and, on
     success, flips the account to `totp_enabled = true`.
   - Equivalent CLI flow, no HTTP session required: `xavier users totp-enroll --email <email>`.
6. **Account Recovery — `POST /auth/recovery`**
   - Body: `{ "email": "...", "seed_phrase": "<24-word phrase>", "new_password": "..." }`.
     Verifies the seed phrase from step 1, resets the password, and disables TOTP on the account
     (so the user can log back in immediately without the lost device).
7. **Other `/auth/*` routes:** `GET /auth/check-users` (whether any user exists yet, used by
   first-run onboarding), `GET /auth/status` (JWT introspection), `GET /auth/oauth/{provider}`
   + `GET /auth/oauth/{provider}/callback` (login via OAuth) and `POST /auth/oauth/link`
   (JWT-protected, links a provider to the authenticated account).
8. **Root-token Session Management** (unrelated to the user-auth flow above; requires the
   `X-Xavier-Token` header, not a JWT):
   - `GET /v1/auth/sessions`: Lists active sessions for the caller's root-token session.
   - `DELETE /v1/auth/sessions/{id}`: Revokes an active session immediately.
   - `POST /v1/auth/session`: Creates a new root-token session.

---

## 3. Rate Limiting

To mitigate brute-force attacks and resource abuse, Xavier includes dynamic rate-limiting middleware and database audit logging.

### Brute-Force Protection on `/auth/*`
- **Protected Routes:** the entire `/auth/*` nest (register, login, refresh, logout, 2FA, recovery, OAuth start/callback) shares one limiter — it does not single out `/auth/login`.
- **Rule:** configurable via `XAVIER_AUTH_RATE_LIMIT`, default **20 requests per minute** per caller (client IP, or agent id when the request carries an authenticated agent lease), over a 60s sliding window.
- **Behavior on Exceed:** returns `429 Too Many Requests` with `{"status": "error", "message": "Demasiados intentos de autenticacion. Maximo <n> por minuto."}`.

### Rate-Limit Response Headers
When a request is processed, the server adds the following headers to help clients regulate frequency:
- `X-RateLimit-Limit`: Maximum requests allowed in time window.
- `X-RateLimit-Remaining`: Remaining available requests.
- `X-RateLimit-Reset`: Unix time in seconds for limit reset.

---

## 4. Error Structure & Security

All errors returned by the Xavier REST API follow a structured JSON standard:

```json
{
  "error": {
    "code": <numeric_code>,
    "message": "<error_description>",
    "details": "<optional_technical_info>"
  }
}
```

### Common HTTP Status Codes
- `400 Bad Request`: Malformed payload or missing required parameters.
- `401 Unauthorized`: Invalid, expired or missing access token.
- `403 Forbidden`: Insufficient privileges for operation (restrictive mesh node ACL).
- `429 Too Many Requests`: Rate limit exceeded.
- `500 Internal Server Error`: Unexpected backend error or vector DB failure.

### Prompt Injection Mitigation (Special Error Code)
Xavier includes an internal malicious prompt injection detector (`PromptInjectionDetector`).
- If a suspicious pattern, Spanish evasion, leetspeak (e.g. `1->i`, `3->e`), accent stripping or Base64 encoding is detected, the request is blocked immediately.
- **Returned Error Code:** `-32000` (`XAVIER_ERROR_SECURITY`) with HTTP `500` or `400`.

---

## 5. How To Use Provided Tools

### Using the Postman Collection
1. Open Postman and import [xavier.postman_collection.json](./xavier.postman_collection.json).
2. In collection properties, adjust variables in the **Variables** tab:
   - `baseUrl`: Default `http://localhost:8006`.
   - `token`: Your secret `XAVIER_TOKEN` (default `change-me`).
3. Run test requests like **Get System Health** to verify your server is online.

### Viewing the OpenAPI / Swagger Spec
You can load [openapi.yaml](./openapi.yaml) in [Swagger Editor](https://editor.swagger.io/) or any OpenAPI viewer integrated in your IDE to visualize interactive documentation and generate automated clients in multiple languages.
