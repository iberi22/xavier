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
- `XAVIER_STATE_DIR`: Persistent directory where `auth.db`, `auth_store.db` and `memory.db` are stored.

---

## 2. Authentication Protocol

Xavier supports two independent authentication mechanisms depending on the use case:

### A. Server Token Basic Authentication (`X-Xavier-Token`)
Designed for direct integrations between local agents, CLI and scripts.
- **Required Header:** `X-Xavier-Token: <static_token>`
- **Behavior:** If the header does not match the `XAVIER_TOKEN` configured on the server, access is denied with `401 Unauthorized`.

### B. Full Session-based Authentication with JWT + 2FA
Designed for advanced user interfaces (like `panel-ui`) and end users. The full flow comprises:

1. **User Registration (`POST /auth/register`)**
   - Registers email and password securely. Passwords are hashed with robust cryptographic algorithms and stored isolated in `auth.db`.
2. **Login (`POST /v1/auth/login`)**
   - Returns initial state. If the user has second factor enabled (MFA/TOTP), the response will indicate `totp_required: true` and will not yet return the final token.
3. **TOTP 2FA Verification (`POST /v1/auth/totp/verify`)**
   - The client sends the one-time code.
   - **Compatibility Note (TOTP Double-Division Bug):** The server implements a double division by 30 of the Unix timestamp (`timestamp / 30 / 30`) to validate TOTP codes. Clients must generate codes taking this into account to avoid time sync failures.
   - Returns the final JWT token on success.
4. **Recovery Seed and Backup Codes**
   - During initial setup, endpoints are exposed to view and verify the cryptographic seed (`/auth/recovery/seed/show` and `/auth/recovery/seed/verify`) or generate emergency backup codes (`/auth/recovery/backup-codes`) to recover access if the 2FA device is lost.
5. **Active Session Management**
   - `GET /v1/auth/sessions`: Lists tokens and devices with active sessions.
   - `DELETE /v1/auth/sessions/{id}`: Revokes and destroys an active session immediately.

---

## 3. Rate Limiting

To mitigate brute-force attacks and resource abuse, Xavier includes dynamic rate-limiting middleware and database audit logging.

### Brute-Force Protection on Login
- **Protected Route:** `/v1/auth/login`
- **Rule:** Maximum **5 failed attempts within a 15-minute window**.
- **Behavior on Exceed:** Server logs `login_failed` in internal audit log, temporarily blocks requesting IP and returns `429 Too Many Requests`.

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
