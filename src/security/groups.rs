use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::security::clearance::ClearanceLevel;

/// Ruta por defecto del registro de grupos (relativa al workspace).
pub const GROUPS_STORAGE_PATH: &str = "data/security/groups.json";

/// Segmentos internos del laboratorio (planes del 10 %, no-DAO).
///
/// Nacen **sin miembros**: la elevación de nivel no se hereda, se asigna. Cuando
/// el consejo tenga nodos expertos verificados, estos segmentos se revelan al DAO.
pub const LAB_SEGMENTS: [(&str, &str, ClearanceLevel); 4] = [
    (
        "seg-admin",
        "Administración del laboratorio",
        ClearanceLevel::TopSecret,
    ),
    ("seg-research", "Investigación", ClearanceLevel::Secret),
    ("seg-ops", "Operaciones", ClearanceLevel::Confidential),
    ("seg-legal", "Legal y cumplimiento", ClearanceLevel::Secret),
];

/// Groups/permissions ACL + audit trail (WAVE-3.03)
/// Permissions configuration for an Information Group.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupAcl {
    pub read: bool,
    pub write: bool,
    pub audit: bool,
}

/// A structured group of information containing identification, names,
/// members allowed to access, and the respective Access Control List (ACL).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InfoGroup {
    pub id: String,
    pub name: String,
    pub members: Vec<String>,
    pub acl: GroupAcl,
    /// Techo de lectura que otorga la pertenencia al grupo (F3.1).
    ///
    /// Sin nivel declarado ⇒ `default_clearance()` (`Internal`): un grupo sin
    /// nivel explícito **no** eleva a nadie por encima del default del sistema.
    #[serde(default = "crate::security::clearance::default_clearance")]
    pub clearance: ClearanceLevel,
}

/// Central registry managing all `InfoGroup` elements, persistence,
/// membership modifications, and central access control enforcement.
#[derive(Debug, Clone)]
pub struct GroupRegistry {
    groups: HashMap<String, InfoGroup>,
    storage_path: PathBuf,
    audit_log: Vec<GroupAuditEntry>,
}

/// Audit trail entry for groups/permissions (WAVE-3.03)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupAuditEntry {
    pub timestamp: String,
    pub group_id: String,
    pub member_id: String,
    pub action: String,
    pub allowed: bool,
}

impl GroupRegistry {
    /// Load the GroupRegistry from the default configuration path.
    pub fn load() -> Result<Self> {
        Self::load_from(PathBuf::from("data/security/groups.json"))
    }

    /// Load the GroupRegistry from a specified storage path.
    pub fn load_from<P: AsRef<Path>>(storage_path: P) -> Result<Self> {
        let path = storage_path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if !path.exists() {
            return Ok(Self {
                groups: HashMap::new(),
                storage_path: path,
                audit_log: Vec::new(),
            });
        }

        let raw = std::fs::read_to_string(&path).context("Failed to read groups storage file")?;

        let groups = if raw.trim().is_empty() {
            HashMap::new()
        } else {
            serde_json::from_str(&raw).context("Failed to parse groups JSON storage")?
        };

        Ok(Self {
            groups,
            storage_path: path,
            audit_log: Vec::new(),
        })
    }

    /// Techo de lectura que los grupos otorgan a un miembro: el máximo de los
    /// niveles de los grupos donde tiene permiso de lectura. `None` si no
    /// pertenece a ninguno (no se eleva nada).
    pub fn member_ceiling(&self, member_ids: &[&str]) -> Option<ClearanceLevel> {
        self.groups
            .values()
            .filter(|group| {
                group.acl.read
                    && group
                        .members
                        .iter()
                        .any(|member| member_ids.contains(&member.as_str()))
            })
            .map(|group| group.clearance)
            .max()
    }

    /// Crea los segmentos internos del laboratorio que falten (idempotente).
    ///
    /// Nacen **sin miembros** y con `audit` activo: la elevación de nivel no se
    /// hereda, se asigna miembro a miembro. Devuelve los ids creados ahora.
    pub fn ensure_lab_segments(&mut self) -> Result<Vec<String>> {
        let mut created = Vec::new();
        for (id, name, clearance) in LAB_SEGMENTS {
            if self.groups.contains_key(id) {
                continue;
            }
            self.groups.insert(
                id.to_string(),
                InfoGroup {
                    id: id.to_string(),
                    name: name.to_string(),
                    members: Vec::new(),
                    acl: GroupAcl {
                        read: true,
                        write: false,
                        audit: true,
                    },
                    clearance,
                },
            );
            created.push(id.to_string());
        }
        if !created.is_empty() {
            self.save()?;
        }
        Ok(created)
    }

