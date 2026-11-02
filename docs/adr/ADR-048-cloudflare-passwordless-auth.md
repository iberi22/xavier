# ADR-048 — Cloudflare-Hosted Passwordless Authentication for SWAL / Xavier (swal.network)

| Campo | Valor |
|-------|--------|
| **ID** | ADR-048 |
| **Estado** | Propuesto |
| **Fecha** | 2026-11-02 |
| **Autores** | Cloudflare Infrastructure & Operational Security Architect (SWAL) |
| **Relacionados** | ADR-037 (rate limiting), ADR-043 (master key fail-closed), ADR-038 (key recovery), `docs/protocol/DECENTRALIZED_NODE_AUTH_SPEC.md`, `src/auth2/mod.rs`, `panel-ui/src/auth/README-AUTH.md` |

---

## 0. Contexto y problema

Hoy Xavier expone un backend Rust (`src/auth2`) con registro por contraseña + TOTP opcional + códigos de un solo uso (`login_code.rs`, 6 dígitos, 120 s). El `panel-ui` (React, Cloudflare Pages) consume tokens JWT + refresh tokens vía `authClient.ts` y los almacena en el cliente. El backend además soporta OAuth (Google/GitHub) y recuperación por frase semilla BIP-39.

El problema que el usuario plantea:

1. **Los tokens maestros / refresh tokens viajan por web clients y máquinas remotas**, lo que amplía la superficie de ataque (XSS, device theft, token replay).
2. **No existe una capa de autenticación passwordless unificada** hosted en Cloudflare; la auth vive en el daemon Rust local o en el cloud node (`xavier-cloud.swal.network`), y el navegador la consume directamente.
3. **No hay postura de seguridad configurable por el usuario**: el mismo flujo sirve para un PWA móvil de baja fricción y para un operador con datos sensibles.

Objetivo: **passwordless (Magic Link o OTP por email) hospedado en Cloudflare**, de modo que los clientes web y máquinas remotas **nunca** toquen tokens maestros de Xavier; además, **niveles de seguridad elegibles por el usuario**.

Restricciones heredadas del proyecto (no negociables):

- Cloudflare Pages para `panel-ui` (`panel-ui/wrangler.toml`, target `xavier.swal.network` / `xavier-panel.pages.dev`).
- Cloudflare Worker / Genesis Node en `xavier-cloud.swal.network` (ver `DECENTRALIZED_NODE_AUTH_SPEC.md`).
- 12-factor: toda configuración por env vars, nada de credenciales en código (`docs/AGENTS.md` §7).
- Secretos escaneados por `scripts/check-secrets.sh` (gitleaks) antes de merge.
- Node identity Ed25519 BIP-39-24, pro-gate por estado de nodo activo, nunca Stripe/paywalls.
- Local-first: el daemon Rust puede correr offline; la capa Cloudflare es el perímetro, no el cerebro.

---

## 1. Opción A — Cloudflare Access (Zero Trust) con OTP por email

### 1.1 Cómo funciona

Cloudflare Access se coloca delante de un origen (Pages / Worker / túnel) y actúa como IdP propio. Para un usuario sin IdP corporativo, Access soporta **One-Time PIN (OTP) por email**:

1. El usuario visita `https://xavier.swal.network` (o un subdominio protegido).
2. Access intercepta y muestra su pantalla de login.
3. El usuario introduce su email; Access envía un OTP de 6 dígitos con TTL corto (~10 min) desde `noreply@notify.cloudflare.com` (o un dominio verificado).
4. El usuario introduce el OTP; Access valida y emite una cookie de sesión firmada (`CF_Authorization`, JWT) con TTL configurable.
5. Access reenvía la petición al origen con cabeceras inyectadas: `Cf-Access-Jwt-Assertion`, `Cf-Access-Authenticated-User-Email`.

### 1.2 Pros operativos

| Dimensión | Ventaja |
|-----------|---------|
| **Cero código de auth** | No hay que escribir ni mantener lógica de OTP, envío de email, rate limiting de login, ni rotación de claves JWT para la cookie de sesión. |
| **Gestión de sesiones** | Access gestiona expiración, revocación global, y "logout everywhere" desde el dashboard. |
| **Integración IdP** | Se puede encadenar Google/GitHub/Okta/SAML sin tocar código; el OTP por email es solo el fallback. |
| **Auditoría** | Logs de acceso centralizados en Cloudflare (quién, cuándo, desde qué IP, qué aplicación). |
| **MFA nativo** | Access soporta políticas de MFA (TOTP, WebAuthn) a nivel de aplicación sin código. |
| **Aislamiento del origen** | El origen (Worker/Rust) puede exigir la cabecera `Cf-Access-Jwt-Assertion` y validarla con la clave pública de Access; nunca ve contraseñas. |
| **Coste** | Free tier cubre hasta 50 usuarios; Pro/Business para más. |
| **Protección DDoS/WAF** | El login de Access ya está detrás de la red de Cloudflare (rate limiting, bot management, Turnstile implícito en algunos flujos). |

