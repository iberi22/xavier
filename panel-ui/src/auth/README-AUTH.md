# Auth UI - Estado Actual

## Frontend (panel-ui/src/auth/)
✅ LoginPage.tsx — Login con email/password/2FA
✅ RegisterPage.tsx — Registro
✅ TwoFactorSetup.tsx — QR + TOTP verification
✅ RecoveryPage.tsx — Seed phrase recovery
✅ BackupCodesPage.tsx — Backup codes display
✅ MasterKeyPage.tsx — Master key export/import
✅ AuthProvider.tsx — Zustand store
✅ authClient.ts — API client (fetch)
✅ QrCodeDisplay.tsx — QR SVG renderer
✅ TwoFactorInput.tsx — 6-digit code input
✅ PasswordInput.tsx — Password field
✅ SeedPhraseDisplay.tsx — 12-word seed phrase

### Connect to backend
authClient llama a:
- POST /auth/login → login_handler
- POST /auth/register → register_handler
- POST /auth/2fa/setup → (needs backend impl)
- POST /auth/2fa/verify → (needs backend impl)
- POST /auth/recovery → (needs backend impl)
- POST /auth/logout → logout_handler
- POST /auth/refresh → refresh_handler

### Routes (App.tsx)
- #/login (default) → LoginPage
- #/register → RegisterPage
- #/recovery → RecoveryPage
- #/2fa/setup → TwoFactorSetup
- #/2fa/backup → BackupCodesPage
- #/masterkey → MasterKeyPage

## Backend (src/auth2/)
✅ auth_routes() with:
- POST /auth/register
- POST /auth/login (with TOTP check)
- POST /auth/refresh
- POST /auth/logout
- GET /auth/status
❌ FALTA POST /auth/2fa/setup
❌ FALTA POST /auth/2fa/verify
❌ FALTA POST /auth/recovery (unificado)

## Onboarding (components/Onboarding/)
Current steps: Welcome → SystemScan → Hardware → Integrations
❌ FALTA: Auth (register) step
❌ FALTA: 2FA setup step
❌ FALTA: Backup codes step
❌ FALTA: Master key step

## Next
Los endpoints de 2fa/setup, 2fa/verify y recovery
deben implementarse en src/auth2/mod.rs usando
totp-rs (ya en Cargo.toml) y el AuthDb existente.
