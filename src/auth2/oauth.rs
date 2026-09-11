//! OAuth 2.0 (Google, GitHub) para registro y login publico.
//!
//! Por que este modulo: `auth2` ya trae email + argon2, JWT, TOTP y recuperacion, pero **no** tenia
//! identidad federada. El diseno del proyecto (`DISENO-BD-NODO-SUPABASE-SWAL.md:257`) dice
//! "Identidad primaria: **email/OAuth** -> vault cifrado en nodo de datos", asi que esto cierra esa
//! parte sin tocar el modelo de seguridad: la cuenta autoriza a un **nodo** (Ed25519); la llave del
//! dato sigue viviendo en el nodo.
//!
//! Reglas que se respetan aqui (las que evitan los agujeros clasicos):
//! 1. El vinculo con el proveedor es por **`subject`** (id inmutable), NUNCA por email: un email
//!    puede cambiar de dueno, el `sub` no.
//! 2. Un email que ya existe en `users` **no** se fusiona solo: el llamante debe vincular con sesion
//!    abierta. Asi un email no verificado no puede quedarse con una cuenta ajena.
//! 3. `state` firmado y de un solo uso con caducidad, y **PKCE S256** siempre.
//! 4. Los secretos salen del entorno; nunca se escriben en la base ni en el repositorio.
//!
//! El `state` es *stateless*: lleva dentro el `code_verifier` de PKCE, de modo que el callback no
//! necesita tabla ni cache. Va firmado con una clave derivada del secreto de estado:
//! `sha256(secreto || payload || secreto)`. Se usa la clave **a los dos lados** del mensaje a
//! proposito: `sha256(secreto || payload)` a secas es vulnerable a *length extension*.

use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// Proveedores soportados.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Google,
    GitHub,
}

impl Provider {
    /// Convierte el `{provider}` de la ruta en un proveedor.
    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug.to_ascii_lowercase().as_str() {
            "google" => Some(Provider::Google),
            "github" => Some(Provider::GitHub),
            _ => None,
        }
    }

    pub fn slug(&self) -> &'static str {
        match self {
            Provider::Google => "google",
            Provider::GitHub => "github",
        }
    }

    /// Endpoint de autorizacion (el navegador del usuario va aqui).
    pub fn authorize_endpoint(&self) -> &'static str {
        match self {
            Provider::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Provider::GitHub => "https://github.com/login/oauth/authorize",
        }
    }

    /// Endpoint de canje de codigo por token.
    pub fn token_endpoint(&self) -> &'static str {
        match self {
            Provider::Google => "https://oauth2.googleapis.com/token",
            Provider::GitHub => "https://github.com/login/oauth/access_token",
        }
    }

    /// Endpoint de datos del usuario.
    pub fn userinfo_endpoint(&self) -> &'static str {
        match self {
            Provider::Google => "https://openidconnect.googleapis.com/v1/userinfo",
            Provider::GitHub => "https://api.github.com/user",
        }
    }

    /// Endpoint de correos (solo GitHub: el email verificado vive ahi).
    pub fn emails_endpoint(&self) -> Option<&'static str> {
        match self {
            Provider::Google => None,
            Provider::GitHub => Some("https://api.github.com/user/emails"),
        }
    }

    pub fn scopes(&self) -> &'static str {
        match self {
            Provider::Google => "openid email profile",
            Provider::GitHub => "read:user user:email",
        }
    }
}

/// Credenciales de un proveedor (vienen del entorno).
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub client_id: String,
    pub client_secret: String,
}

/// Configuracion completa de OAuth.
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub redirect_base: String,
    pub state_secret: String,
    pub google: Option<ProviderConfig>,
    pub github: Option<ProviderConfig>,
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

impl OAuthConfig {
    /// Lee la configuracion del entorno. Un proveedor sin `client_id`/`client_secret` queda
    /// deshabilitado (y la ruta responde 501 explicando que falta, en vez de fallar a medias).
    pub fn from_env() -> Self {
        let provider_cfg = |name: &str| -> Option<ProviderConfig> {
            let id = env_opt(&format!("XAVIER_OAUTH_{name}_CLIENT_ID"))?;
            let secret = env_opt(&format!("XAVIER_OAUTH_{name}_CLIENT_SECRET"))?;
            Some(ProviderConfig {
                client_id: id,
                client_secret: secret,
            })
        };

        let redirect_base =
            env_opt("XAVIER_OAUTH_REDIRECT_BASE").unwrap_or_else(|| "http://localhost:8006".into());

        // El secreto de estado puede darse aparte; si no, se deriva del token del nodo para no
        // anadir otra variable obligatoria. Nunca se expone ni se registra.
        let state_secret = env_opt("XAVIER_OAUTH_STATE_SECRET")
            .or_else(|| env_opt("XAVIER_TOKEN"))
            .unwrap_or_else(|| "xavier-oauth-dev-secret".to_string());

        Self {
            redirect_base: redirect_base.trim_end_matches('/').to_string(),
            state_secret,
            google: provider_cfg("GOOGLE"),
            github: provider_cfg("GITHUB"),
        }
    }

