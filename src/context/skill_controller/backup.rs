//! Link-object backups and atomic file persistence for skill controller.
//!
//! Provides write-ahead backup state and crash-safe file persistence.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const BACKUPS_SUBDIR: &str = "backups";
const _: () = assert!(!BACKUPS_SUBDIR.is_empty());

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
}

#[derive(Debug, Clone)]
pub struct BackupManager {
    base_dir: PathBuf,
}

impl BackupManager {
    pub fn new(base_dir: PathBuf) -> Result<Self, BackupError> {
        if let Err(source) = fs::create_dir_all(&base_dir) {
            return Err(BackupError::Io {
                path: base_dir,
                source,
            });
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&base_dir, fs::Permissions::from_mode(0o700));
        }
        Ok(Self { base_dir })
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn backups_dir(&self, tx_id: &str) -> PathBuf {
        self.base_dir.join(BACKUPS_SUBDIR).join(tx_id)
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
    if let Err(source) = fs::create_dir_all(parent) {
        return Err(BackupError::Io {
            path: parent.to_path_buf(),
            source,
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
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
    fn test_atomic_write_creates_and_overwrites() {
        let dir = tempdir().unwrap();
        let target_file = dir.path().join("nested").join("file.txt");

        atomic_write(&target_file, b"version 1").unwrap();
        assert_eq!(fs::read(&target_file).unwrap(), b"version 1");

        atomic_write(&target_file, b"version 2").unwrap();
        assert_eq!(fs::read(&target_file).unwrap(), b"version 2");
    }
}
