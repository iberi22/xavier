//! Legacy migration (WP-13o).
//!
//! The node's pre-espacio data lives in ONE workspace (the "default" one,
//! `{memory.workspace_dir}/{default_workspace_id}`) plus independent
//! `{data_dir}/db/*.sqlite` files. Migration is a pure RECORD: at boot a
//! descriptor `{spaces}/{id}/space.json` with `legacy: true` is written that
//! points at those paths by reference. Legacy data is never moved, copied,
//! rewritten or re-encrypted, and no legacy path is created or opened here.
//!
//! The descriptor is deliberately NOT a registered, token-addressable space:
//! the default workspace keeps being served by the root credential exactly as
//! before, and a legacy descriptor never gets its own store, links or tokens.
//! It only shows up in the admin plane (`default`) and makes the id
//! unavailable for a new space. `*.sqlite` files under the legacy db dir are
//! listed as `adoptable` (listing only; there is no adopt action yet).
//!
//! Rollback: `XAVIER_SPACES=off` never opens a `SpaceManager`, so nothing in
//! this module runs and the legacy path behaves exactly as before.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::store::{self, validate_space_id};

/// Reference to the pre-espacio workspace. Paths are recorded, never opened.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegacyWorkspace {
    /// Id of the legacy workspace (settings `default_workspace_id`).
    pub id: String,
    /// Legacy workspace root (`{workspace_dir}/{id}`).
    pub workspace_root: PathBuf,
    /// Directory of the legacy independent `*.sqlite` databases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_dir: Option<PathBuf>,
}

impl LegacyWorkspace {
    /// The legacy workspace of this node, from the settings (same inputs the
    /// workspace registry and `MultiDbManager` use). Pure computation.
    pub fn from_settings() -> Self {
        let s = crate::settings::XavierSettings::current();
        let id = s.workspace.default_workspace_id.clone();
        let data_dir = if s.memory.data_dir.trim().is_empty() {
            PathBuf::from("data")
        } else {
            PathBuf::from(&s.memory.data_dir)
        };
        Self {
            workspace_root: PathBuf::from(&s.memory.workspace_dir).join(&id),
            db_dir: Some(data_dir.join("db")),
            id,
        }
    }
}

/// A legacy descriptor as loaded at boot.
#[derive(Debug, Clone, Serialize)]
pub struct LegacySpace {
    pub id: String,
    pub name: String,
    /// Always true; lets clients tell it apart from real spaces.
    pub legacy: bool,
    pub created_at: DateTime<Utc>,
    pub store: LegacyWorkspace,
}

/// A legacy `*.sqlite` file that could be adopted as a space later.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AdoptableDb {
    pub db_id: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

/// Write `{spaces_dir}/{id}/space.json` for the legacy workspace when no
/// descriptor exists yet. Returns true when a descriptor was written.
///
/// Idempotent and non-destructive: an existing descriptor (legacy or a real
/// space that happens to use the id) is never touched, even a corrupt one.
/// Only the espacio directory is written; the legacy paths are not.
pub fn ensure_default_space(spaces_dir: &Path, legacy: &LegacyWorkspace) -> Result<bool> {
    validate_space_id(&legacy.id).map_err(|_| {
        anyhow!(
            "default workspace id {:?} is not a valid space id",
            legacy.id
        )
    })?;
    let dir = spaces_dir.join(&legacy.id);
    let desc = dir.join(store::DESCRIPTOR_FILE);
    if desc.exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(&dir)?;
    let body = serde_json::json!({
        "version": 1,
        "id": legacy.id,
        "name": "default workspace (legacy)",
        "description": "Pre-espacio workspace, referenced in place; never moved or rewritten",
        "owner_node": "root",
        "is_public": false,
        "created_at": Utc::now(),
        "namespace": format!("xavier://{}/xavier/default", legacy.id),
        "legacy": true,
        "legacy_store": legacy,
    });
    store::write_atomic(&desc, &serde_json::to_vec_pretty(&body)?)?;
    Ok(true)
}