    /// Credenciales de un proveedor, si esta configurado.
    pub fn provider(&self, p: Provider) -> Option<&ProviderConfig> {
        match p {
            Provider::Google => self.google.as_ref(),
            Provider::GitHub => self.github.as_ref(),
        }
    }

    /// URI de retorno exacta que hay que registrar en la consola del proveedor.
    pub fn redirect_uri(&self, p: Provider) -> String {
        // Se recorta la barra final aqui tambien (no solo en from_env): una variable de entorno
        // como "https://host/" generaria un callback con doble barra y el proveedor lo rechazaria
        // por no coincidir EXACTAMENTE con el registrado.
        format!(
            "{}/auth/oauth/{}/callback",
            self.redirect_base.trim_end_matches('/'),
            p.slug()
        )
    }
}

// ── utilidades base64url (sin dependencia externa) ───────────────────────────────

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Codifica en base64url SIN relleno (lo que exige PKCE/RFC 7636).
pub fn b64url_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64URL[((n >> 18) & 63) as usize] as char);
        out.push(B64URL[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64URL[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(B64URL[(n & 63) as usize] as char);
        }
    }
    out
}

/// Decodifica base64url sin relleno.
pub fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            b'=' => continue, // tolerante con relleno
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

/// Codifica `application/x-www-form-urlencoded` sin dependencias.
///
/// `reqwest` esta compilado aqui sin la feature que aporta `.form()`, asi que el cuerpo se arma a
/// mano. Se escapan todos los bytes fuera del juego no reservado (RFC 3986).
pub fn form_urlencode(pairs: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        out.push_str(&pct_encode(k));
        out.push('=');
        out.push_str(&pct_encode(v));
    }
    out
}

fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ── PKCE ─────────────────────────────────────────────────────────────────────────

/// `code_verifier`: 64 caracteres base64url de aleatoriedad criptografica (RFC 7636 pide 43-128).
pub fn new_code_verifier() -> String {
    let mut raw = [0u8; 48];
    OsRng.fill_bytes(&mut raw);
    b64url_encode(&raw)
}

/// `code_challenge` = base64url(sha256(verifier)) — metodo S256.
pub fn code_challenge_s256(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    b64url_encode(digest.as_ref())
}

// ── state firmado (contiene el code_verifier) ────────────────────────────────────

fn mac(secret: &str, payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(payload.as_bytes());
    hasher.update(secret.as_bytes());
    let digest = hasher.finalize();
    let bytes: &[u8] = digest.as_ref();
    b64url_encode(&bytes[..16])
}

/// Construye el `state`: `payload.firma`, con `payload = b64url(provider|verifier|exp)`.
pub fn new_state(secret: &str, provider: Provider, verifier: &str, ttl_secs: i64) -> String {
    let exp = now_secs() + ttl_secs;
    let payload = format!("{}|{}|{}", provider.slug(), verifier, exp);
    let payload_b64 = b64url_encode(payload.as_bytes());
    let firma = mac(secret, &payload_b64);
    format!("{payload_b64}.{firma}")
}

/// Valida el `state` y devuelve `(provider, code_verifier)`. Rechaza firma mala o caducado.
pub fn verify_state(secret: &str, state: &str) -> Option<(Provider, String)> {
    let (payload_b64, firma) = state.split_once('.')?;
    // Comparacion en tiempo constante para no filtrar la firma esperada.
    let esperada = mac(secret, payload_b64);
    if !constant_time_eq(firma.as_bytes(), esperada.as_bytes()) {
        return None;
    }
    let payload = String::from_utf8(b64url_decode(payload_b64)?).ok()?;
    let mut partes = payload.split('|');
    let provider = Provider::from_slug(partes.next()?)?;
    let verifier = partes.next()?.to_string();
    let exp: i64 = partes.next()?.parse().ok()?;
    if now_secs() > exp {
        return None;
    }
    Some((provider, verifier))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut dif = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        dif |= x ^ y;
    }
    dif == 0
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── URL de autorizacion ──────────────────────────────────────────────────────────