    /// Save the registry to the configured storage path.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.groups)?;
        std::fs::write(&self.storage_path, json).context("Failed to write groups storage file")?;
        Ok(())
    }

    /// Create or update an information group.
    pub fn create(&mut self, group: InfoGroup) -> Result<()> {
        self.groups.insert(group.id.clone(), group);
        self.save()
    }

    /// Add a member to a group if they are not already a member.
    /// Returns Ok(true) if the member was added, Ok(false) if already there,
    /// or an Err if the group does not exist.
    pub fn join(&mut self, group_id: &str, member_id: &str) -> Result<bool> {
        if let Some(group) = self.groups.get_mut(group_id) {
            if !group.members.contains(&member_id.to_string()) {
                group.members.push(member_id.to_string());
                self.save()?;
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Err(anyhow!("Group with ID '{}' not found", group_id))
        }
    }

    /// Retrieve an information group by its ID.
    pub fn get_group(&self, group_id: &str) -> Option<&InfoGroup> {
        self.groups.get(group_id)
    }

    /// Remove a group by its ID. Returns Ok(true) if it existed and was removed, Ok(false) otherwise.
    pub fn remove_group(&mut self, group_id: &str) -> Result<bool> {
        if self.groups.remove(group_id).is_some() {
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Central enforcement check: verify if a member has a specific permission on a group.
    pub fn check_access(&self, group_id: &str, member_id: &str, action: &str) -> bool {
        let group = match self.groups.get(group_id) {
            Some(g) => g,
            None => return false,
        };

        if !group.members.contains(&member_id.to_string()) {
            return false;
        }

        match action.to_lowercase().as_str() {
            "read" => group.acl.read,
            "write" => group.acl.write,
            "audit" => group.acl.audit,
            _ => false,
        }
    }

    /// Enforcement with audit trail — logs every check
    pub fn check_access_audited(&mut self, group_id: &str, member_id: &str, action: &str) -> bool {
        let allowed = self.check_access(group_id, member_id, action);
        self.audit_log.push(GroupAuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            group_id: group_id.to_string(),
            member_id: member_id.to_string(),
            action: action.to_string(),
            allowed,
        });
        allowed
    }

    /// Enforce on all reads — returns audit trail
    pub fn audit_trail(&self) -> &[GroupAuditEntry] {
        &self.audit_log
    }

    /// Number of audit entries
    pub fn audit_len(&self) -> usize {
        self.audit_log.len()
    }

    /// Bypass-attempt test helper: returns true if non-member tried to access
    pub fn was_bypass_attempt(&self, group_id: &str, member_id: &str) -> bool {
        self.audit_log
            .iter()
            .any(|e| e.group_id == group_id && e.member_id == member_id && !e.allowed)
    }
}

/// Registro compartido del camino de lectura.
///
/// Se relee solo si el archivo cambió (mtime + tamaño): una búsqueda no puede
/// pagar un parseo por request, y los escritores (`/v1/f12/groups`) persisten en
/// el mismo archivo, así que el cambio se ve en la siguiente lectura.
fn shared_registry_at(path: &Path) -> Option<GroupRegistry> {
    type Cache = Option<(PathBuf, u64, u64, GroupRegistry)>;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

    let meta = std::fs::metadata(path).ok()?;
    let stamp = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos() as u64;
    let len = meta.len();

    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().ok()?;
    if let Some((cached_path, cached_stamp, cached_len, registry)) = guard.as_ref() {
        if cached_path == path && *cached_stamp == stamp && *cached_len == len {
            return Some(registry.clone());
        }
    }
    let registry = GroupRegistry::load_from(path).ok()?;
    *guard = Some((path.to_path_buf(), stamp, len, registry.clone()));
    Some(registry)
}

/// Techo de lectura que otorgan los grupos para los identificadores dados,
/// contra una ruta explícita (tests y usos no estándar).
pub fn shared_member_ceiling_at(path: &Path, member_ids: &[&str]) -> Option<ClearanceLevel> {
    shared_registry_at(path)?.member_ceiling(member_ids)
}

/// Igual que [`shared_member_ceiling_at`] contra la ruta por defecto del workspace.
pub fn shared_member_ceiling(member_ids: &[&str]) -> Option<ClearanceLevel> {
    shared_member_ceiling_at(Path::new(GROUPS_STORAGE_PATH), member_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn make_test_group(id: &str, name: &str, read: bool, write: bool, audit: bool) -> InfoGroup {
        InfoGroup {
            id: id.to_string(),
            name: name.to_string(),
            members: vec![],
            acl: GroupAcl { read, write, audit },
            clearance: crate::security::clearance::default_clearance(),
        }
    }

    // ── F3.1: techo de lectura por segmento ─────────────────────────────────

    #[test]
    fn test_member_ceiling_requires_read_membership() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let mut secret = make_test_group("seg-research", "Investigación", true, false, true);
        secret.clearance = ClearanceLevel::Secret;
        secret.members = vec!["nodo-1".to_string()];
        registry.create(secret).unwrap();

        // Miembro con lectura ⇒ techo del grupo.
        assert_eq!(
            registry.member_ceiling(&["nodo-1"]),
            Some(ClearanceLevel::Secret)
        );
        // También por email (el solicitante puede venir identificado así).
        assert_eq!(
            registry.member_ceiling(&["otro", "nodo-1"]),
            Some(ClearanceLevel::Secret)
        );
        // No miembro ⇒ nada que elevar.
        assert_eq!(registry.member_ceiling(&["nodo-2"]), None);

        // Grupo sin permiso de lectura: la membresía no eleva.
        let mut no_read = make_test_group("seg-x", "Sin lectura", false, true, false);
        no_read.clearance = ClearanceLevel::TopSecret;
        no_read.members = vec!["nodo-3".to_string()];
        registry.create(no_read).unwrap();
        assert_eq!(registry.member_ceiling(&["nodo-3"]), None);
    }

    #[test]
    fn test_member_ceiling_is_max_over_segments() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let mut ops = make_test_group("seg-ops", "Operaciones", true, false, true);
        ops.clearance = ClearanceLevel::Confidential;
        ops.members = vec!["nodo-1".to_string()];
        registry.create(ops).unwrap();

        let mut research = make_test_group("seg-research", "Investigación", true, false, true);
        research.clearance = ClearanceLevel::Secret;
        research.members = vec!["nodo-1".to_string()];
        registry.create(research).unwrap();

        assert_eq!(
            registry.member_ceiling(&["nodo-1"]),
            Some(ClearanceLevel::Secret),
            "el techo es el máximo de los segmentos donde lee"
        );
    }

    #[test]
    fn test_group_without_clearance_does_not_elevate() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let mut plain = make_test_group("proj", "Proyecto normal", true, false, false);
        plain.members = vec!["nodo-9".to_string()];
        registry.create(plain).unwrap();

        assert_eq!(
            registry.member_ceiling(&["nodo-9"]),
            Some(crate::security::clearance::default_clearance()),
            "sin nivel explícito el grupo solo otorga el default del sistema"
        );
    }

    #[test]
    fn test_ensure_lab_segments_is_idempotent_and_empty() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let created = registry.ensure_lab_segments().unwrap();
        assert_eq!(created.len(), 4, "crea los 4 segmentos del laboratorio");
        assert!(created.contains(&"seg-admin".to_string()));
        assert!(registry.ensure_lab_segments().unwrap().is_empty());

        for (id, _, clearance) in LAB_SEGMENTS {
            let group = registry.get_group(id).expect("segmento creado");
            assert_eq!(group.clearance, clearance);
            assert!(group.acl.read && group.acl.audit);
            assert!(
                group.members.is_empty(),
                "un segmento nace sin miembros: la elevación se asigna"
            );
        }

        // Nadie recibe elevación por existir los segmentos.
        assert_eq!(registry.member_ceiling(&["nodo-1"]), None);
        assert_eq!(
            registry.get_group("seg-admin").unwrap().clearance,
            ClearanceLevel::TopSecret
        );
    }

    #[test]
    fn test_shared_ceiling_reads_file_and_reloads_on_change() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        let mut registry = GroupRegistry::load_from(&path).unwrap();
        let mut seg = make_test_group("seg-legal", "Legal", true, false, true);
        seg.clearance = ClearanceLevel::Secret;
        registry.create(seg).unwrap();
        drop(registry);

        // Sin membresía ⇒ sin elevación.
        assert_eq!(
            shared_member_ceiling_at(&path, &["nodo-7"]),
            None,
            "archivo sin ese miembro no eleva"
        );

        // Al agregar la membresía, la siguiente lectura lo ve (recarga por mtime).
        let mut registry = GroupRegistry::load_from(&path).unwrap();
        registry.join("seg-legal", "nodo-7").unwrap();
        assert_eq!(
            shared_member_ceiling_at(&path, &["nodo-7"]),
            Some(ClearanceLevel::Secret)
        );
    }

    #[test]
    fn test_create_group() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("core-dev", "Core Developers", true, true, false);
        let create_res = registry.create(group.clone());
        assert!(create_res.is_ok());

        let retrieved = registry.get_group("core-dev");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Core Developers");
    }

    #[test]
    fn test_join_group() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("core-dev", "Core Developers", true, true, false);
        registry.create(group).unwrap();

        let join_res1 = registry.join("core-dev", "bela");
        assert!(join_res1.unwrap());

        // Try joining again - should return false as already joined
        let join_res2 = registry.join("core-dev", "bela");
        assert!(!join_res2.unwrap());

        let group = registry.get_group("core-dev").unwrap();
        assert_eq!(group.members, vec!["bela".to_string()]);
    }

    #[test]
    fn test_join_nonexistent_group() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let join_res = registry.join("nonexistent", "bela");
        assert!(join_res.is_err());
    }

    #[test]
    fn test_check_access_read_allowed() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("readers", "Read Only Group", true, false, false);
        registry.create(group).unwrap();
        registry.join("readers", "bela").unwrap();

        assert!(registry.check_access("readers", "bela", "read"));
    }

    #[test]
    fn test_check_access_read_denied() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("no-read", "No Read Group", false, true, false);
        registry.create(group).unwrap();
        registry.join("no-read", "bela").unwrap();

        assert!(!registry.check_access("no-read", "bela", "read"));
    }

    #[test]
    fn test_check_access_write_allowed() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("writers", "Write Group", true, true, false);
        registry.create(group).unwrap();
        registry.join("writers", "bela").unwrap();

        assert!(registry.check_access("writers", "bela", "write"));
    }

    #[test]
    fn test_check_access_write_denied() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        // Target test requirement: membre sin permiso write -> check_access(write) = false
        let group = make_test_group("readers", "Read Only Group", true, false, false);
        registry.create(group).unwrap();
        registry.join("readers", "bela").unwrap();

        assert!(!registry.check_access("readers", "bela", "write"));
    }

    #[test]
    fn test_check_access_audit_allowed() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("auditors", "Audit Group", false, false, true);
        registry.create(group).unwrap();
        registry.join("auditors", "bela").unwrap();

        assert!(registry.check_access("auditors", "bela", "audit"));
    }

    #[test]
    fn test_check_access_audit_denied() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("no-audit", "No Audit Group", true, true, false);
        registry.create(group).unwrap();
        registry.join("no-audit", "bela").unwrap();

        assert!(!registry.check_access("no-audit", "bela", "audit"));
    }

    #[test]
    fn test_check_access_non_member() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("writers", "Write Group", true, true, true);
        registry.create(group).unwrap();

        // 'stranger' is not a member of the group
        assert!(!registry.check_access("writers", "stranger", "read"));
        assert!(!registry.check_access("writers", "stranger", "write"));
        assert!(!registry.check_access("writers", "stranger", "audit"));
    }

    #[test]
    fn test_check_access_nonexistent_group() {
        let temp_file = NamedTempFile::new().unwrap();
        let registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        assert!(!registry.check_access("nonexistent", "bela", "read"));
    }

    #[test]
    fn test_check_access_unknown_action() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("writers", "Write Group", true, true, true);
        registry.create(group).unwrap();
        registry.join("writers", "bela").unwrap();

        assert!(!registry.check_access("writers", "bela", "delete"));
        assert!(!registry.check_access("writers", "bela", "manage"));
    }

    #[test]
    fn test_registry_persistence() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        {
            let mut registry = GroupRegistry::load_from(&path).unwrap();
            let mut group = make_test_group("family", "Family Group", true, true, true);
            group.members.push("bela".to_string());
            registry.create(group).unwrap();
        } // drops and saves

        // reload from the same file
        let reloaded = GroupRegistry::load_from(&path).unwrap();
        let group = reloaded.get_group("family");
        assert!(group.is_some());
        let g = group.unwrap();
        assert_eq!(g.name, "Family Group");
        assert_eq!(g.members, vec!["bela".to_string()]);
        assert!(g.acl.read);
        assert!(g.acl.write);
        assert!(g.acl.audit);
    }
}
