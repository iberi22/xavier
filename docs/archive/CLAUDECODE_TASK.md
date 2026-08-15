# TASK: Fix auth2 - Compilation + Missing Endpoints + TOTP real + Onboarding auth step

## Contexto

El commit `feat-local-auth/AA4: Login UI Panel Web (React) + Recovery Flow` agregó el UI completo de auth (LoginPage, RegisterPage, RecoveryPage, TwoFactorSetup, BackupCodesPage, AuthProvider, authClient.ts) pero el backend no compila y faltan endpoints.

## Archivos existentes

### Backend (Rust):
- `src/auth2/mod.rs` — auth_routes(), register, login, refresh, logout, status handlers
- `src/auth2/db.rs` — AuthDb (SQLCipher via rusqlite)
- `src/auth2/jwt.rs` — JWT token creation
- `src/auth2/password.rs` — Argon2 hashing
- `src/auth2/refresh.rs` — Refresh token rotation
- `src/auth2/middleware.rs` — Auth middleware
- `src/security/encryption_keys.rs` — Master Key Manager (USES hkdf crate)

### UI (React):
- `panel-ui/src/api/authClient.ts` — API client: login, register, setup2FA, verify2FA, recover, logout, refresh
- `panel-ui/src/auth/LoginPage.tsx` — Login form with email + password + 2FA
- `panel-ui/src/auth/RegisterPage.tsx` — Register form
- `panel-ui/src/auth/RecoveryPage.tsx` — Seed phrase recovery
- `panel-ui/src/auth/TwoFactorSetup.tsx` — TOTP QR + backup codes display
- `panel-ui/src/auth/BackupCodesPage.tsx` — Backup codes display
- `panel-ui/src/auth/MasterKeyPage.tsx` — Master key display
- `panel-ui/src/auth/AuthProvider.tsx` — Zustand auth store
- `panel-ui/src/App.tsx` — Root with hash routing for auth pages
- `panel-ui/src/components/Onboarding/OnboardingFlow.tsx` — 4-step onboarding (NO auth step)
- `panel-ui/src/hooks/useAuth.ts` — Auth hook
- `panel-ui/src/hooks/useSession.ts` — Session hook

## Problemas a resolver

### 1. FIX: Cargo.toml — falta dependency `hkdf`
El archivo `src/security/encryption_keys.rs` usa:
```rust
use hkdf::Hkdf;
```
Pero `hkdf` no está en `Cargo.toml`.

**Acción:** Agregar `hkdf = "0.13"` a [dependencies] en Cargo.toml.

### 2. FIX: Implementar endpoints faltantes en `src/auth2/mod.rs`
El `authClient.ts` llama estos endpoints pero NO existen en el backend:
- `POST /auth/2fa/setup` — Generar secret TOTP + QR code + backup codes
- `POST /auth/2fa/verify` — Verificar código TOTP + habilitar 2FA
- `POST /auth/recovery` — Verificar seed phrase + reset password + disable 2FA

**Acción:** Agregar handlers en `auth2/mod.rs`:
```rust
// POST /auth/2fa/setup
#[derive(Serialize)]
pub struct TwoFactorSetupResponse {
    pub qr_code: String,       // SVG string
    pub secret: String,        // base32 secret
    pub backup_codes: Vec<String>,
}
async fn setup_2fa_handler(/* ... */) -> Result<impl IntoResponse, StatusCode> {
    // 1. Buscar usuario (desde token JWT en request)
    // 2. Generar secret TOTP con totp-rs (gen_secret)
    // 3. Build otpauth:// URL
    // 4. Generar QR code SVG con qrcode crate
    // 5. Generar backup codes (10 códigos aleatorios) hasheados
    // 6. Guardar secret + backup_codes_hash en DB
    // 7. Devolver { qr_code, secret, backup_codes }
}

// POST /auth/2fa/verify
async fn verify_2fa_handler(/* ... */) -> Result<impl IntoResponse, StatusCode> {
    // 1. Buscar usuario por JWT
    // 2. Verificar código TOTP contra el secret almacenado
    // 3. Hacer totp_enabled = true
    // 4. Log audit
}

// POST /auth/recovery
async fn recovery_handler(/* ... */) -> Result<impl IntoResponse, StatusCode> {
    // 1. Buscar usuario por email
    // 2. Verificar seed phrase usando bip39 (comprobar hash)
    // 3. Hashear nuevo password con Argon2
    // 4. Reset password + disable 2FA
    // 5. Log audit
}
```

### 3. FIX: Registrar rutas nuevas en auth_routes()
Agregar las 3 rutas:
```rust
.route("/2fa/setup", post(setup_2fa_handler::<S>))
.route("/2fa/verify", post(verify_2fa_handler::<S>))
.route("/recovery", post(recovery_handler::<S>))
```

### 4. FIX: TOTP real en login_handler
Actualmente en `login_handler`, el código TOTP se recibe pero NO se verifica (solo hay `// Verify TOTP...` como comentario).

**Acción:** Implementar verificación TOTP real:
```rust
if user.totp_enabled {
    let code = payload.totp_code.ok_or(StatusCode::UNAUTHORIZED)?;
    let totp = TOTP::new(
        TOTPAlgorithm::SHA1,
        6,
        1,
        30,
        user.totp_secret.as_ref().unwrap().clone(),
    );
    if !totp.check(&code)? {
        return Err(StatusCode::UNAUTHORIZED);
    }
}
```

### 5. FIX: Agregar endpoint de registro que devuelva seed phrase
El `RegisterRequest` no devuelve `seed_phrase` al frontend. El `RegisterPage.tsx` espera:
```typescript
interface RegisterResponse {
  user: User;
  seed_phrase: string;
}
```

**Acción:** Modificar `register_handler` para:
- Generar seed phrase con `bip39::Mnemonic::generate_in(bip39::Language::Spanish, 24)`
- Hashear y guardar en `user.recovery_seed_hash`
- Devolver `{ user, seed_phrase }` en la respuesta

### 6. FIX: Agregar paso de auth en OnboardingFlow.tsx
Actualmente el onboarding tiene 4 pasos: Welcome → SystemScan → Hardware → Integrations. No pide al usuario registrarse ni configurar 2FA.

**Acción:** Agregar paso opcional de auth entre Hardware e Integrations:
- Si no hay token existente → mostrar RegisterPage simplificado + 2FA setup + backup codes
- Si ya hay auth → pasar directo

Pero esto puede hacerse más simple: solo agregar un paso "Auth" que:
- Muestre opción: "Configure authentication" con formulario de registro
- Genere seed phrase y muestre master key
- Si ya registrado, mostrar login

### 7. Warnings existentes (nice-to-have)
- `auth2/mod.rs:117` — variable `code` no usada (ya se resolverá con #4)
- `auth2/db.rs:1` — import `rusqlite::Result` no usado
- `auth2/db.rs:5-6` — imports `encrypt_data`, `decrypt_data`, `NonceBytes`, `NONCE_SIZE` no usados
- `auth2/middleware.rs:2` — import `body::Body` no usado
- `auth2/middleware.rs:8` — import `std::sync::Arc` no usado
- `server/mcp/tools_memory.rs:727` — variable `mut expanded` innecesaria

## Ship: Compilar back y build front

Después de los cambios:
```
cargo check
cd panel-ui && npx vite build
```

Ambos deben pasar sin errores.
