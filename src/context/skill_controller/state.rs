//! Ownership journal, link-object backups, and transaction state for skill projection.
//!
//! Enforces write-ahead durability and rollback safety before filesystem mutations (D4, D9, D14).

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const TRANSACTIONS_SUBDIR: &str = "transactions";
pub const BACKUPS_SUBDIR: &str = "backups";
const _: () = assert!(!TRANSACTIONS_SUBDIR.is_empty() && !BACKUPS_SUBDIR.is_empty());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[rustfmt::skip]
pub enum TransactionState { Pending, Committed, RolledBack }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[rustfmt::skip]
pub enum BackupKind {
    Symlink { target: PathBuf },
    OwnedCopy { hash: String, backup_path: Option<PathBuf> },
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[rustfmt::skip]
pub struct BackupRecord { pub target_path: PathBuf, pub kind: BackupKind, pub backed_up_at: u64 }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
#[rustfmt::skip]
pub enum JournalAction { LinkSymlink { source: PathBuf }, WriteCopy { source: PathBuf, hash: String }, Remove }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[rustfmt::skip]
pub struct JournalEntry { pub target_path: PathBuf, pub action: JournalAction, pub prior_state: BackupKind, pub owned_hash: Option<String> }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[rustfmt::skip]
pub struct TransactionRecord { pub id: String, pub state: TransactionState, pub created_at: u64, pub updated_at: u64, pub entries: Vec<JournalEntry> }

#[derive(Debug, Error)]
#[rustfmt::skip]
pub enum StateError {
    #[error("I/O error at {path}: {source}")] Io { path: PathBuf, source: io::Error },
    #[error("JSON serialization error: {0}")] Serialization(#[from] serde_json::Error),
    #[error("transaction '{0}' not found")] TransactionNotFound(String),
    #[error("unmanaged directory collision at {0}")] UnmanagedDirectory(PathBuf),
}

#[derive(Debug, Clone)]
#[rustfmt::skip]
pub struct Journal { base_dir: PathBuf }

impl Journal {
    #[rustfmt::skip]
    pub fn new(base_dir: PathBuf) -> Result<Self, StateError> {
        if let Err(e) = fs::create_dir_all(&base_dir) { return Err(StateError::Io { path: base_dir, source: e }); }
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let _ = fs::set_permissions(&base_dir, fs::Permissions::from_mode(0o700)); }
        Ok(Self { base_dir })
    }

    #[rustfmt::skip]
    pub fn from_settings() -> Result<Self, StateError> { Self::new(Self::default_dir()) }
    #[rustfmt::skip]
    pub fn default_dir() -> PathBuf { crate::settings::XavierSettings::resolve_data_dir().join("skill_controller") }
    #[rustfmt::skip]
    pub fn base_dir(&self) -> &Path { &self.base_dir }
    #[rustfmt::skip]
    pub fn transactions_dir(&self) -> PathBuf { self.base_dir.join(TRANSACTIONS_SUBDIR) }
    #[rustfmt::skip]
    pub fn backups_dir(&self, tx_id: &str) -> PathBuf { self.base_dir.join(BACKUPS_SUBDIR).join(tx_id) }
    #[rustfmt::skip]
    fn tx_path(&self, id: &str) -> PathBuf { self.transactions_dir().join(format!("{id}.json")) }

    #[rustfmt::skip]
    fn backup_file_path(&self, tx_id: &str, target_path: &Path) -> PathBuf {
        use sha2::{Digest, Sha256};
        let hash = crate::crypto::hex_encode(Sha256::digest(target_path.to_string_lossy().as_bytes()));
        let short = if hash.len() >= 16 { &hash[..16] } else { &hash };
        let name = target_path.file_name().and_then(|n| n.to_str()).unwrap_or("entry");
        self.backups_dir(tx_id).join(format!("{short}-{name}.json"))
    }

    #[rustfmt::skip]
    pub fn begin_transaction(&self, id: &str, entries: Vec<JournalEntry>, dry_run: bool) -> Result<TransactionRecord, StateError> {
        let now = current_timestamp();
        let tx = TransactionRecord { id: id.to_string(), state: TransactionState::Pending, created_at: now, updated_at: now, entries };
        if !dry_run { atomic_write(&self.tx_path(id), &serde_json::to_vec_pretty(&tx)?)?; }
        Ok(tx)
    }