### 1.3 Contras operativos

| Dimensión | Desventaja |
|-----------|-----------|
| **UX no nativa** | La pantalla de login es la de Cloudflare, no la del producto. Branding limitado (logo + colores en Business+). Para un PWA "seamless" rompe la experiencia. |
| **Dominio de email fijo** | El OTP viene de `notify.cloudflare.com` por defecto; para enviar desde `swal.network` hay que configurar un dominio verificado y un proveedor de email (Resend/SendGrid/SES) vía Access — añade pasos. |
| **Granularidad de identidad** | Access identifica por email, pero no conoce el modelo de "nodo" de Xavier (Ed25519 node id, karma, pro-gate). Hay que mapear email → node_id en el backend igualmente. |
| **Dependencia de Cloudflare** | Si Access cae, nadie entra. Para un sistema local-first que debe funcionar offline, Access no puede ser el único camino. |
| **Coste a escala** | Business/Enterprise para políticas avanzadas y SSO; el free tier tiene límites de usuarios y aplicaciones. |
| **Cookies de terceros** | En contextos cross-origin (PWA instalada, webview), la cookie `CF_Authorization` puede ser bloqueada por políticas de cookies de terceros; requiere configurar `SameSite`/dominios. |
| **No emite refresh tokens propios** | Access emite su cookie; el backend Rust sigue necesitando su propio JWT para autorizar llamadas a la API. Hay que hacer un "exchange" Access JWT → Xavier JWT en el backend. |
| **Rate limiting de OTP** | Cloudflare Access tiene sus propios límites de envío de OTP, pero no son configurables por aplicación ni por email con la granularidad que Xavier necesita (ej. 3/15 min por IP+email). |

### 1.4 Cuándo elegir A

- Perímetro corporativo / operadores internos.
- Cuando se quiere SSO con IdP real y MFA centralizada.
- Cuando la fricción de la pantalla de Cloudflare es aceptable (admin console, no PWA de usuario final).

---

## 2. Opción B — Custom Cloudflare Worker Magic Link / OTP

### 2.1 Cómo funciona

Se despliega un Worker (o un conjunto de Workers) en `auth.swal.network` (o rutas `/auth/*` del Pages) que implementa el flujo passwordless completo:

1. **`POST /auth/magic-link`** (o `/auth/otp/request`): recibe email (+ Turnstile token + WAF rate limit). Valida el email, genera un token/OTP de un solo uso con TTL (ej. 10 min magic link, 5 min OTP), lo hashea (SHA-256) y lo persiste en Cloudflare KV / D1 / Durable Object. Envía el email vía un proveedor (Resend / SendGrid / AWS SES / Cloudflare Email Routing) con el link `https://xavier.swal.network/auth/verify?token=...` o el código de 6 dígitos.
2. **`GET /auth/verify?token=...`** (o `POST /auth/otp/verify`): valida el token/OTP (hash compare en tiempo constante, TTL, single-use), lo marca como consumido, y emite una **sesión de corta vida** (cookie HttpOnly + Secure + SameSite=Strict, o un JWT de Xavier de vida corta) tras hacer un exchange con el backend Rust.
3. El Worker **nunca** expone tokens maestros de Xavier; solo media un exchange firmado (HMAC/JWT compartido entre Worker y backend Rust, rotado por env var).

### 2.2 Pros operativos

| Dimensión | Ventaja |
|-----------|---------|
| **UX nativa y seamless** | La pantalla de login es del producto, con branding completo, animaciones, i18n. Ideal para PWA. |
| **Control total del flujo** | TTL, formato del código (6 dígitos vs magic link vs ambos), copy del email, plantillas, idioma, deep links. |
| **Granularidad de rate limiting** | Se puede implementar rate limiting por IP+email+device fingerprint con la semántica exacta que Xavier necesita (ADR-037). |
| **Integración con el modelo de nodo** | El Worker puede consultar el backend Rust (o KV compartido) para mapear email → node_id, pro-gate, karma, etc., antes de emitir sesión. |
| **Multi-tenant / multi-instancia** | Un Worker puede servir a varios namespaces `swal/{app_id}/{instance_id}` con configuración por binding. |
| **Coste predecible** | Workers: 100k req/día gratis, luego $0.50/M. KV/D1 baratos. Email vía Resend (~$0.0001/email) o SES. |
| **Offline-friendly** | El Worker es solo el perímetro de entrada; el daemon Rust local sigue funcionando offline con su auth local (TOTP/códigos). La capa Cloudflare es opcional. |
| **Turnstile nativo** | Se puede embedir Turnstile en el formulario del producto y verificar el token en el Worker. |
| **Auditabilidad propia** | Logs estructurados a Cloudflare Logs / R2 / Xavier memory; control total del formato. |

### 2.3 Contras operativos

