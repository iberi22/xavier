//! Link-object backups and atomic file persistence for skill controller.
//!
//! Provides write-ahead backup state and crash-safe file persistence.
//!
//! Containment is delegated to the J02 policy layer: callers must authorize
//! `base_dir` and every `target_path` through `FsPolicy` before calling in;
//! this module never re-canonicalizes caller paths. All operations are
//! synchronous `std::fs`, so callers on a Tokio worker must wrap them in
//! `tokio::task::spawn_blocking`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const BACKUPS_SUBDIR: &str = "backups";
const _: () = assert!(!BACKUPS_SUBDIR.is_empty());

/// Per-file backup cap, matching the J02 package member bound
/// (`PACKAGE_BOUNDS.3`): foreign files are untrusted input.
pub const MAX_BACKUP_BYTES: u64 = 1 << 20;
const _: () = assert!(MAX_BACKUP_BYTES > 0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum BackupKind {
    Symlink {
        target: PathBuf,
    },
    OwnedCopy {
        hash: String,
        backup_path: Option<PathBuf>,
    },
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupRecord {
    pub target_path: PathBuf,
    pub kind: BackupKind,
    pub backed_up_at: u64,
}

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("I/O error at {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("unmanaged directory collision at {0}")]
    UnmanagedDirectory(PathBuf),
    #[error("backup source {path} exceeds the {limit} byte cap")]
    BoundExceeded { path: PathBuf, limit: u64 },
}

#[derive(Debug, Clone)]
pub struct BackupManager {
    base_dir: PathBuf,
}

#[cfg(unix)]
fn set_dir_permissions(path: &Path) -> Result<(), BackupError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| BackupError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn set_dir_permissions(_path: &Path) -> Result<(), BackupError> {
    Ok(())
}

impl BackupManager {
    pub fn new(base_dir: PathBuf) -> Result<Self, BackupError> {
        let preexisting = base_dir.exists();
        if let Err(source) = fs::create_dir_all(&base_dir) {
            return Err(BackupError::Io {
                path: base_dir,
                source,
            });
        }
        // Restrict the mode only when this constructor created the directory;
        // a pre-existing shared directory keeps its owner's permissions.
        if !preexisting {
            set_dir_permissions(&base_dir)?;
        }
        Ok(Self { base_dir })
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Directory holding every backup record of one transaction. The `tx_id`
    /// is hashed into the directory name so arbitrary `tx_id` content can
    /// never traverse out of `base_dir`.
    pub fn backups_dir(&self, tx_id: &str) -> PathBuf {
        use sha2::{Digest, Sha256};
        let hash = crate::crypto::hex_encode(Sha256::digest(tx_id.as_bytes()));
        let short = if hash.len() >= 16 { &hash[..16] } else { &hash };
        self.base_dir.join(BACKUPS_SUBDIR).join(short)
    }

    pub fn backup_file_path(&self, tx_id: &str, target_path: &Path) -> PathBuf {
        use sha2::{Digest, Sha256};
        let hash =
            crate::crypto::hex_encode(Sha256::digest(target_path.to_string_lossy().as_bytes()));
        let short = if hash.len() >= 16 { &hash[..16] } else { &hash };
        let name = target_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("entry");
        self.backups_dir(tx_id).join(format!("{short}-{name}.json"))
    }

    pub fn backup_target(
        &self,
        tx_id: &str,
        target_path: &Path,
        dry_run: bool,
    ) -> Result<BackupKind, BackupError> {
        let meta = match fs::symlink_metadata(target_path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BackupKind::Absent),
            Err(source) => {
                return Err(BackupError::Io {
                    path: target_path.to_path_buf(),
                    source,
                })
            }
        };

        if meta.file_type().is_symlink() {
            let target = fs::read_link(target_path).map_err(|source| BackupError::Io {
                path: target_path.to_path_buf(),
                source,
            })?;
            let kind = BackupKind::Symlink { target };
            if !dry_run {
                let record = BackupRecord {
                    target_path: target_path.to_path_buf(),
                    kind: kind.clone(),
                    backed_up_at: current_timestamp(),
                };
                let backup_file = self.backup_file_path(tx_id, target_path);
                atomic_write(&backup_file, &serde_json::to_vec_pretty(&record)?)?;
            }
            return Ok(kind);
        }

        if meta.is_file() {
            if meta.len() > MAX_BACKUP_BYTES {
                return Err(BackupError::BoundExceeded {
                    path: target_path.to_path_buf(),
                    limit: MAX_BACKUP_BYTES,
                });
            }
            let content = fs::read(target_path).map_err(|source| BackupError::Io {
                path: target_path.to_path_buf(),
                source,
            })?;
            use sha2::{Digest, Sha256};
            let hash = crate::crypto::hex_encode(Sha256::digest(&content));
            let backup_path = if !dry_run {
                let backup_file = self.backup_file_path(tx_id, target_path);
                let content_path = backup_file.with_extension("data");
                atomic_write(&content_path, &content)?;
                let record = BackupRecord {
                    target_path: target_path.to_path_buf(),
                    kind: BackupKind::OwnedCopy {
                        hash: hash.clone(),
                        backup_path: Some(content_path.clone()),
                    },
                    backed_up_at: current_timestamp(),
                };
                atomic_write(&backup_file, &serde_json::to_vec_pretty(&record)?)?;
                Some(content_path)
            } else {
                None
            };
            return Ok(BackupKind::OwnedCopy { hash, backup_path });
        }

        if meta.is_dir() {
            return Err(BackupError::UnmanagedDirectory(target_path.to_path_buf()));
        }

        Ok(BackupKind::Absent)
    }

    pub fn read_backup(
        &self,
        tx_id: &str,
        target_path: &Path,
    ) -> Result<Option<BackupRecord>, BackupError> {
        let path = self.backup_file_path(tx_id, target_path);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(BackupError::Io { path, source }),
        };
        Ok(Some(serde_json::from_slice(&bytes)?))
    }
}

pub fn current_timestamp() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => 0,
    }
}

pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), BackupError> {
    let parent = match path.parent() {
        Some(p) => p,
        None => {
            return Err(BackupError::Io {
                path: path.to_path_buf(),
                source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
            })
        }
    };
    let preexisting = parent.exists();
    if let Err(source) = fs::create_dir_all(parent) {
        return Err(BackupError::Io {
            path: parent.to_path_buf(),
            source,
        });
    }
    // Same rule as `BackupManager::new`: only directories this call created
    // get the restrictive mode; a pre-existing parent keeps its owner's mode.
    if !preexisting {
        set_dir_permissions(parent)?;
    }
    let rand_id = uuid::Uuid::new_v4().simple();
    let tmp = parent.join(format!(".tmp-{}-{}", current_timestamp(), rand_id));
    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
    {
        Ok(f) => f,
        Err(source) => return Err(BackupError::Io { path: tmp, source }),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    if let Err(source) = file.write_all(content) {
        let _ = fs::remove_file(&tmp);
        return Err(BackupError::Io { path: tmp, source });
    }
    if let Err(source) = file.sync_all() {
        let _ = fs::remove_file(&tmp);
        return Err(BackupError::Io { path: tmp, source });
    }
    drop(file);
    if let Err(source) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(BackupError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    #[cfg(unix)]
    {
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_symlink_backup_preserves_readlink_without_dereferencing() {
        let dir = tempdir().unwrap();
        let manager = BackupManager::new(dir.path().join("state")).unwrap();
        let target_dir = dir.path().join("real_skill_dir");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("SKILL.md"), "name: test-skill").unwrap();
        fs::write(target_dir.join("extra.txt"), "payload").unwrap();

        let symlink_path = dir.path().join("symlink_skill");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_dir, &symlink_path).unwrap();

        let tx_id = "tx-symlink-01";
        let backup = manager.backup_target(tx_id, &symlink_path, false).unwrap();
        match backup {
            BackupKind::Symlink { target } => {
                assert_eq!(target, target_dir);
                assert_eq!(target, fs::read_link(&symlink_path).unwrap());
            }
            _ => panic!("expected symlink backup kind"),
        }
        let record = manager
            .read_backup(tx_id, &symlink_path)
            .unwrap()
            .expect("backup record exists");
        match record.kind {
            BackupKind::Symlink { target } => assert_eq!(target, target_dir),
            _ => panic!("expected symlink backup in record"),
        }
        assert_eq!(record.target_path, symlink_path);
        assert!(!manager.backups_dir(tx_id).join("SKILL.md").exists());
        assert!(!manager.backups_dir(tx_id).join("extra.txt").exists());
    }

    #[test]
    fn test_owned_copy_backup_for_regular_file() {
        let dir = tempdir().unwrap();
        let manager = BackupManager::new(dir.path().join("state")).unwrap();
        let file_path = dir.path().join("SKILL.md");
        fs::write(&file_path, b"content to preserve").unwrap();

        let tx_id = "tx-file-01";
        let backup = manager.backup_target(tx_id, &file_path, false).unwrap();
        match backup {
            BackupKind::OwnedCopy { hash, backup_path } => {
                assert!(!hash.is_empty());
                let p = backup_path.expect("backup_path should be present when dry_run is false");
                assert!(p.exists());
                assert_eq!(fs::read(&p).unwrap(), b"content to preserve");
            }
            _ => panic!("expected owned copy backup kind"),
        }

        let record = manager
            .read_backup(tx_id, &file_path)
            .unwrap()
            .expect("backup record exists");
        match record.kind {
            BackupKind::OwnedCopy { hash, backup_path } => {
                assert!(!hash.is_empty());
                assert!(backup_path.is_some());
            }
            _ => panic!("expected owned copy in record"),
        }
    }

    #[test]
    fn test_dry_run_does_not_write_backup() {
        let dir = tempdir().unwrap();
        let manager = BackupManager::new(dir.path().join("state")).unwrap();
        let file_path = dir.path().join("dry_skill.txt");
        fs::write(&file_path, b"dry run payload").unwrap();

        let tx_id = "tx-dry-01";
        let backup = manager.backup_target(tx_id, &file_path, true).unwrap();
        match backup {
            BackupKind::OwnedCopy { hash, backup_path } => {
                assert!(!hash.is_empty());
                assert!(backup_path.is_none());
            }
            _ => panic!("expected owned copy backup kind"),
        }

        assert!(!manager.backups_dir(tx_id).exists());
        assert!(manager.read_backup(tx_id, &file_path).unwrap().is_none());
    }

    #[test]
    fn test_absent_target_returns_absent() {
        let dir = tempdir().unwrap();
        let manager = BackupManager::new(dir.path().join("state")).unwrap();
        let nonexistent = dir.path().join("no_such_file");

        let tx_id = "tx-absent-01";
        let backup = manager.backup_target(tx_id, &nonexistent, false).unwrap();
        assert_eq!(backup, BackupKind::Absent);
        assert!(!manager.backups_dir(tx_id).exists());
    }

    #[test]
    fn test_unmanaged_directory_returns_error() {
        let dir = tempdir().unwrap();
        let manager = BackupManager::new(dir.path().join("state")).unwrap();
        let unmanaged_dir = dir.path().join("normal_dir");
        fs::create_dir_all(&unmanaged_dir).unwrap();

        let tx_id = "tx-unmanaged-01";
        let result = manager.backup_target(tx_id, &unmanaged_dir, false);
        assert!(matches!(result, Err(BackupError::UnmanagedDirectory(_))));
    }

    #[test]
    fn test_oversized_target_is_refused_without_writing() {
        let dir = tempdir().unwrap();
        let manager = BackupManager::new(dir.path().join("state")).unwrap();
        let file_path = dir.path().join("huge.bin");
        let oversized = vec![b'x'; (MAX_BACKUP_BYTES + 1) as usize];
        fs::write(&file_path, &oversized).unwrap();

        let tx_id = "tx-oversized-01";
        let result = manager.backup_target(tx_id, &file_path, false);
        assert!(matches!(result, Err(BackupError::BoundExceeded { .. })));
        assert!(!manager.backups_dir(tx_id).exists());
    }

    #[test]
    fn test_tx_id_with_traversal_components_stays_inside_base_dir() {
        let dir = tempdir().unwrap();
        let manager = BackupManager::new(dir.path().join("state")).unwrap();
        let file_path = dir.path().join("SKILL.md");
        fs::write(&file_path, b"traversal payload").unwrap();

        let evil_tx = "../../escape";
        assert!(manager.backups_dir(evil_tx).starts_with(manager.base_dir()));

        let backup = manager.backup_target(evil_tx, &file_path, false).unwrap();
        match backup {
            BackupKind::OwnedCopy {
                backup_path: Some(p),
                ..
            } => assert!(p.starts_with(manager.base_dir())),
            _ => panic!("expected owned copy backup kind"),
        }
        assert!(manager.read_backup(evil_tx, &file_path).unwrap().is_some());
    }

    #[test]
    fn test_new_restricts_mode_only_for_directories_it_creates() {
        let dir = tempdir().unwrap();
        let fresh = BackupManager::new(dir.path().join("fresh")).unwrap();
        assert!(fresh.base_dir().exists());
        let shared = dir.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let manager = BackupManager::new(shared.clone()).unwrap();
        assert!(manager.base_dir().exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(fresh.base_dir()), 0o700);
            assert_eq!(mode(manager.base_dir()), 0o755);
        }
    }

    #[test]
    fn test_atomic_write_leaves_preexisting_parent_mode_untouched() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("nested");
        fs::create_dir_all(&parent).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let target = parent.join("file.txt");
        atomic_write(&target, b"data").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"data");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
    }

    #[test]
    fn test_atomic_write_creates_and_overwrites() {
        let dir = tempdir().unwrap();
        let target_file = dir.path().join("nested").join("file.txt");

        atomic_write(&target_file, b"version 1").unwrap();
        assert_eq!(fs::read(&target_file).unwrap(), b"version 1");

        atomic_write(&target_file, b"version 2").unwrap();
        assert_eq!(fs::read(&target_file).unwrap(), b"version 2");
    }
}