    #[rustfmt::skip]
    pub fn record_entry(&self, tx_id: &str, entry: JournalEntry, dry_run: bool) -> Result<TransactionRecord, StateError> {
        if dry_run {
            let now = current_timestamp();
            return Ok(TransactionRecord { id: tx_id.to_string(), state: TransactionState::Pending, created_at: now, updated_at: now, entries: vec![entry] });
        }
        let mut tx = match self.get_transaction(tx_id)? {
            Some(t) => t,
            None => TransactionRecord { id: tx_id.to_string(), state: TransactionState::Pending, created_at: current_timestamp(), updated_at: current_timestamp(), entries: Vec::new() },
        };
        tx.entries.push(entry);
        tx.updated_at = current_timestamp();
        atomic_write(&self.tx_path(tx_id), &serde_json::to_vec_pretty(&tx)?)?;
        Ok(tx)
    }

    #[rustfmt::skip]
    pub fn commit_transaction(&self, id: &str, dry_run: bool) -> Result<TransactionRecord, StateError> { self.transition_state(id, TransactionState::Committed, dry_run) }
    #[rustfmt::skip]
    pub fn rollback_transaction(&self, id: &str, dry_run: bool) -> Result<TransactionRecord, StateError> { self.transition_state(id, TransactionState::RolledBack, dry_run) }

    #[rustfmt::skip]
    fn transition_state(&self, id: &str, state: TransactionState, dry_run: bool) -> Result<TransactionRecord, StateError> {
        let mut tx = match self.get_transaction(id)? {
            Some(t) => t,
            None => return Err(StateError::TransactionNotFound(id.to_string())),
        };
        tx.state = state;
        tx.updated_at = current_timestamp();
        if !dry_run { atomic_write(&self.tx_path(id), &serde_json::to_vec_pretty(&tx)?)?; }
        Ok(tx)
    }

    #[rustfmt::skip]
    pub fn get_transaction(&self, id: &str) -> Result<Option<TransactionRecord>, StateError> {
        let path = self.tx_path(id);
        if !path.exists() { return Ok(None); }
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(StateError::Io { path, source: e }),
        };
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    #[rustfmt::skip]
    pub fn list_transactions(&self) -> Result<Vec<TransactionRecord>, StateError> {
        let dir = self.transactions_dir();
        if !dir.exists() { return Ok(Vec::new()); }
        let read_dir = fs::read_dir(&dir).map_err(|e| StateError::Io { path: dir, source: e })?;
        let mut list = Vec::new();
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                if let Ok(bytes) = fs::read(&path) {
                    if let Ok(tx) = serde_json::from_slice::<TransactionRecord>(&bytes) { list.push(tx); }
                }
            }
        }
        list.sort_by_key(|a| a.created_at);
        Ok(list)
    }

    #[rustfmt::skip]
    pub fn pending_transactions(&self) -> Result<Vec<TransactionRecord>, StateError> {
        Ok(self.list_transactions()?.into_iter().filter(|t| t.state == TransactionState::Pending).collect())
    }
    #[rustfmt::skip]
    pub fn committed_transactions(&self) -> Result<Vec<TransactionRecord>, StateError> {
        Ok(self.list_transactions()?.into_iter().filter(|t| t.state == TransactionState::Committed).collect())
    }

    #[rustfmt::skip]
    pub fn backup_target(&self, tx_id: &str, target_path: &Path, dry_run: bool) -> Result<BackupKind, StateError> {
        let meta = match fs::symlink_metadata(target_path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BackupKind::Absent),
            Err(e) => return Err(StateError::Io { path: target_path.to_path_buf(), source: e }),
        };
        if meta.file_type().is_symlink() {
            let target = fs::read_link(target_path).map_err(|e| StateError::Io { path: target_path.to_path_buf(), source: e })?;
            let kind = BackupKind::Symlink { target };
            if !dry_run {
                let record = BackupRecord { target_path: target_path.to_path_buf(), kind: kind.clone(), backed_up_at: current_timestamp() };
                atomic_write(&self.backup_file_path(tx_id, target_path), &serde_json::to_vec_pretty(&record)?)?;
            }
            return Ok(kind);
        }
        if meta.is_file() {
            let content = fs::read(target_path).map_err(|e| StateError::Io { path: target_path.to_path_buf(), source: e })?;
            use sha2::{Digest, Sha256};
            let hash = crate::crypto::hex_encode(Sha256::digest(&content));
            let backup_path = if !dry_run {
                let backup_file = self.backup_file_path(tx_id, target_path);
                let content_path = backup_file.with_extension("data");
                atomic_write(&content_path, &content)?;
                let record = BackupRecord { target_path: target_path.to_path_buf(), kind: BackupKind::OwnedCopy { hash: hash.clone(), backup_path: Some(content_path.clone()) }, backed_up_at: current_timestamp() };
                atomic_write(&backup_file, &serde_json::to_vec_pretty(&record)?)?;
                Some(content_path)
            } else { None };
            return Ok(BackupKind::OwnedCopy { hash, backup_path });
        }
        if meta.is_dir() { return Err(StateError::UnmanagedDirectory(target_path.to_path_buf())); }
        Ok(BackupKind::Absent)
    }

    #[rustfmt::skip]
    pub fn read_backup(&self, tx_id: &str, target_path: &Path) -> Result<Option<BackupRecord>, StateError> {
        let path = self.backup_file_path(tx_id, target_path);
        if !path.exists() { return Ok(None); }
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(StateError::Io { path, source: e }),
        };
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    #[rustfmt::skip]
    pub fn apply_with_journal<F>(&self, tx_id: &str, target_path: &Path, action: JournalAction, owned_hash: Option<String>, dry_run: bool, mutate_fn: F) -> Result<(), StateError>
    where
        F: FnOnce(&Path) -> Result<(), StateError>,
    {
        if dry_run { return Ok(()); }
        let prior_state = self.backup_target(tx_id, target_path, false)?;
        self.record_entry(tx_id, JournalEntry { target_path: target_path.to_path_buf(), action, prior_state, owned_hash }, false)?;
        mutate_fn(target_path)
    }
}