| Dimensión | Desventaja |
|-----------|-----------|
| **Código propio = bugs propios** | Hay que escribir, testear y mantener el flujo de magic link/OTP, hashing, TTL, single-use, email templating, y el exchange con el backend. |
| **Gestión de claves** | El Worker necesita una clave compartida con el backend Rust para firmar el exchange; rotación coordinada, almacenamiento en secrets de Wrangler. |
| **Deliverability** | Hay que configurar SPF/DKIM/DMARC en Cloudflare DNS y warm-up del dominio de envío; riesgo de caer en spam si se hace mal. |
| **Rate limiting propio** | Aunque Cloudflare WAF ofrece rate limiting, la lógica fina (por email, no solo IP) requiere estado en KV/DO y cuidado con race conditions. |
| **Seguridad del token en URL** | Magic links en query string quedan en historial del navegador, logs de proxies y referrers. Mitigación: token de un solo uso + TTL corto + POST en el verify + `Referrer-Policy: no-referrer`. |
| **Escalabilidad del email** | Un proveedor de email con límites; hay que manejar bounces, complaints, y listas de supresión. |
| **Complejidad de sesiones** | Hay que diseñar cookie vs JWT, refresh, rotación, revocación, y sincronización con el backend Rust. |

### 2.4 Cuándo elegir B

- PWA de usuario final donde la UX nativa es crítica.
- Cuando se necesita control fino del flujo (branding, i18n, deep links, multi-tenant).
- Cuando el modelo de "nodo" de Xavier debe participar en la decisión de emitir sesión.

---

## 3. Combinación: Access como perímetro externo + Worker como experiencia nativa

**Sí, se pueden combinar**, y es la opción recomendada para una arquitectura de defensa en profundidad. Hay dos patrones:

### 3.1 Patrón "Access outer + Worker inner" (perímetro + app)

```
Internet
   │
   ▼
┌──────────────────────────────────────────────┐
│ Cloudflare Edge                              │
│  • WAF rules (rate limit, geo, managed)      │
│  • Turnstile (en formularios)                │
│  • Cloudflare Access (política por app)      │  ← outer perimeter
└──────────────────┬───────────────────────────┘
                   │ CF_Authorization cookie + Cf-Access-Jwt-Assertion
                   ▼
┌──────────────────────────────────────────────┐
│ Custom Worker (auth.swal.network)            │  ← native UX + business logic
│  • Renderiza login nativo (magic link/OTP)   │
│  • Valida Turnstile token                    │
│  • Rate limit por IP+email (WAF + propio)    │
│  • Emite magic link / OTP por email          │
│  • Verify → exchange firmado con backend     │
└──────────────────┬───────────────────────────┘
                   │ signed exchange (HMAC) + node lookup
                   ▼
┌──────────────────────────────────────────────┐
│ Xavier backend (Rust) / Genesis Node         │
│  • Valida exchange, emite JWT + refresh      │
│  • Mapea email → node_id, pro-gate, karma   │
│  • Nunca expone master tokens al navegador  │
└──────────────────────────────────────────────┘
```

**Cómo se combinan en la práctica:**

- **Access protege rutas administrativas / de alto riesgo** (`/admin/*`, `/ops/*`, dashboards internos) con OTP por email o SSO corporativo. El usuario final de la PWA no ve Access; solo los operadores.
- **El Worker protege las rutas de usuario final** (`/auth/*`, `/app/*`) con la experiencia nativa. Access se configura con una política "bypass" para el dominio público o se aplica solo a subdominios admin (`admin.swal.network`).
- **Access como fallback / break-glass**: si el Worker cae, Access puede seguir sirviendo un login de emergencia para operadores (política de "service token" o OTP).
- **Validación en cadena**: el Worker valida la cabecera `Cf-Access-Jwt-Assertion` cuando la petición viene de una ruta protegida por Access, y el backend Rust valida el JWT del Worker. Doble verificación.

### 3.2 Patrón "Access as IdP, Worker as RP" (federación)

- Access se configura como un **proveedor OIDC/SAML** (no como proxy) y el Worker actúa como Relying Party: redirige a Access para autenticar, recibe el `id_token`, y luego emite su propia sesión nativa.
- Ventaja: se aprovecha el IdP de Access (OTP por email, MFA, SSO) sin perder la UX nativa del producto.
- Coste: más complejo (flujo OIDC completo en el Worker), y Access debe estar configurado como aplicación OIDC (requiere plan Business+ para custom OIDC provider).

### 3.3 Recomendación de combinación

Para SWAL / Xavier, se recomienda el **Patrón 3.1** (Access outer para admin/ops, Worker inner para usuarios finales), porque:

1. Mantiene la UX nativa de la PWA (crítica para adopción).
2. Añade una capa de perímetro real para operadores y endpoints sensibles sin código.
3. Permite que el daemon Rust local funcione offline (Access no es dependencia del camino crítico del usuario final).
4. Aprovecha Turnstile + WAF en el edge para ambos caminos.

---