/// `*.sqlite` files directly under `db_dir` whose stem is a valid id and not
/// in `taken`, sorted by id. A read-only directory listing.
pub fn list_adoptable(db_dir: &Path, taken: &[String]) -> Vec<AdoptableDb> {
    let Ok(rd) = std::fs::read_dir(db_dir) else {
        return Vec::new();
    };
    let mut out: Vec<AdoptableDb> = rd
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("sqlite") || !path.is_file() {
                return None;
            }
            let stem = path.file_stem()?.to_str()?.to_string();
            if validate_space_id(&stem).is_err() || taken.contains(&stem) {
                return None;
            }
            let size_bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
            Some(AdoptableDb {
                db_id: stem,
                path,
                size_bytes,
            })
        })
        .collect();
    out.sort_by(|a, b| a.db_id.cmp(&b.db_id));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::espacio::manager::SpaceManager;
    use crate::espacio::StaticNodeKek;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    /// Hash of every file under `root` (relative path -> sha256), plus the
    /// directory set, so additions, removals and rewrites are all visible.
    fn snapshot(root: &Path) -> BTreeMap<String, String> {
        fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().to_string();
                if p.is_dir() {
                    out.insert(format!("{rel}/"), String::new());
                    walk(&p, root, out);
                } else {
                    let h = Sha256::digest(std::fs::read(&p).unwrap());
                    out.insert(
                        rel,
                        h.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(root, root, &mut out);
        out
    }

    struct Fx {
        _tmp: tempfile::TempDir,
        data: PathBuf,
        legacy: LegacyWorkspace,
    }

    fn fixture() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let root = tmp.path().join("workspaces/default");
        let db_dir = tmp.path().join("legacy-db");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(&db_dir).unwrap();
        std::fs::write(root.join("memory.sqlite"), b"legacy-main-store-bytes").unwrap();
        std::fs::write(root.join("sub/notes.db"), b"more legacy bytes").unwrap();
        std::fs::write(db_dir.join("projx.sqlite"), b"projx").unwrap();
        std::fs::write(db_dir.join("registry.json"), b"{}").unwrap();
        std::fs::write(db_dir.join("bad name.sqlite"), b"x").unwrap();
        Fx {
            legacy: LegacyWorkspace {
                id: "default".into(),
                workspace_root: root,
                db_dir: Some(db_dir),
            },
            data,
            _tmp: tmp,
        }
    }

    fn boot(f: &Fx) -> SpaceManager {
        SpaceManager::open_with_keys_and_legacy(
            &f.data,
            Arc::new(StaticNodeKek::new([4u8; 32])),
            Some(f.legacy.clone()),
        )
    }

    #[tokio::test]
    async fn migration_leaves_legacy_files_byte_identical() {
        let f = fixture();
        let parent = f.legacy.workspace_root.parent().unwrap().to_path_buf();
        let db_dir = f.legacy.db_dir.clone().unwrap();
        let before = (snapshot(&parent), snapshot(&db_dir));
        let m = boot(&f);
        let after = (snapshot(&parent), snapshot(&db_dir));
        assert_eq!(before, after, "legacy store files must be untouched");
        // The descriptor is the only artifact, inside the espacio dir.
        let desc: serde_json::Value = serde_json::from_slice(
            &std::fs::read(f.data.join("spaces/default/space.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(desc["legacy"], true);
        assert_eq!(desc["id"], "default");
        assert_eq!(
            desc["legacy_store"]["workspace_root"],
            f.legacy.workspace_root.to_string_lossy().as_ref()
        );
        // Not a registered space: root keeps serving the default workspace.
        assert!(m.list().await.is_empty());
        assert!(m.get("default").await.is_err());
        assert_eq!(m.legacy().unwrap().id, "default");
    }

    #[tokio::test]
    async fn migration_is_idempotent() {
        let f = fixture();
        drop(boot(&f));
        let desc = f.data.join("spaces/default/space.json");
        let first = std::fs::read(&desc).unwrap();
        let snap = snapshot(&f.data);
        let m = boot(&f);
        assert_eq!(std::fs::read(&desc).unwrap(), first, "descriptor rewritten");
        assert_eq!(snapshot(&f.data), snap, "second boot changed the data dir");
        assert!(m.legacy().is_some());
    }

    #[tokio::test]
    async fn existing_real_space_with_the_id_is_never_replaced() {
        let f = fixture();
        {
            let m = SpaceManager::open(&f.data);
            m.create(
                "default".into(),
                "real".into(),
                "".into(),
                "o".into(),
                false,
            )
            .await
            .unwrap();
        }
        let m = boot(&f);
        assert!(m.legacy().is_none());
        assert_eq!(m.get("default").await.unwrap().name, "real");
    }

    #[tokio::test]
    async fn legacy_id_cannot_be_created_as_a_space() {
        let f = fixture();
        let m = boot(&f);
        let err = m
            .create("default".into(), "x".into(), "".into(), "o".into(), false)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[tokio::test]
    async fn adoptable_lists_unregistered_sqlite_only() {
        let f = fixture();
        let m = boot(&f);
        let a = m.adoptable();
        assert_eq!(
            a.iter().map(|d| d.db_id.as_str()).collect::<Vec<_>>(),
            ["projx"],
            "registry.json and invalid names are not adoptable"
        );
        // A space with the same id takes it out of the list.
        m.create("projx".into(), "p".into(), "".into(), "o".into(), false)
            .await
            .unwrap();
        assert!(m.adoptable().is_empty());
        // Listing moved nothing.
        assert!(f.legacy.db_dir.unwrap().join("projx.sqlite").is_file());
    }

    #[tokio::test]
    async fn plain_open_without_legacy_writes_nothing_extra() {
        let f = fixture();
        let m = SpaceManager::open(&f.data);
        assert!(m.legacy().is_none());
        assert!(!f.data.join("spaces/default").exists());
    }
}
