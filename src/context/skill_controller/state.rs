//! Ownership journal and write-ahead transaction state for skill projection.
//!
//! Enforces write-ahead durability and rollback safety before filesystem mutations.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const TRANSACTIONS_SUBDIR: &str = "transactions";
const _: () = assert!(!TRANSACTIONS_SUBDIR.is_empty());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionState {
    Pending,
    Committed,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum JournalAction {
    LinkSymlink { source: PathBuf },
    WriteCopy { source: PathBuf, hash: String },
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub target_path: PathBuf,
    pub action: JournalAction,
    pub prior_state: super::backup::BackupKind,
    pub owned_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub id: String,
    pub state: TransactionState,
    pub created_at: u64,
    pub updated_at: u64,
    pub entries: Vec<JournalEntry>,
}

#[derive(Debug, Error)]
pub enum StateError {
    #[error("I/O error at {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("transaction '{0}' not found")]
    TransactionNotFound(String),
    #[error("unmanaged directory collision at {0}")]
    UnmanagedDirectory(PathBuf),
    #[error("backup error: {0}")]
    Backup(#[from] super::backup::BackupError),
}

#[derive(Debug, Clone)]
pub struct Journal {
    base_dir: PathBuf,
    backup_mgr: super::backup::BackupManager,
}

impl Journal {
    pub fn new(base_dir: PathBuf) -> Result<Self, StateError> {
        let backup_mgr = super::backup::BackupManager::new(base_dir.clone())?;
        Ok(Self {
            base_dir,
            backup_mgr,
        })
    }

    pub fn from_settings() -> Result<Self, StateError> {
        Self::new(Self::default_dir())
    }

    pub fn default_dir() -> PathBuf {
        crate::settings::XavierSettings::resolve_data_dir().join("skill_controller")
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn transactions_dir(&self) -> PathBuf {
        self.base_dir.join(TRANSACTIONS_SUBDIR)
    }

    pub fn backups_dir(&self, tx_id: &str) -> PathBuf {
        self.backup_mgr.backups_dir(tx_id)
    }

    fn tx_path(&self, id: &str) -> PathBuf {
        self.transactions_dir().join(format!("{id}.json"))
    }

    pub fn backup_target(
        &self,
        tx_id: &str,
        target_path: &Path,
        dry_run: bool,
    ) -> Result<super::backup::BackupKind, StateError> {
        self.backup_mgr
            .backup_target(tx_id, target_path, dry_run)
            .map_err(StateError::from)
    }

    pub fn read_backup(
        &self,
        tx_id: &str,
        target_path: &Path,
    ) -> Result<Option<super::backup::BackupRecord>, StateError> {
        self.backup_mgr
            .read_backup(tx_id, target_path)
            .map_err(StateError::from)
    }

    pub fn begin_transaction(
        &self,
        id: &str,
        entries: Vec<JournalEntry>,
        dry_run: bool,
    ) -> Result<TransactionRecord, StateError> {
        let now = super::backup::current_timestamp();
        let tx = TransactionRecord {
            id: id.to_string(),
            state: TransactionState::Pending,
            created_at: now,
            updated_at: now,
            entries,
        };
        if !dry_run {
            let bytes = serde_json::to_vec_pretty(&tx)?;
            super::backup::atomic_write(&self.tx_path(id), &bytes)?;
        }
        Ok(tx)
    }

    pub fn record_entry(
        &self,
        tx_id: &str,
        entry: JournalEntry,
        dry_run: bool,
    ) -> Result<TransactionRecord, StateError> {
        if dry_run {
            let now = super::backup::current_timestamp();
            return Ok(TransactionRecord {
                id: tx_id.to_string(),
                state: TransactionState::Pending,
                created_at: now,
                updated_at: now,
                entries: vec![entry],
            });
        }
        let now = super::backup::current_timestamp();
        let mut tx = match self.get_transaction(tx_id)? {
            Some(t) => t,
            None => TransactionRecord {
                id: tx_id.to_string(),
                state: TransactionState::Pending,
                created_at: now,
                updated_at: now,
                entries: Vec::new(),
            },
        };
        tx.entries.push(entry);
        tx.updated_at = now;
        let bytes = serde_json::to_vec_pretty(&tx)?;
        super::backup::atomic_write(&self.tx_path(tx_id), &bytes)?;
        Ok(tx)
    }

    pub fn commit_transaction(
        &self,
        id: &str,
        dry_run: bool,
    ) -> Result<TransactionRecord, StateError> {
        self.transition_state(id, TransactionState::Committed, dry_run)
    }

    pub fn rollback_transaction(
        &self,
        id: &str,
        dry_run: bool,
    ) -> Result<TransactionRecord, StateError> {
        self.transition_state(id, TransactionState::RolledBack, dry_run)
    }

    fn transition_state(
        &self,
        id: &str,
        state: TransactionState,
        dry_run: bool,
    ) -> Result<TransactionRecord, StateError> {
        let mut tx = match self.get_transaction(id)? {
            Some(t) => t,
            None => return Err(StateError::TransactionNotFound(id.to_string())),
        };
        tx.state = state;
        tx.updated_at = super::backup::current_timestamp();
        if !dry_run {
            let bytes = serde_json::to_vec_pretty(&tx)?;
            super::backup::atomic_write(&self.tx_path(id), &bytes)?;
        }
        Ok(tx)
    }

    pub fn get_transaction(&self, id: &str) -> Result<Option<TransactionRecord>, StateError> {
        let path = self.tx_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(StateError::Io { path, source }),
        };
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    pub fn list_transactions(&self) -> Result<Vec<TransactionRecord>, StateError> {
        let dir = self.transactions_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let read_dir = fs::read_dir(&dir).map_err(|source| StateError::Io { path: dir, source })?;
        let mut list = Vec::new();
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                if let Ok(bytes) = fs::read(&path) {
                    if let Ok(tx) = serde_json::from_slice::<TransactionRecord>(&bytes) {
                        list.push(tx);
                    }
                }
            }
        }
        list.sort_by_key(|a| a.created_at);
        Ok(list)
    }

    pub fn pending_transactions(&self) -> Result<Vec<TransactionRecord>, StateError> {
        Ok(self
            .list_transactions()?
            .into_iter()
            .filter(|t| t.state == TransactionState::Pending)
            .collect())
    }

    pub fn committed_transactions(&self) -> Result<Vec<TransactionRecord>, StateError> {
        Ok(self
            .list_transactions()?
            .into_iter()
            .filter(|t| t.state == TransactionState::Committed)
            .collect())
    }

    pub fn apply_with_journal<F>(
        &self,
        tx_id: &str,
        target_path: &Path,
        action: JournalAction,
        owned_hash: Option<String>,
        dry_run: bool,
        mutate_fn: F,
    ) -> Result<(), StateError>
    where
        F: FnOnce(&Path) -> Result<(), StateError>,
    {
        if dry_run {
            return Ok(());
        }
        let prior_state = self.backup_target(tx_id, target_path, false)?;
        self.record_entry(
            tx_id,
            JournalEntry {
                target_path: target_path.to_path_buf(),
                action,
                prior_state,
                owned_hash,
            },
            false,
        )?;
        mutate_fn(target_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_round_trip_pending_and_committed_transactions() {
        let dir = tempdir().unwrap();
        let journal = Journal::new(dir.path().join("state")).unwrap();
        let tx_id = "tx-test-01";
        let entry = JournalEntry {
            target_path: PathBuf::from("/tmp/test/target-skill"),
            action: JournalAction::LinkSymlink {
                source: PathBuf::from("/tmp/test/source-skill"),
            },
            prior_state: super::super::backup::BackupKind::Absent,
            owned_hash: None,
        };
        let tx = journal
            .begin_transaction(tx_id, vec![entry.clone()], false)
            .unwrap();
        assert_eq!(tx.state, TransactionState::Pending);

        let recovered = journal
            .get_transaction(tx_id)
            .unwrap()
            .expect("transaction exists");
        assert_eq!(recovered.id, tx_id);
        assert_eq!(recovered.state, TransactionState::Pending);
        assert_eq!(recovered.entries, vec![entry]);
        assert_eq!(journal.pending_transactions().unwrap().len(), 1);
        assert!(journal.committed_transactions().unwrap().is_empty());

        let committed = journal.commit_transaction(tx_id, false).unwrap();
        assert_eq!(committed.state, TransactionState::Committed);
        assert_eq!(
            journal.get_transaction(tx_id).unwrap().unwrap().state,
            TransactionState::Committed
        );
        assert!(journal.pending_transactions().unwrap().is_empty());
        assert_eq!(journal.committed_transactions().unwrap().len(), 1);
    }

    #[test]
    fn test_rollback_transaction_transition() {
        let dir = tempdir().unwrap();
        let journal = Journal::new(dir.path().join("state")).unwrap();
        let tx_id = "tx-rollback-01";
        journal.begin_transaction(tx_id, Vec::new(), false).unwrap();
        let rolled_back = journal.rollback_transaction(tx_id, false).unwrap();
        assert_eq!(rolled_back.state, TransactionState::RolledBack);
        let recovered = journal.get_transaction(tx_id).unwrap().unwrap();
        assert_eq!(recovered.state, TransactionState::RolledBack);
    }

    #[test]
    fn test_journal_failure_prevents_target_mutation() {
        let dir = tempdir().unwrap();
        let target_path = dir.path().join("dest_skill");
        let journal_base = dir.path().join("state");
        fs::create_dir_all(&journal_base).unwrap();
        fs::write(journal_base.join(TRANSACTIONS_SUBDIR), b"blocking-file").unwrap();
        let journal = Journal::new(journal_base).unwrap();
        let mut mutation_executed = false;
        let result = journal.apply_with_journal(
            "tx-fail-01",
            &target_path,
            JournalAction::LinkSymlink {
                source: PathBuf::from("/nonexistent"),
            },
            None,
            false,
            |_| {
                mutation_executed = true;
                fs::write(&target_path, b"mutated").map_err(|e| StateError::Io {
                    path: target_path.clone(),
                    source: e,
                })
            },
        );
        assert!(result.is_err());
        assert!(!mutation_executed);
        assert!(!target_path.exists());
    }

    #[test]
    fn test_backup_failure_prevents_target_mutation() {
        let dir = tempdir().unwrap();
        let target_path = dir.path().join("existing_skill");
        fs::write(&target_path, b"original-content").unwrap();
        let journal_base = dir.path().join("state");
        let journal = Journal::new(journal_base).unwrap();

        let tx_id = "tx-backup-fail";
        let bdir = journal.backups_dir(tx_id);
        fs::create_dir_all(bdir.parent().unwrap()).unwrap();
        fs::write(&bdir, b"blocking-file").unwrap();

        let mut mutation_executed = false;
        let result = journal.apply_with_journal(
            tx_id,
            &target_path,
            JournalAction::LinkSymlink {
                source: PathBuf::from("/nonexistent"),
            },
            None,
            false,
            |_| {
                mutation_executed = true;
                fs::write(&target_path, b"overwritten").map_err(|e| StateError::Io {
                    path: target_path.clone(),
                    source: e,
                })
            },
        );
        assert!(result.is_err());
        assert!(!mutation_executed);
        assert_eq!(fs::read(&target_path).unwrap(), b"original-content");
    }

    #[test]
    fn test_dry_run_writes_nothing() {
        let dir = tempdir().unwrap();
        let journal = Journal::new(dir.path().join("state")).unwrap();
        let target_path = dir.path().join("dry_skill");
        let mut mutation_called = false;
        let tx = journal
            .begin_transaction(
                "tx-dry-01",
                vec![JournalEntry {
                    target_path: target_path.clone(),
                    action: JournalAction::Remove,
                    prior_state: super::super::backup::BackupKind::Absent,
                    owned_hash: None,
                }],
                true,
            )
            .unwrap();
        assert_eq!(tx.state, TransactionState::Pending);
        assert!(!journal.transactions_dir().exists());
        journal
            .apply_with_journal(
                "tx-dry-01",
                &target_path,
                JournalAction::Remove,
                None,
                true,
                |_| {
                    mutation_called = true;
                    Ok(())
                },
            )
            .unwrap();
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