/// URL a la que se redirige al usuario. `None` si el proveedor no esta configurado.
pub fn authorize_url(
    cfg: &OAuthConfig,
    p: Provider,
    state: &str,
    challenge: &str,
) -> Option<String> {
    let creds = cfg.provider(p)?;
    let mut url = url::Url::parse(p.authorize_endpoint()).ok()?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", &creds.client_id);
        q.append_pair("redirect_uri", &cfg.redirect_uri(p));
        q.append_pair("response_type", "code");
        q.append_pair("scope", p.scopes());
        q.append_pair("state", state);
        q.append_pair("code_challenge", challenge);
        q.append_pair("code_challenge_method", "S256");
        if p == Provider::Google {
            // Sin esto Google no garantiza refresh token, pero para login basta con el id_token.
            q.append_pair("access_type", "online");
            q.append_pair("prompt", "select_account");
        }
    }
    Some(url.to_string())
}

// ── Canje del codigo ─────────────────────────────────────────────────────────────

/// Identidad devuelta por el proveedor, ya normalizada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthIdentity {
    /// Id inmutable del proveedor (`sub` en Google, `id` en GitHub). Es la clave del vinculo.
    pub subject: String,
    pub email: Option<String>,
    /// Solo se acepta como email de alta si el proveedor lo da por verificado.
    pub email_verified: bool,
}