## 4. Defense-in-Depth sobre Cloudflare

### 4.1 Cloudflare Turnstile (CAPTCHA invisible) en el formulario de email

**Objetivo:** prevenir email-bombing / amplification attacks (un atacante envía miles de magic links a una víctima para saturar su bandeja y el proveedor de email).

**Implementación:**

- En el formulario del Worker (o en `panel-ui`), se embede el widget Turnstile en modo **invisible** (managed/invisible). El sitio web obtiene un token `cf-turnstile-response`.
- El Worker, en `POST /auth/magic-link`, **verifica el token contra la API de Turnstile** (`https://challenges.cloudflare.com/turnstile/v0/siteverify`) con el `secret key` almacenado en Wrangler secrets.
- Si la verificación falla → 403, no se envía email.
- Configuración recomendada:
  - Modo: **Invisible** (no fricción para usuarios legítimos).
  - Dominio: `swal.network` y subdominios.
  - Acción: `magic_link_request`.
  - Threshold: default (Cloudflare ajusta automáticamente).

**Consideraciones:**

- Turnstile no es un reemplazo del rate limiting; es una capa adicional. Un atacante puede resolver Turnstile manualmente, pero no a escala.
- Para clientes no-browser (CLI, móvil nativo), Turnstile no aplica; se usa rate limiting + proof-of-work opcional o device attestation.
- El token de Turnstile expira en ~5 min; debe verificarse inmediatamente.

### 4.2 Cloudflare WAF Rate Limiting en `POST /auth/magic-link`

**Objetivo:** limitar la tasa de solicitudes de magic link para prevenir abuso y email-bombing.

**Reglas WAF (Cloudflare Rate Limiting Rules):**

| Regla | Clave | Umbral | Acción |
|-------|-------|--------|--------|
| R1 | IP de origen | 3 req / 15 min | Block (429) + challenge |
| R2 | Email (en body, extraído por WAF) | 3 req / 15 min | Block (429) |
| R3 | IP + User-Agent | 10 req / 1 min | Managed challenge |
| R4 | Global (toda la ruta) | 1000 req / 1 min | Block (503) — protección de disponibilidad |

**Implementación:**

- Cloudflare WAF Rate Limiting Rules (disponibles en Pro+) permiten rate limiting por IP y por campos del body (con "rate limiting with custom keys" / "advanced rate limiting").
- Para rate limiting por email (que está en el body JSON), se usa **Advanced Rate Limiting** con una expresión que extrae el campo `email` del body, o bien se implementa en el Worker con estado en KV/Durable Objects (más control, más código).
- Recomendación: **ambas capas**:
  - WAF (edge, barato, rápido) para IP-based y protección de disponibilidad.
  - Worker (application) para email-based y lógica de negocio (ej. "este email ya tiene un código activo, no enviar otro").

**Respuesta 429:**

```json
{
  "error": "rate_limited",
  "message": "demasiadas solicitudes; reintenta en 15 minutos",
  "retry_after": 900
}
```

Con cabecera `Retry-After: 900`.

### 4.3 Email deliverability (SPF, DKIM, DMARC en Cloudflare DNS)

**Dominio de envío recomendado:** `auth.swal.network` o `mail.swal.network` (subdominio dedicado para email transaccional, aislado del dominio principal).

**Registros DNS en Cloudflare:**

| Tipo | Nombre | Valor | TTL |
|------|--------|-------|-----|
| TXT | `swal.network` | `v=spf1 include:_spf.resend.com ~all` (o el proveedor elegido) | 3600 |
| TXT | `resend._domainkey.swal.network` | `v=DKIM1; k=rsa; p=<public_key>` (generado por el proveedor) | 3600 |
| TXT | `_dmarc.swal.network` | `v=DMARC1; p=quarantine; rua=mailto:dmarc@swal.network; ruf=mailto:dmarc@swal.network; pct=100; adkim=s; aspf=s` | 3600 |
| MX | `swal.network` | `10 route1.mx.cloudflare.net.` (si se usa Cloudflare Email Routing para inbound) | 3600 |
| TXT | `swal.network` | `google-site-verification=...` (si se usa Google Workspace) | 3600 |

**Recomendaciones operativas:**

- **SPF:** usar `~all` (softfail) inicialmente; pasar a `-all` (hardfail) tras validar que todo el email legítimo pasa.
- **DKIM:** rotar claves cada 6-12 meses; usar 2048-bit RSA mínimo.
- **DMARC:** empezar con `p=none` (monitoring) durante 2-4 semanas, luego `p=quarantine`, finalmente `p=reject` tras confirmar que no hay falsos positivos.
- **Dominio dedicado:** enviar desde `auth.swal.network` o `mail.swal.network`, no desde el dominio raíz, para aislar la reputación del email transaccional del email corporativo.
- **Warm-up:** empezar con volúmenes bajos y escalar gradualmente; usar un proveedor con buena reputación (Resend, Postmark, SES).
- **BIMI (opcional):** añadir logo verificado para mayor confianza visual en clientes que lo soportan.
- **Monitoreo:** configurar alertas de DMARC reports (rua) y monitorear la reputación del dominio en Google Postmaster Tools y Microsoft SNDS.

