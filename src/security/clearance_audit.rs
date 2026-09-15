//! Auditoría de lecturas clasificadas (F3.2).
//!
//! Registra cada decisión de lectura con nivel —**concedida o denegada**— en un
//! archivo append-only (JSONL), porque un control sin rastro no es auditable: si
//! mañana alguien pregunta "¿quién leyó los planes del segmento de investigación?",
//! la respuesta tiene que estar en disco.
//!
//! Privacidad: **no** se guarda la query cruda, solo su hash y longitud. El sujeto
//! se guarda como `sub`/email del token (los identificadores que ya circulan en el
//! sistema), nunca contenido de memoria.
//!
//! Política de fallo: si el archivo no se puede escribir se emite `tracing::error`
//! y la lectura **continúa** — perder una línea de auditoría no puede tumbar el
//! servicio; el error queda visible y conta(ble) en los logs.

use crate::security::auth::Claims;
use crate::security::clearance::ClearanceLevel;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;

/// Ruta del archivo de auditoría (relativa al workspace).
pub const CLEARANCE_AUDIT_PATH: &str = "data/security/clearance_audit.jsonl";
/// Override por entorno (tests / operación con otro volumen).
pub const CLEARANCE_AUDIT_ENV: &str = "XAVIER_CLEARANCE_AUDIT_PATH";

/// Una decisión de lectura con nivel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClearanceReadAudit {
    /// RFC3339 UTC.
    pub timestamp: String,
    /// Sujeto autenticado (`sub`, o el email si el primero viene vacío).
    pub subject: String,
    /// Rol declarado por el token.
    pub role: String,
    /// Techo efectivo con el que se evaluó (rol ∪ segmentos).
    pub requester_level: String,
    /// Ruta HTTP donde ocurrió la lectura.
    pub route: String,
    /// Acción: `search`, `get`, `export`, `route_denied`.
    pub action: String,
    /// sha256 corto de la query (no se guarda la query).
    pub query_hash: String,
    /// Entradas devueltas.
    pub visible: usize,
    /// Entradas ocultas por nivel (la búsqueda no revela cuáles).
    pub hidden_by_clearance: usize,
    /// Si la operación se permitió.
    pub allowed: bool,
}

impl ClearanceReadAudit {
    /// Construye el registro con timestamp actual.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        subject: impl Into<String>,
        role: impl Into<String>,
        requester_level: ClearanceLevel,
        route: impl Into<String>,
        action: impl Into<String>,
        query: &str,
        visible: usize,
        hidden_by_clearance: usize,
        allowed: bool,
    ) -> Self {
        Self {
            timestamp: chrono::Utc::now().to_rfc3339(),
            subject: subject.into(),
            role: role.into(),
            requester_level: requester_level.as_str().to_uppercase(),
            route: route.into(),
            action: action.into(),
            query_hash: query_hash(query),
            visible,
            hidden_by_clearance,
            allowed,
        }
    }

    /// Serializa como una línea JSONL (sin salto final).
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Hash corto de una query: permite correlacionar sin almacenar el texto.
pub fn query_hash(query: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(query.as_bytes());
    let digest = hasher.finalize();
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Sujeto y rol a partir de las claims (o valores anónimos).
pub fn subject_and_role(claims: Option<&Claims>) -> (String, String) {
    match claims {
        Some(claims) if !claims.sub.is_empty() => {
            (claims.sub.clone(), format!("{:?}", claims.role))
        }
        Some(claims) => (claims.email.clone(), format!("{:?}", claims.role)),
        None => ("anonymous".to_string(), "none".to_string()),
    }
}

/// Ruta del archivo de auditoría activo.
pub fn audit_path() -> std::path::PathBuf {
    std::env::var(CLEARANCE_AUDIT_ENV)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(CLEARANCE_AUDIT_PATH))
}

/// Añade una línea al archivo dado (append-only). No crea directorios: si falta,
/// devuelve el error para que el llamador lo reporte.
pub fn append_at(path: &Path, entry: &ClearanceReadAudit) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", entry.to_line())
}