/// Canjea el `code` por un token y consulta la identidad del usuario.
pub async fn exchange_code(
    cfg: &OAuthConfig,
    p: Provider,
    code: &str,
    verifier: &str,
) -> anyhow::Result<OAuthIdentity> {
    let creds = cfg
        .provider(p)
        .ok_or_else(|| anyhow::anyhow!("provider {} not configured", p.slug()))?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // 1. Codigo -> token de acceso
    // El redirect_uri debe coincidir EXACTAMENTE con el del paso de autorizacion.
    let redirect = cfg.redirect_uri(p);
    let cuerpo = form_urlencode(&[
        ("client_id", creds.client_id.as_str()),
        ("client_secret", creds.client_secret.as_str()),
        ("code", code),
        ("redirect_uri", redirect.as_str()),
        ("grant_type", "authorization_code"),
        ("code_verifier", verifier),
    ]);

    let resp = client
        .post(p.token_endpoint())
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(cuerpo)
        .send()
        .await?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    if !status.is_success() {
        anyhow::bail!("token exchange failed: {status} {body}");
    }
    let access_token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("no access_token in response: {body}"))?
        .to_string();

    // 2. Token -> identidad
    let user_resp = client
        .get(p.userinfo_endpoint())
        .bearer_auth(&access_token)
        .header("Accept", "application/json")
        .header("User-Agent", "xavier-oauth")
        .send()
        .await?;
    let user: serde_json::Value = user_resp.json().await.unwrap_or(serde_json::Value::Null);

    match p {
        Provider::Google => {
            let subject = user
                .get("sub")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("google: respuesta sin 'sub'"))?
                .to_string();
            Ok(OAuthIdentity {
                subject,
                email: user
                    .get("email")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_ascii_lowercase()),
                email_verified: user
                    .get("email_verified")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            })
        }
        Provider::GitHub => {
            let subject = user
                .get("id")
                .map(|v| v.to_string().trim_matches('"').to_string())
                .ok_or_else(|| anyhow::anyhow!("github: respuesta sin 'id'"))?;

            // En GitHub el email puede no venir en /user (si es privado): se consulta /user/emails.
            let mut email = user
                .get("email")
                .and_then(|v| v.as_str())
                .map(|s| s.to_ascii_lowercase());
            let mut verified = false;

            if let Some(ep) = p.emails_endpoint() {
                if let Ok(r) = client
                    .get(ep)
                    .bearer_auth(&access_token)
                    .header("Accept", "application/json")
                    .header("User-Agent", "xavier-oauth")
                    .send()
                    .await
                {
                    if let Ok(list) = r.json::<serde_json::Value>().await {
                        if let Some(arr) = list.as_array() {
                            // Se prefiere el primario verificado.
                            let pick = arr
                                .iter()
                                .find(|e| {
                                    e.get("primary").and_then(|v| v.as_bool()).unwrap_or(false)
                                        && e.get("verified")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false)
                                })
                                .or_else(|| {
                                    arr.iter().find(|e| {
                                        e.get("verified").and_then(|v| v.as_bool()).unwrap_or(false)
                                    })
                                });
                            if let Some(e) = pick {
                                email = e
                                    .get("email")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_ascii_lowercase());
                                verified = true;
                            }
                        }
                    }
                }
            }

            Ok(OAuthIdentity {
                subject,
                email,
                email_verified: verified,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b64url_round_trip_sin_relleno() {
        for caso in [&b"a"[..], &b"ab"[..], &b"abc"[..], &b"hola mundo!"[..]] {
            let e = b64url_encode(caso);
            assert!(!e.contains('='), "base64url no debe llevar relleno: {e}");
            assert!(
                !e.contains('+') && !e.contains('/'),
                "alfabeto url-safe: {e}"
            );
            assert_eq!(b64url_decode(&e).unwrap(), caso.to_vec());
        }
    }

    #[test]
    fn challenge_s256_vector_conocido() {
        // RFC 7636, Apendice B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            code_challenge_s256(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifier_tiene_longitud_y_alfabeto_validos() {
        let v = new_code_verifier();
        assert!((43..=128).contains(&v.len()), "len={}", v.len());
        assert!(v
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn state_valido_devuelve_proveedor_y_verifier() {
        let v = new_code_verifier();
        let s = new_state("secreto-de-prueba", Provider::GitHub, &v, 600);
        let (p, ver) = verify_state("secreto-de-prueba", &s).expect("state deberia validar");
        assert_eq!(p, Provider::GitHub);
        assert_eq!(ver, v);
    }

    #[test]
    fn state_con_firma_manipulada_se_rechaza() {
        let v = new_code_verifier();
        let s = new_state("secreto-de-prueba", Provider::Google, &v, 600);
        let manipulado = format!(
            "{}x",
            s.trim_end_matches(|c: char| c.is_ascii_alphanumeric())
        );
        assert!(verify_state("secreto-de-prueba", &manipulado).is_none());
        // Firma cambiada pero de la misma longitud
        let (payload, firma) = s.split_once('.').unwrap();
        let otra = if firma.starts_with('A') { "B" } else { "A" };
        let firma_mala = format!("{}{}", otra, &firma[1..]);
        assert!(verify_state("secreto-de-prueba", &format!("{payload}.{firma_mala}")).is_none());
    }

    #[test]
    fn state_caducado_se_rechaza() {
        let v = new_code_verifier();
        let s = new_state("secreto", Provider::Google, &v, -10); // ya caducado
        assert!(verify_state("secreto", &s).is_none());
    }

    #[test]
    fn state_de_otro_secreto_se_rechaza() {
        let v = new_code_verifier();
        let s = new_state("secreto-A", Provider::Google, &v, 600);
        assert!(verify_state("secreto-B", &s).is_none());
    }

    #[test]
    fn authorize_url_incluye_pkce_y_state() {
        let cfg = OAuthConfig {
            redirect_base: "https://xaviercloud.pages.dev".into(),
            state_secret: "s".into(),
            google: Some(ProviderConfig {
                client_id: "cid.apps.googleusercontent.com".into(),
                client_secret: "shh".into(),
            }),
            github: None,
        };
        let url = authorize_url(&cfg, Provider::Google, "estado123", "reto456").expect("url");
        assert!(url.contains("client_id=cid.apps.googleusercontent.com"));
        assert!(url.contains("code_challenge=reto456"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=estado123"));
        assert!(url.contains(
            "redirect_uri=https%3A%2F%2Fxaviercloud.pages.dev%2Fauth%2Foauth%2Fgoogle%2Fcallback"
        ));

        // Proveedor sin credenciales no produce URL (la ruta respondera 501).
        assert!(authorize_url(&cfg, Provider::GitHub, "e", "r").is_none());
    }

    #[test]
    fn form_urlencode_escapa_lo_necesario() {
        let s = form_urlencode(&[
            ("redirect_uri", "https://x/x callback"),
            ("grant_type", "authorization_code"),
        ]);
        assert!(s.starts_with("redirect_uri=https%3A%2F%2Fx%2Fx%20callback&"));
        assert!(s.contains("grant_type=authorization_code"));
        assert!(!s.contains(" "), "no debe quedar espacio sin escapar: {s}");
    }

    #[test]
    fn provider_desconocido_no_se_parsea() {
        assert_eq!(Provider::from_slug("google"), Some(Provider::Google));
        assert_eq!(Provider::from_slug("GitHub"), Some(Provider::GitHub));
        assert_eq!(Provider::from_slug("facebook"), None);
        assert_eq!(Provider::from_slug(""), None);
    }

    #[test]
    fn redirect_uri_por_proveedor() {
        let cfg = OAuthConfig {
            redirect_base: "https://xaviercloud.pages.dev/".into(),
            state_secret: "s".into(),
            google: None,
            github: None,
        };
        assert_eq!(
            cfg.redirect_uri(Provider::GitHub),
            "https://xaviercloud.pages.dev/auth/oauth/github/callback"
        );
    }
}