---

## 5. Arquitectura recomendada y niveles de seguridad elegibles por el usuario

### 5.1 Blueprint de alto nivel

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Cloudflare Edge (swal.network)                  │
│                                                                         │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐ │
│  │ Cloudflare  │  │   WAF Rules  │  │  Turnstile   │  │ Cloudflare   │ │
│  │   Access    │  │ (rate limit, │  │ (invisible)  │  │ Email Routing│ │
│  │  (admin/*)  │  │  geo, managed)│  │              │  │  (inbound)   │ │
│  └──────┬──────┘  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘ │
│         │                │                  │                  │         │
│         └────────────────┴──────────────────┴──────────────────┘         │
│                                  │                                      │
│  ┌───────────────────────────────┴────────────────────────────────────┐  │
│  │  Custom Worker: auth.swal.network (o /auth/* en Pages)           │  │
│  │  • POST /auth/magic-link  (request)                             │  │
│  │  • POST /auth/otp/verify   (verify OTP)                          │  │
│  │  • GET  /auth/verify      (magic link verify)                   │  │
│  │  • POST /auth/exchange    (Worker↔Backend signed exchange)       │  │
│  │  Bindings: KV (tokens), D1 (users/audit), Secrets (HMAC, Turnstile)│  │
│  │  Integrations: Resend/SES (email), Turnstile verify, WAF          │  │
│  └───────────────────────────────┬────────────────────────────────────┘  │
│                                  │ signed exchange (HMAC-SHA256, 60s TTL)│
└──────────────────────────────────┼──────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  Xavier Backend (Rust) — xavier-cloud.swal.network / local daemon      │
│  • Valida exchange, emite JWT (15 min) + refresh (7 días, rotativo)   │
│  • Mapea email → node_id, pro-gate, karma, security tier              │
│  • Auth DB (SQLite, local o D1-replicated)                            │
│  • Nunca expone master tokens al navegador                            │
│  • Local-first: funciona offline con auth local (TOTP/códigos)        │
└─────────────────────────────────────────────────────────────────────────┘
```

### 5.2 Flujo detallado: Magic Link

```
1. Usuario introduce email en formulario nativo (panel-ui o PWA).
2. Frontend obtiene Turnstile token (invisible).
3. POST /auth/magic-link { email, turnstile_token, security_tier }
   → Worker verifica Turnstile (siteverify).
   → WAF rate limit (IP + email).
   → Worker valida email (formato, dominio permitido).
   → Worker genera token (32 bytes CSPRNG), hashea SHA-256, persiste en KV
     con TTL = f(security_tier): tier1=15min, tier2=10min, tier3=5min.
   → Worker envía email con link https://xavier.swal.network/auth/verify?token=<raw>
     (token raw solo en el email, nunca en logs).
   → Respuesta 200 { "status": "sent" } (genérica, no revela si el email existe).
4. Usuario hace clic en el link → GET /auth/verify?token=<raw>
   → Worker hashea el token, busca en KV.
   → Si válido y no consumido: marca como consumido (single-use), elimina de KV.
   → Worker hace exchange firmado con backend Rust:
     POST https://xavier-cloud.swal.network/internal/auth/exchange
     { email, verified_at, security_tier, hmac_signature }
   → Backend valida HMAC, busca/crea usuario, mapea a node_id, emite JWT + refresh.
   → Worker setea cookie HttpOnly+Secure+SameSite=Strict con el JWT de Xavier
     (o redirige con token en fragment #, nunca en query).
   → Respuesta 302 → /app (o PWA deep link).
5. Frontend usa el JWT (en cookie o memoria) para llamadas a la API.
   → Refresh automático vía /auth/refresh (rotativo, detección de robo).
```

### 5.3 Flujo detallado: OTP por email

```
1. Usuario introduce email → POST /auth/otp/request { email, turnstile_token }
   → Mismo pipeline que magic link, pero genera código de 6 dígitos (CSPRNG,
     rejection sampling, como en login_code.rs).
   → Email con código (no link).
2. Usuario introduce código en formulario nativo → POST /auth/otp/verify { email, code }
   → Worker valida código (hash const-time, TTL, max 3 intentos → burn).
   → Exchange firmado → JWT + refresh → cookie.
```

### 5.4 Niveles de seguridad elegibles por el usuario

El usuario elige su postura de seguridad al registrarse o en settings. El nivel afecta TTL, MFA, y políticas de sesión.

| Tier | Nombre | Características | TTL magic link/OTP | MFA | Sesión | Caso de uso |
|------|--------|-----------------|---------------------|-----|--------|-------------|
| **T1** | **Convenience** (bajo) | Magic link por email, sin OTP adicional. Cookie de sesión 7 días. Refresh rotativo. | 15 min | No | 7 días | PWA móvil, usuario casual, datos no sensibles. |
| **T2** | **Balanced** (medio) | OTP por email (6 dígitos, 5 min) + magic link como fallback. Cookie 24 h. Refresh 3 días. | 10 min / 5 min | OTP email | 24 h | Usuario estándar, datos personales. |
| **T3** | **Strict** (alto) | OTP por email + TOTP (authenticator app) obligatorio. Cookie 1 h. Refresh 1 día. Re-autenticación para acciones sensibles. | 5 min | OTP email + TOTP | 1 h | Operador, datos sensibles, admin. |
| **T4** | **Paranoid** (máximo) | OTP por email + TOTP + WebAuthn (passkey). Cookie 15 min. Sin refresh (re-auth cada 15 min). IP pinning. Device binding. | 3 min | OTP + TOTP + WebAuthn | 15 min | Admin de infra, claves maestras, operaciones críticas. |

**Mecanismo de selección:**

- Al registrarse, el usuario elige el tier (default: T2 Balanced).
- En settings, puede cambiar de tier en cualquier momento; el cambio aplica a la próxima sesión.
- El tier se persiste en el backend Rust (campo `security_tier` en `User`) y se incluye en el JWT (`claims.security_tier`).
- El Worker lee el tier del usuario (vía exchange o KV cache) y aplica las políticas correspondientes.
- **Downgrade protection:** cambiar de T3/T4 a T1 requiere re-autenticación con el factor más fuerte disponible (ej. TOTP + email OTP).

**Configurabilidad adicional (por tier o global):**

| Parámetro | T1 | T2 | T3 | T4 |
|-----------|----|----|----|----|
| Magic link TTL | 15 min | 10 min | 5 min | 3 min |
| OTP TTL | — | 5 min | 3 min | 2 min |
| Max intentos OTP | — | 3 | 3 | 2 |
| Sesión (cookie) | 7 d | 24 h | 1 h | 15 min |
| Refresh token | 7 d | 3 d | 1 d | No (re-auth) |
| Rate limit (req/15 min/IP) | 5 | 3 | 2 | 1 |
| Rate limit (req/15 min/email) | 5 | 3 | 2 | 1 |
| IP pinning | No | No | Opcional | Sí |
| Device binding | No | No | No | Sí (WebAuthn) |
| Re-auth para acciones sensibles | No | No | Sí | Sí |

### 5.5 Integración con el modelo de nodo de Xavier

El passwordless layer de Cloudflare es el **perímetro de entrada**, no el cerebro. El modelo de nodo (Ed25519, karma, pro-gate) sigue siendo autoritativo:

- Al verificar un magic link/OTP, el Worker hace exchange con el backend Rust.
- El backend busca el `node_id` asociado al email (o crea uno nuevo si es registro).
- El backend emite un JWT que incluye `node_id`, `security_tier`, `role`, `karma`.
- El pro-gate (funciones pro) sigue dependiendo del estado del nodo activo, no del tier de auth.
- Para máquinas remotas (CLI, daemon), el flujo es diferente: usan `xavier users create` + códigos de un solo uso (`/auth/exchange`) o device attestation. El magic link web no reemplaza esto; lo complementa.

### 5.6 Almacenamiento de tokens en el cliente (web)

**Recomendación: cookies HttpOnly + Secure + SameSite=Strict**, no localStorage.

| Aspecto | Cookie (HttpOnly) | localStorage |
|---------|-------------------|--------------|
| XSS | Protegido (JS no puede leer) | Vulnerable |
| CSRF | Protegido con SameSite=Strict + CSRF token | No aplica (no se envía auto) |
| Mobile/PWA | Funciona con credenciales | Funciona |
| Refresh | Automático (cookie se envía sola) | Manual |

- Access token (JWT, 15 min): cookie HttpOnly, Secure, SameSite=Strict, Path=/.
- Refresh token (rotativo): cookie HttpOnly, Secure, SameSite=Strict, Path=/auth/refresh.
- Para clientes no-browser (CLI, móvil nativo): tokens en keychain/keystore del OS, nunca en archivos de texto.

### 5.7 Componentes y tecnologías

| Componente | Tecnología | Notas |
|------------|------------|-------|
| Frontend (login nativo) | React + Turnstile widget | `panel-ui`, embede Turnstile invisible. |
| Auth Worker | Cloudflare Workers (TypeScript) | Rutas `/auth/*`, bindings KV + D1 + Secrets. |
| Token store (magic links/OTP) | Cloudflare KV (TTL nativo) | Auto-expiración, single-use via delete. |
| User/audit DB | Cloudflare D1 (SQLite) o backend Rust | D1 para edge; Rust para autoritativo. |
| Email | Resend / Postmark / AWS SES | Subdominio dedicado, SPF/DKIM/DMARC. |
| CAPTCHA | Cloudflare Turnstile (invisible) | Verificación en Worker. |
| Rate limiting | Cloudflare WAF + Worker (KV/DO) | IP + email, por tier. |
| Sesión | HttpOnly cookies + JWT (Xavier) | Exchange firmado HMAC Worker↔Backend. |
| Backend | Rust (Xavier) | Valida exchange, emite JWT, mapea nodo. |
| Perímetro admin | Cloudflare Access (OTP/SSO) | Solo rutas admin/ops. |
| DNS | Cloudflare DNS | SPF, DKIM, DMARC, MX. |

### 5.8 Variables de entorno (12-factor)

Nuevas variables a añadir a `.env.example`:

```bash
# ── Passwordless auth (Cloudflare Worker) ────────────────────────────────
XAVIER_AUTH_WORKER_URL=https://auth.swal.network
XAVIER_AUTH_EXCHANGE_SECRET=<32-byte hex, rotado cada 90 días>  # HMAC compartido Worker↔Backend
XAVIER_AUTH_MAGIC_LINK_TTL_DEFAULT=600                          # segundos (default T2)
XAVIER_AUTH_OTP_TTL_DEFAULT=300
XAVIER_AUTH_MAX_OTP_ATTEMPTS=3
XAVIER_AUTH_COOKIE_DOMAIN=.swal.network
XAVIER_AUTH_COOKIE_SECURE=true

# ── Turnstile ────────────────────────────────────────────────────────────
TURNSTILE_SITE_KEY=<public>
TURNSTILE_SECRET_KEY=<secret, Wrangler secret>

# ── Email provider ───────────────────────────────────────────────────────
EMAIL_PROVIDER=resend
EMAIL_API_KEY=<secret, Wrangler secret>
EMAIL_FROM=auth@swal.network
EMAIL_REPLY_TO=support@swal.network

# ── Cloudflare Access (admin perimeter) ──────────────────────────────────
CF_ACCESS_TEAM_DOMAIN=swal.cloudflareaccess.com
CF_ACCESS_AUD=<application audience tag>
CF_ACCESS_JWT_PUBKEY_URL=https://swal.cloudflareaccess.com/cdn-cgi/access/certs

# ── Security tier defaults ───────────────────────────────────────────────
XAVIER_DEFAULT_SECURITY_TIER=T2
```

### 5.9 Consideraciones de seguridad (amenazas y mitigaciones)

| Amenaza | Mitigación |
|---------|------------|
| Email bombing / amplification | Turnstile invisible + WAF rate limit (IP + email) + Worker rate limit (KV/DO) + tier-based limits. |
| Token en URL (magic link) | Single-use, TTL corto, `Referrer-Policy: no-referrer`, `no-store` cache, POST en verify, eliminación inmediata de KV. |
| Token replay | Single-use (delete on verify), TTL, binding a email, HMAC firma del exchange con timestamp. |
| XSS robo de token | HttpOnly cookie (JS no puede leer), CSP estricta, no localStorage. |
| CSRF | SameSite=Strict + CSRF token (double-submit) para mutaciones. |
| Phishing del magic link | Dominio verificado (swal.network), DMARC reject, BIMI, email con branding claro, aviso "si no solicitaste esto, ignora". |
| Brute force OTP | Max 3 intentos → burn, rate limit por IP+email, lockout temporal. |
| Enumeración de emails | Respuesta genérica ("si el email existe, se envió un link"), timing constante, no diferenciar errores. |
| Downgrade de tier | Re-auth con factor más fuerte para bajar de T3/T4 a T1. |
| Worker compromise | Secrets en Wrangler (no en código), rotación de HMAC, audit logs, principio de mínimo privilegio en bindings. |
| Backend compromise | Master tokens nunca en web; exchange firmado con TTL 60s; JWT de corta vida; refresh rotativo con detección de robo. |
| Offline / local-first | El Worker es perímetro opcional; el daemon Rust funciona con auth local (TOTP/códigos) sin Cloudflare. |

### 5.10 Migración desde el sistema actual

1. **Fase 1 (sin breaking changes):** Añadir el Worker de passwordless en paralelo; mantener `/auth/login` (password) y `/auth/exchange` (códigos CLI). El `panel-ui` añade la opción "Iniciar con email" junto al login por contraseña.
2. **Fase 2 (adopción):** Migrar usuarios existentes: al primer login passwordless, vincular el email al `node_id` existente (no crear duplicados). Mantener TOTP existente como factor adicional en T3/T4.
3. **Fase 3 (deprecación):** Marcar `/auth/login` (password) como deprecated; eliminar tras 2 ciclos de release. Mantener `/auth/exchange` para CLI/daemon.
4. **Fase 4 (limpieza):** Eliminar código de password hashing del backend (o mantener solo para recovery seed). Actualizar `README-AUTH.md`.

### 5.11 Métricas de éxito (verificación posterior)

| Métrica | Objetivo | Fecha de revisión |
|---------|----------|-------------------|
| % de logins passwordless (vs password) | > 80% en 30 días | 2026-12-02 |
| Tasa de entrega de email (deliverability) | > 99% | 2026-11-15 |
| Spam complaint rate | < 0.1% | 2026-11-15 |
| Intentos de email bombing bloqueados por WAF | 100% | continuo |
| Tiempo medio de login passwordless | < 30 s | 2026-12-02 |
| Incidentes de token robado en web | 0 | continuo |
| Adopción de tier T3/T4 por operadores | > 50% | 2026-12-15 |

---

## 6. Decisión (propuesta)

**Opción ganadora:** Combinación del Patrón 3.1 — **Cloudflare Access como perímetro externo para rutas admin/ops** + **Custom Cloudflare Worker para la experiencia passwordless nativa de usuarios finales**, con **Cloudflare Turnstile + WAF rate limiting + email deliverability hardened** como capas de defensa en profundidad, y **4 tiers de seguridad elegibles por el usuario** (T1 Convenience → T4 Paranoid).

**Razón:** Maximiza UX nativa para la PWA (crítica para adopción) sin sacrificar seguridad operacional (perímetro real para admin, rate limiting granular, email hardened, tiers configurables). Mantiene el local-first y el modelo de nodo de Xavier como autoritativo. Evita dependencia total de Access (que rompería el offline).

**Qué invalidaría esta decisión:**

- Si Cloudflare Access lanza un modo "headless OIDC" que permita UX nativa sin Worker propio a menor coste → reevaluar (posible simplificación).
- Si el coste de Workers + email provider supera el presupuesto → considerar Access-only para todos los usuarios.
- Si la tasa de spam complaint supera 0.5% tras warm-up → revisar proveedor de email y dominio.
- Si se descubre que la cookie HttpOnly rompe el flujo de la PWA instalada (webview) → migrar a token en memoria con refresh silencioso.

---

## 7. Verificación posterior

- Implementar en staging (`staging.swal.network`) con un subdominio de email de prueba.
- Ejecutar tests E2E del flujo magic link + OTP + cada tier.
- Validar SPF/DKIM/DMARC con `mail-tester.com` y Google Postmaster Tools.
- Auditoría de seguridad (OWASP) sobre el Worker antes de producción.
- Revisión de métricas a los 30 días post-launch.

---

## Apéndice A — Comparativa resumida de opciones

| Criterio | A: Cloudflare Access OTP | B: Custom Worker Magic Link/OTP | Combinación (recomendada) |
|----------|--------------------------|----------------------------------|---------------------------|
| UX nativa | ❌ Pantalla de Cloudflare | ✅ Pantalla del producto | ✅ (usuarios) / ❌ (admin) |
| Código propio | ✅ Ninguno | ❌ Mucho | ⚠️ Medio (solo Worker) |
| Control del flujo | ❌ Limitado | ✅ Total | ✅ Total (usuarios) |
| Rate limiting granular | ❌ Limitado | ✅ Total | ✅ Total |
| Turnstile nativo | ⚠️ Implícito | ✅ Explícito | ✅ Explícito |
| Email deliverability | ⚠️ Dominio CF por defecto | ✅ Dominio propio | ✅ Dominio propio |
| Coste a escala | ⚠️ Business+ para SSO | ✅ Predecible (Workers) | ✅ Predecible |
| Offline / local-first | ❌ Dependencia total | ✅ Perímetro opcional | ✅ Perímetro opcional |
| Integración con nodo Xavier | ❌ Solo email | ✅ Email → node_id | ✅ Email → node_id |
| MFA centralizada | ✅ Nativa | ❌ Propia | ✅ Access (admin) + propia (usuarios) |
| Complejidad operativa | ✅ Baja | ❌ Alta | ⚠️ Media |

## Apéndice B — Checklist de implementación (Fase 1)

- [ ] Crear Worker `auth.swal.network` con bindings KV + D1 + Secrets.
- [ ] Configurar Turnstile (site key + secret key) en Cloudflare dashboard.
- [ ] Configurar WAF rate limiting rules (IP + email).
- [ ] Configurar registros DNS (SPF, DKIM, DMARC) para `swal.network`.
- [ ] Integrar proveedor de email (Resend/Postmark) con subdominio dedicado.
- [ ] Implementar `POST /auth/magic-link` y `GET /auth/verify`.
- [ ] Implementar `POST /auth/otp/request` y `POST /auth/otp/verify`.
- [ ] Implementar exchange firmado HMAC Worker↔Backend Rust.
- [ ] Añadir campo `security_tier` a `User` en backend Rust.
- [ ] Implementar cookies HttpOnly en el Worker.
- [ ] Añadir formulario nativo en `panel-ui` con Turnstile invisible.
- [ ] Configurar Cloudflare Access para rutas admin/ops.
- [ ] Tests E2E + auditoría de seguridad.
- [ ] Desplegar en staging, validar deliverability.
- [ ] Desplegar en producción.

---

*Fin del documento.*