#[rustfmt::skip]
fn current_timestamp() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => 0,
    }
}

#[rustfmt::skip]
fn atomic_write(path: &Path, content: &[u8]) -> Result<(), StateError> {
    let parent = match path.parent() {
        Some(p) => p,
        None => return Err(StateError::Io { path: path.to_path_buf(), source: io::Error::new(io::ErrorKind::InvalidInput, "no parent") }),
    };
    if let Err(e) = fs::create_dir_all(parent) { return Err(StateError::Io { path: parent.to_path_buf(), source: e }); }
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700)); }
    let rand_id = uuid::Uuid::new_v4().simple();
    let tmp = parent.join(format!(".tmp-{}-{}", current_timestamp(), rand_id));
    let mut file = match fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
        Ok(f) => f,
        Err(e) => return Err(StateError::Io { path: tmp, source: e }),
    };
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600)); }
    if let Err(e) = file.write_all(content) { let _ = fs::remove_file(&tmp); return Err(StateError::Io { path: tmp, source: e }); }
    if let Err(e) = file.sync_all() { let _ = fs::remove_file(&tmp); return Err(StateError::Io { path: tmp, source: e }); }
    drop(file);
    if let Err(e) = fs::rename(&tmp, path) { let _ = fs::remove_file(&tmp); return Err(StateError::Io { path: path.to_path_buf(), source: e }); }
    #[cfg(unix)] { if let Ok(dir) = fs::File::open(parent) { let _ = dir.sync_all(); } }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    #[rustfmt::skip]
    fn test_round_trip_pending_and_committed_transactions() {
        let dir = tempdir().unwrap();
        let journal = Journal::new(dir.path().join("state")).unwrap();
        let tx_id = "tx-test-01";
        let entry = JournalEntry {
            target_path: PathBuf::from("/tmp/test/target-skill"),
            action: JournalAction::LinkSymlink { source: PathBuf::from("/tmp/test/source-skill") },
            prior_state: BackupKind::Absent,
            owned_hash: None,
        };
        let tx = journal.begin_transaction(tx_id, vec![entry.clone()], false).unwrap();
        assert_eq!(tx.state, TransactionState::Pending);
        let recovered = journal.get_transaction(tx_id).unwrap().expect("transaction exists");
        assert_eq!(recovered.id, tx_id);
        assert_eq!(recovered.state, TransactionState::Pending);
        assert_eq!(recovered.entries, vec![entry]);
        assert_eq!(journal.pending_transactions().unwrap().len(), 1);
        assert!(journal.committed_transactions().unwrap().is_empty());

        let committed = journal.commit_transaction(tx_id, false).unwrap();
        assert_eq!(committed.state, TransactionState::Committed);
        assert_eq!(journal.get_transaction(tx_id).unwrap().unwrap().state, TransactionState::Committed);
        assert!(journal.pending_transactions().unwrap().is_empty());
        assert_eq!(journal.committed_transactions().unwrap().len(), 1);
    }

    #[test]
    #[rustfmt::skip]
    fn test_symlink_backup_preserves_readlink_without_dereferencing() {
        let dir = tempdir().unwrap();
        let journal = Journal::new(dir.path().join("state")).unwrap();
        let target_dir = dir.path().join("real_skill_dir");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("SKILL.md"), "name: test-skill").unwrap();
        fs::write(target_dir.join("extra.txt"), "payload").unwrap();

        let symlink_path = dir.path().join("symlink_skill");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_dir, &symlink_path).unwrap();

        let tx_id = "tx-symlink-01";
        let backup = journal.backup_target(tx_id, &symlink_path, false).unwrap();
        match backup {
            BackupKind::Symlink { target } => {
                assert_eq!(target, target_dir);
                assert_eq!(target, fs::read_link(&symlink_path).unwrap());
            }
            _ => panic!("expected symlink backup kind"),
        }
        let record = journal.read_backup(tx_id, &symlink_path).unwrap().expect("backup record exists");
        match record.kind {
            BackupKind::Symlink { target } => assert_eq!(target, target_dir),
            _ => panic!("expected symlink backup in record"),
        }
        assert!(!journal.backups_dir(tx_id).join("SKILL.md").exists());
        assert!(!journal.backups_dir(tx_id).join("extra.txt").exists());
    }

    #[test]
    #[rustfmt::skip]
    fn test_journal_failure_prevents_target_mutation() {
        let dir = tempdir().unwrap();
        let target_path = dir.path().join("dest_skill");
        let journal_base = dir.path().join("state");
        fs::create_dir_all(&journal_base).unwrap();
        fs::write(journal_base.join(TRANSACTIONS_SUBDIR), b"blocking-file").unwrap();
        let journal = Journal::new(journal_base).unwrap();
        let mut mutation_executed = false;
        let result = journal.apply_with_journal(
            "tx-fail-01", &target_path, JournalAction::LinkSymlink { source: PathBuf::from("/nonexistent") }, None, false,
            |_| { mutation_executed = true; fs::write(&target_path, b"mutated").map_err(|e| StateError::Io { path: target_path.clone(), source: e }) },
        );
        assert!(result.is_err());
        assert!(!mutation_executed);
        assert!(!target_path.exists());
    }

    #[test]
    #[rustfmt::skip]
    fn test_backup_failure_prevents_target_mutation() {
        let dir = tempdir().unwrap();
        let target_path = dir.path().join("existing_skill");
        fs::write(&target_path, b"original-content").unwrap();
        let journal_base = dir.path().join("state");
        fs::create_dir_all(&journal_base).unwrap();
        let backups_parent = journal_base.join(BACKUPS_SUBDIR);
        fs::create_dir_all(&backups_parent).unwrap();
        fs::write(backups_parent.join("tx-backup-fail"), b"blocking-file").unwrap();
        let journal = Journal::new(journal_base).unwrap();
        let mut mutation_executed = false;
        let result = journal.apply_with_journal(
            "tx-backup-fail", &target_path, JournalAction::LinkSymlink { source: PathBuf::from("/nonexistent") }, None, false,
            |_| { mutation_executed = true; fs::write(&target_path, b"overwritten").map_err(|e| StateError::Io { path: target_path.clone(), source: e }) },
        );
        assert!(result.is_err());
        assert!(!mutation_executed);
        assert_eq!(fs::read(&target_path).unwrap(), b"original-content");
    }

    #[test]
    #[rustfmt::skip]
    fn test_dry_run_writes_nothing() {
        let dir = tempdir().unwrap();
        let journal = Journal::new(dir.path().join("state")).unwrap();
        let target_path = dir.path().join("dry_skill");
        let mut mutation_called = false;
        let tx = journal.begin_transaction(
            "tx-dry-01",
            vec![JournalEntry { target_path: target_path.clone(), action: JournalAction::Remove, prior_state: BackupKind::Absent, owned_hash: None }],
            true,
        ).unwrap();
        assert_eq!(tx.state, TransactionState::Pending);
        assert!(!journal.transactions_dir().exists());
        journal.apply_with_journal("tx-dry-01", &target_path, JournalAction::Remove, None, true, |_| { mutation_called = true; Ok(()) }).unwrap();
        assert!(!mutation_called);
        assert!(!journal.transactions_dir().exists());
    }

    #[tokio::test]
    async fn test_journal_env_guard_settings() {
        let _guard = crate::context::skill_registry::env_lock().lock().await;
        let default_dir = Journal::default_dir();
        assert!(default_dir.ends_with("skill_controller"));
    }
}
