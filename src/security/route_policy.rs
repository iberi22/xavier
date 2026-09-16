//! Política de nivel exigido por ruta (F3.2).
//!
//! La exigencia de nivel es **configuración del servidor**, no del cliente. Hasta
//! ahora la única forma de exigir nivel era el header `X-Required-Clearance`, que
//! el propio solicitante controla: eso no protege de nada (en el mejor caso, un
//! cliente se autobloquea; en el peor, cree que hay un control que no existe).
//!
//! Fuente, en orden de prioridad:
//!
//! 1. `XAVIER_REQUIRED_CLEARANCE_ROUTES` — JSON inline (tests y operación puntual).
//! 2. `data/security/route_clearance.json` — archivo, recargado por mtime.
//! 3. Reglas por defecto del binario (abajo).
//!
//! Formato:
//! ```json
//! { "routes": [ { "prefix": "/memory/export", "required": "INTERNAL" } ] }
//! ```
//! Semántica: gana el prefijo más largo; un nivel ilegible en config ⇒ `TopSecret`
//! (un error de configuración **cierra**, nunca abre).

use crate::security::clearance::{parse_required_level, ClearanceLevel};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Ruta del archivo de política (relativa al workspace).
pub const ROUTE_POLICY_PATH: &str = "data/security/route_clearance.json";

/// Override por entorno con el mismo JSON.
pub const ROUTE_POLICY_ENV: &str = "XAVIER_REQUIRED_CLEARANCE_ROUTES";

/// Una regla de política: qué nivel exige un prefijo de ruta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRule {
    pub prefix: String,
    pub required: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

/// Conjunto de reglas activas.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutePolicy {
    #[serde(default)]
    pub routes: Vec<RouteRule>,
}

impl RoutePolicy {
    /// Parsea una política desde JSON; `None` si el JSON es inválido.
    pub fn from_json(raw: &str) -> Option<Self> {
        serde_json::from_str(raw).ok()
    }

    /// Reglas por defecto del binario.
    ///
    /// Cubren el hueco más caro: los endpoints de exportación devuelven memoria en
    /// bloque **sin** aplicar el nivel de cada entrada, así que se exige identidad
    /// (cualquier rol autenticado) para usarlos. El filtrado por nivel dentro del
    /// export queda como deuda declarada (P1), no se pretende que esto lo cierre.
    pub fn default_rules() -> Self {
        Self {
            routes: vec![
                RouteRule {
                    prefix: "/memory/export".to_string(),
                    required: "INTERNAL".to_string(),
                    enabled: true,
                },
                RouteRule {
                    prefix: "/v1/memory/export-markdown".to_string(),
                    required: "INTERNAL".to_string(),
                    enabled: true,
                },
            ],
        }
    }

    /// Nivel exigido para una ruta; gana el prefijo más largo habilitado.
    pub fn required_for(&self, path: &str) -> Option<ClearanceLevel> {
        self.routes
            .iter()
            .filter(|rule| rule.enabled && path.starts_with(rule.prefix.as_str()))
            .max_by_key(|rule| rule.prefix.len())
            .map(|rule| parse_required_level(&rule.required))
    }
}

fn load_from_file(path: &Path) -> Option<RoutePolicy> {
    let raw = std::fs::read_to_string(path).ok()?;
    RoutePolicy::from_json(&raw)
}

/// Política activa: env > archivo > defaults (con caché por mtime del archivo).
pub fn active_policy() -> RoutePolicy {
    if let Ok(raw) = std::env::var(ROUTE_POLICY_ENV) {
        if let Some(policy) = RoutePolicy::from_json(&raw) {
            return policy;
        }
        tracing::error!(
            "{} no es JSON válido: se cierra con la política por defecto",
            ROUTE_POLICY_ENV
        );
    }

    type Cache = Option<(PathBuf, u64, u64, RoutePolicy)>;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

    let path = PathBuf::from(ROUTE_POLICY_PATH);
    let (stamp, len) = match std::fs::metadata(&path) {
        Ok(meta) => (
            meta.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
            meta.len(),
        ),
        // Sin archivo ⇒ defaults del binario.
        Err(_) => return RoutePolicy::default_rules(),
    };

    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cache.lock() {
        if let Some((cached_path, cached_stamp, cached_len, policy)) = guard.as_ref() {
            if cached_path == &path && *cached_stamp == stamp && *cached_len == len {
                return policy.clone();
            }
        }
        if let Some(policy) = load_from_file(&path) {
            *guard = Some((path.clone(), stamp, len, policy.clone()));
            return policy;
        }
    }

    RoutePolicy::default_rules()
}

/// Nivel exigido para la ruta dada; `None` si no hay exigencia configurada.
pub fn required_for_path(path: &str) -> Option<ClearanceLevel> {
    active_policy().required_for(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_longest_prefix_wins() {
        let policy = RoutePolicy::from_json(
            r#"{"routes":[
                {"prefix":"/v1","required":"INTERNAL"},
                {"prefix":"/v1/documents","required":"SECRET"}
            ]}"#,
        )
        .expect("json válido");

        assert_eq!(
            policy.required_for("/v1/documents/abc"),
            Some(ClearanceLevel::Secret)
        );
        assert_eq!(
            policy.required_for("/v1/memories"),
            Some(ClearanceLevel::Internal)
        );
        assert_eq!(policy.required_for("/health"), None);
    }

    #[test]
    fn test_unknown_level_in_config_closes() {
        // Un typo en la config no puede degradar la exigencia a público.
        let policy =
            RoutePolicy::from_json(r#"{"routes":[{"prefix":"/x","required":"TPOSECRET"}]}"#)
                .expect("json válido");
        assert_eq!(policy.required_for("/x/y"), Some(ClearanceLevel::TopSecret));
    }

    #[test]
    fn test_disabled_rule_is_ignored() {
        let policy = RoutePolicy::from_json(
            r#"{"routes":[{"prefix":"/x","required":"TOPSECRET","enabled":false}]}"#,
        )
        .expect("json válido");
        assert_eq!(policy.required_for("/x/y"), None);
    }

    #[test]
    fn test_env_override_is_authoritative() {
        let policy =
            RoutePolicy::from_json(r#"{"routes":[{"prefix":"/e2e","required":"SECRET"}]}"#)
                .expect("json válido");
        assert_eq!(
            policy.required_for("/e2e/read"),
            Some(ClearanceLevel::Secret)
        );
        // El prefijo del env no exige nada fuera de su rama.
        assert_eq!(policy.required_for("/otra"), None);
    }

    #[test]
    fn test_default_rules_cover_exports() {
        let policy = RoutePolicy::default_rules();
        assert_eq!(
            policy.required_for("/memory/export"),
            Some(ClearanceLevel::Internal)
        );
        assert_eq!(
            policy.required_for("/memory/export-markdown"),
            Some(ClearanceLevel::Internal)
        );
        assert_eq!(
            policy.required_for("/memory/export-pack"),
            Some(ClearanceLevel::Internal)
        );
        assert_eq!(policy.required_for("/memory/search"), None);
    }
}
