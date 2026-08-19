# Auth Contract & Architecture

This document defines the canonical authentication architecture, client-side state provider, and API contract between the React frontend (`panel-ui`) and the Rust backend (`src/auth2`).

---

## Canonical Client Provider & State

The single, unified entry point for authentication state and operations on the client is **`AuthProvider.tsx`** (exporting `useAuthStore` and the `<AuthProvider />` wrapper).

* **State Manager:** [Zustand](https://github.com/pmndrs/zustand) (`useAuthStore`).
* **Main Router Integration:** `App.tsx` wraps/consumes the authentication state using `useAuthStore()` to determine the active view and guard routes.
* **API Client:** Actions inside `useAuthStore` delegate network requests to `authClient.ts`, which uses standard fetch requests with the API URL prefix.

---

## Endpoint Contract (Prefix: `/auth`)

All authentication-related requests are routed through the backend under the `/auth` endpoint prefix (registered via `auth_routes` in the Rust codebase under `src/auth2/mod.rs`).

### Backend & Frontend Routes Matrix

| Endpoint | Method | Client Action (`authClient`) | Backend Handler (`src/auth2/mod.rs`) | Status |
| :--- | :--- | :--- | :--- | :--- |
| `/auth/register` | `POST` | `register(email, name, password)` | `register_handler` | ✅ Implemented |
| `/auth/login` | `POST` | `login(email, password, totp_code?)` | `login_handler` | ✅ Implemented (with 2FA check) |
| `/auth/refresh` | `POST` | `refresh()` | `refresh_handler` | ✅ Implemented (session renewal) |
| `/auth/logout` | `POST` | `logout()` | `logout_handler` | ✅ Implemented (token revocation) |
| `/auth/2fa/setup` | `POST` | `setup2FA()` | `setup_2fa_handler` | ✅ Implemented (active) |
| `/auth/2fa/verify` | `POST` | `verify2FA(code)` | `verify_2fa_handler` | ✅ Implemented (active) |
| `/auth/recovery` | `POST` | `recover(email, seed, password)` | `recovery_handler` | ✅ Implemented (active) |
| `/auth/status` | `GET` | *(implicit)* | `status_handler` | ✅ Implemented (JWT validation) |

---

## Client Routing & Views (`App.tsx`)

Views managed in the UI match the state provided by `useAuthStore`:

* `#/login` (Default/Fallback) $\rightarrow$ `LoginPage`
* `#/register` $\rightarrow$ `RegisterPage`
* `#/recovery` $\rightarrow$ `RecoveryPage`
* `#/2fa/setup` $\rightarrow$ `TwoFactorSetup`
* `#/2fa/backup` $\rightarrow$ `BackupCodesPage`
* `#/master-key` $\rightarrow$ `MasterKeyPage`

---

## Features Breakdown

### 1. Two-Factor Authentication (2FA)
* **Setup:** Triggered via `POST /auth/2fa/setup` to generate a secret, backup codes, and a QR code (unicode matrix returned from backend).
* **Verification:** Finalized via `POST /auth/2fa/verify` sending the 6-digit TOTP code to persist setup in the database.
* **Enforcement:** Subsequent logins detect `requires_2fa: true` from `/auth/login` response and request the code in the UI before completing sign-in.

### 2. Password Recovery (Seed Phrase)
* **Registration:** During registration, a 24-word Spanish mnemonic seed phrase is generated and shown **once** to the user. Its SHA-256 hash is stored on the backend.
* **Recovery Flow:** If the user forgets their password, they submit `/auth/recovery` with their email, seed phrase, and a new password. The backend validates the seed phrase hash, resets the password, and automatically disables 2FA for emergency recovery.