/// Registra la decisión en el archivo activo. Nunca falla hacia el llamador: un
/// fallo de auditoría se reporta por log y la operación continúa.
pub fn record(entry: &ClearanceReadAudit) {
    if let Err(error) = append_at(&audit_path(), entry) {
        tracing::error!(
            route = %entry.route,
            subject = %entry.subject,
            "auditoría de clearance no se pudo escribir: {}",
            error
        );
    }
}

/// Registra una lectura permitida y devuelve la entrada (útil en tests).
#[allow(clippy::too_many_arguments)]
pub fn record_allowed(
    claims: Option<&Claims>,
    requester_level: ClearanceLevel,
    route: &str,
    action: &str,
    query: &str,
    visible: usize,
    hidden_by_clearance: usize,
) -> ClearanceReadAudit {
    let (subject, role) = subject_and_role(claims);
    let entry = ClearanceReadAudit::new(
        subject,
        role,
        requester_level,
        route,
        action,
        query,
        visible,
        hidden_by_clearance,
        true,
    );
    record(&entry);
    entry
}

/// Registra una denegación (por política de ruta o por nivel insuficiente).
pub fn record_denied(
    claims: Option<&Claims>,
    requester_level: ClearanceLevel,
    route: &str,
    action: &str,
) -> ClearanceReadAudit {
    let (subject, role) = subject_and_role(claims);
    let entry = ClearanceReadAudit::new(
        subject,
        role,
        requester_level,
        route,
        action,
        "",
        0,
        0,
        false,
    );
    record(&entry);
    entry
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_query_hash_is_stable_and_hides_text() {
        let h1 = query_hash("planes internos del 10%");
        let h2 = query_hash("planes internos del 10%");
        let h3 = query_hash("planes internos del 10%!");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 16, "hash corto (8 bytes en hex)");
        assert!(!h1.contains("planes"), "el texto no aparece en el hash");
    }

    #[test]
    fn test_append_writes_jsonl_and_can_be_read_back() {
        let file = NamedTempFile::new().unwrap();
        let entry = ClearanceReadAudit::new(
            "nodo-1",
            "User",
            ClearanceLevel::Confidential,
            "/memory/search",
            "search",
            "consulta de prueba",
            3,
            2,
            true,
        );

        append_at(file.path(), &entry).unwrap();
        append_at(file.path(), &entry).unwrap();

        let raw = std::fs::read_to_string(file.path()).unwrap();
        let lines: Vec<&str> = raw.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 2, "append-only: dos líneas, nunca sobrescribe");

        let parsed: ClearanceReadAudit = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed.subject, "nodo-1");
        assert_eq!(parsed.requester_level, "CONFIDENTIAL");
        assert_eq!(parsed.hidden_by_clearance, 2);
        assert_eq!(parsed.visible, 3);
        assert!(parsed.allowed);
    }

    #[test]
    fn test_subject_and_role_from_claims() {
        use crate::security::auth::UserRole;
        let claims = Claims::new(
            "nodo-7".into(),
            "nodo7@swal.dev".into(),
            UserRole::User,
            chrono::Duration::hours(1),
        );
        let (subject, role) = subject_and_role(Some(&claims));
        assert_eq!(subject, "nodo-7");
        assert!(!role.is_empty());

        let (anon, role_none) = subject_and_role(None);
        assert_eq!(anon, "anonymous");
        assert_eq!(role_none, "none");
    }

    #[test]
    fn test_record_uses_env_path_when_set() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_string_lossy().to_string();
        std::env::set_var(CLEARANCE_AUDIT_ENV, &path);

        let entry = record_denied(
            None,
            ClearanceLevel::Unclassified,
            "/memory/export",
            "route_denied",
        );
        assert!(!entry.allowed);

        let raw = std::fs::read_to_string(file.path()).unwrap();
        assert!(raw.contains("route_denied"), "la denegación quedó auditada");
        std::env::remove_var(CLEARANCE_AUDIT_ENV);
    }
}
