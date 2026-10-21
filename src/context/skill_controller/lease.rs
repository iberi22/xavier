//! Task-scoped publication and leases; callers run filesystem work off Tokio workers.

use super::backup::{atomic_write, current_timestamp, BackupError, BackupKind};
use super::policy::{FsPolicy, PolicyError, RootKind};
use super::projector::{ProjectorError, RootLock, LOCKS_SUBDIR};
use super::rollback::{rollback_transaction, RollbackError, RollbackReport};
use super::state::{Journal, JournalAction, JournalEntry, StateError, TransactionRecord};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const LEASE_SECONDS: u64 = 300;
pub type ReleaseReport = RollbackReport;

#[derive(Debug, thiserror::Error)]
pub enum LeaseError {
    #[error("invalid task id")]
    InvalidTaskId,
    #[error("task skill is {tokens} tokens, over the {max} token budget")]
    TokenBudget { tokens: usize, max: usize },
    #[error("task skill destination is already occupied or ownership is invalid")]
    Conflict,
    #[error(transparent)]
    State(#[from] StateError),
    #[error(transparent)]
    Rollback(#[from] RollbackError),
    #[error(transparent)]
    Backup(#[from] BackupError),
    #[error(transparent)]
    Policy(#[from] PolicyError),
    #[error(transparent)]
    Lock(#[from] ProjectorError),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug)]
pub struct PublishedSkill {
    pub tx_id: String,
    pub skill_path: PathBuf,
    pub link_path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Lease {
    tx_id: String,
    expires_at: u64,
}

fn paths(worktree: &Path, task_id: &str) -> Result<(FsPolicy, PathBuf, PathBuf), LeaseError> {
    if task_id.is_empty()
        || task_id.len() > 64
        || !task_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(LeaseError::InvalidTaskId);
    }
    let root = fs::canonicalize(worktree)?;
    let policy = FsPolicy::new(&[], std::slice::from_ref(&root))?;
    let dir = root.join(format!(".agents/skills/eph-{task_id}"));
    let link = root.join(format!(".claude/skills/eph-{task_id}"));
    for path in [&dir, &link] {
        let mut parent = path.parent().ok_or(LeaseError::Conflict)?;
        while !parent.try_exists()? {
            parent = parent.parent().ok_or(LeaseError::Conflict)?;
        }
        policy.authorize(RootKind::Target, parent)?;
    }
    Ok((policy, dir, link))
}

// Called only on the fixed two-component discovery paths below an existing root.
fn mkdir(policy: &FsPolicy, path: &Path) -> Result<(), LeaseError> {
    if !path.try_exists()? {
        let parent = path.parent().ok_or(LeaseError::Conflict)?;
        mkdir(policy, parent)?;
        policy.authorize(RootKind::Target, parent)?.revalidate()?;
        fs::create_dir(path)?;
    }
    policy.authorize(RootKind::Target, path)?.revalidate()?;
    Ok(())
}

fn vacant(path: &Path) -> Result<(), LeaseError> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
        Ok(_) => Err(LeaseError::Conflict),
    }
}

fn lock(journal: &Journal, worktree: &Path) -> Result<RootLock, LeaseError> {
    Ok(RootLock::acquire(
        &journal.base_dir().join(LOCKS_SUBDIR),
        &fs::canonicalize(worktree)?,
    )?)
}

/// Publication bounded by the ephemeral token budget; nothing is written when exceeded.
pub fn publish_task_skill_bounded(
    journal: &Journal,
    worktree: &Path,
    task_id: &str,
    content: &str,
    max_tokens: usize,
) -> Result<PublishedSkill, LeaseError> {
    let tokens = crate::context::token_estimate::estimate_tokens(content);
    if tokens > max_tokens {
        return Err(LeaseError::TokenBudget {
            tokens,
            max: max_tokens,
        });
    }
    publish_task_skill(journal, worktree, task_id, content)
}

/// Explicit publication: refuses all pre-existing task directories and link objects.
pub fn publish_task_skill(
    journal: &Journal,
    worktree: &Path,
    task_id: &str,
    content: &str,
) -> Result<PublishedSkill, LeaseError> {
    let (policy, dir, link) = paths(worktree, task_id)?;
    if content.len() as u64 > super::backup::MAX_BACKUP_BYTES {
        return Err(LeaseError::Conflict);
    }
    let findings =
        super::policy::scan_text(content, format!(".agents/skills/eph-{task_id}/SKILL.md"))?;
    if !findings.is_empty() {
        let summary = findings
            .iter()
            .map(|f| format!("{} at {}", f.class, f.location))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(LeaseError::Policy(PolicyError::SecretsBlocked { summary }));
    }
    let _lock = lock(journal, worktree)?;
    vacant(&dir)?;
    vacant(&link)?;
    mkdir(&policy, dir.parent().ok_or(LeaseError::Conflict)?)?;
    mkdir(&policy, link.parent().ok_or(LeaseError::Conflict)?)?;
    policy
        .authorize(RootKind::Target, dir.parent().ok_or(LeaseError::Conflict)?)?
        .revalidate()?;
    fs::create_dir(&dir)?;
    let tx_id = format!("lease-{}", uuid::Uuid::new_v4().simple());
    let skill = dir.join("SKILL.md");
    let result = (|| {
        policy.authorize(RootKind::Target, &dir)?.revalidate()?;
        journal.begin_transaction(&tx_id, Vec::new(), false)?;
        let lease = Lease {
            tx_id: tx_id.clone(),
            expires_at: current_timestamp().saturating_add(LEASE_SECONDS),
        };
        atomic_write(&dir.join(".lease"), &serde_json::to_vec(&lease)?)?;
        let hash = crate::crypto::hex_encode(Sha256::digest(content.as_bytes()));
        journal.apply_with_journal(
            &tx_id,
            &skill,
            JournalAction::WriteCopy {
                source: skill.clone(),
                hash: hash.clone(),
            },
            Some(hash),
            false,
            |path| atomic_write(path, content.as_bytes()).map_err(StateError::from),
        )?;
        policy
            .authorize(RootKind::Target, link.parent().ok_or(LeaseError::Conflict)?)?
            .revalidate()?;
        journal.apply_with_journal(
            &tx_id,
            &link,
            JournalAction::LinkSymlink {
                source: dir.clone(),
            },
            None,
            false,
            |path| {
                #[cfg(unix)]
                let result = std::os::unix::fs::symlink(&dir, path);
                #[cfg(windows)]
                let result = std::os::windows::fs::symlink_dir(&dir, path);
                result.map_err(|source| StateError::Io {
                    path: path.to_path_buf(),
                    source,
                })
            },
        )?;
        if !skill.is_file() || fs::read_link(&link)? != dir {
            return Err(LeaseError::Conflict);
        }
        journal.commit_transaction(&tx_id, false)?;
        Ok(PublishedSkill {
            tx_id: tx_id.clone(),
            skill_path: skill,
            link_path: link,
        })
    })();
    if result.is_err() {
        let _ = rollback_transaction(journal, &policy, &tx_id, false);
        let _ = fs::remove_file(dir.join(".lease"));
        let _ = fs::remove_dir(&dir);
    }
    result
}

fn read_lease(
    journal: &Journal,
    policy: &FsPolicy,
    dir: &Path,
    link: &Path,
) -> Result<Lease, LeaseError> {
    policy.authorize(RootKind::Target, dir)?.revalidate()?;
    let path = dir.join(".lease");
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.len() > 4096 {
        return Err(LeaseError::Conflict);
    }
    let lease: Lease =
        serde_json::from_slice(&fs::read(path)?).map_err(|_| LeaseError::Conflict)?;
    // The marker cannot redirect rollback to another task or an arbitrary transaction.
    if !lease.tx_id.starts_with("lease-")
        || !lease
            .tx_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(LeaseError::Conflict);
    }
    let tx = journal
        .get_transaction(&lease.tx_id)?
        .ok_or(LeaseError::Conflict)?;
    let skill = dir.join("SKILL.md");
    if tx.entries.len() != 2 || tx.entries.iter().any(|e| e.prior_state != BackupKind::Absent)
        || !tx.entries.iter().any(|e| e.target_path == skill && matches!(&e.action, JournalAction::WriteCopy { source, hash } if source == &skill && e.owned_hash.as_ref() == Some(hash)))
        || !tx.entries.iter().any(|e| e.target_path == link && matches!(&e.action, JournalAction::LinkSymlink { source } if source == dir))
    {
        return Err(LeaseError::Conflict);
    }
    Ok(lease)
}

pub fn renew_lease(journal: &Journal, worktree: &Path, task_id: &str) -> Result<(), LeaseError> {
    let (policy, dir, link) = paths(worktree, task_id)?;
    let _lock = lock(journal, worktree)?;
    let mut lease = read_lease(journal, &policy, &dir, &link)?;
    lease.expires_at = lease
        .expires_at
        .max(current_timestamp().saturating_add(LEASE_SECONDS));
    atomic_write(&dir.join(".lease"), &serde_json::to_vec(&lease)?)?;
    Ok(())
}

pub fn release_task(
    journal: &Journal,
    worktree: &Path,
    task_id: &str,
) -> Result<ReleaseReport, LeaseError> {
    let (policy, dir, link) = paths(worktree, task_id)?;
    let _lock = lock(journal, worktree)?;
    let lease = read_lease(journal, &policy, &dir, &link)?;
    release_owned(journal, &policy, &dir, &lease)
}

fn release_owned(
    journal: &Journal,
    policy: &FsPolicy,
    dir: &Path,
    lease: &Lease,
) -> Result<ReleaseReport, LeaseError> {
    let report = rollback_transaction(journal, policy, &lease.tx_id, false)?;
    policy.authorize(RootKind::Target, dir)?.revalidate()?;
    fs::remove_file(dir.join(".lease"))?;
    match fs::remove_dir(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::DirectoryNotEmpty => {}
        Err(e) => return Err(e.into()),
    }
    Ok(report)
}

pub fn cleanup_stale(
    journal: &Journal,
    worktree: &Path,
    now: u64,
    is_active: impl Fn(&str) -> bool,
) -> Result<Vec<ReleaseReport>, LeaseError> {
    let (policy, dir, _) = paths(worktree, "probe")?;
    let parent = dir.parent().ok_or(LeaseError::Conflict)?;
    if !parent.try_exists()? {
        return Ok(Vec::new());
    }
    policy.authorize(RootKind::Target, parent)?;
    let mut reports = Vec::new();
    for entry in fs::read_dir(parent)?.take(1024) {
        let entry = entry?;
        let name = entry.file_name();
        let Some(task_id) = name.to_str().and_then(|name| name.strip_prefix("eph-")) else {
            continue;
        };
        let Ok((policy, dir, link)) = paths(worktree, task_id) else {
            continue;
        };
        // Read and recheck under the same lock used by renewal; do not race a heartbeat.
        let _lock = lock(journal, worktree)?;
        let Ok(lease) = read_lease(journal, &policy, &dir, &link) else {
            continue;
        };
        if lease.expires_at <= now && !is_active(task_id) {
            reports.push(release_owned(journal, &policy, &dir, &lease)?);
        }
    }
    Ok(reports)
}

pub fn list_stale(
    journal: &Journal,
    worktree: &Path,
    now: u64,
    is_active: impl Fn(&str) -> bool,
) -> Result<Vec<String>, LeaseError> {
    let (policy, dir, _) = paths(worktree, "probe")?;
    let parent = dir.parent().ok_or(LeaseError::Conflict)?;
    if !parent.try_exists()? {
        return Ok(Vec::new());
    }
    policy.authorize(RootKind::Target, parent)?;
    let mut stale = Vec::new();
    for entry in fs::read_dir(parent)?.take(1024) {
        let entry = entry?;
        let name = entry.file_name();
        let Some(task_id) = name.to_str().and_then(|name| name.strip_prefix("eph-")) else {
            continue;
        };
        let Ok((policy, dir, link)) = paths(worktree, task_id) else {
            continue;
        };
        let _lock = lock(journal, worktree)?;
        let Ok(lease) = read_lease(journal, &policy, &dir, &link) else {
            continue;
        };
        if lease.expires_at <= now && !is_active(task_id) {
            stale.push(task_id.to_string());
        }
    }
    stale.sort();
    Ok(stale)
}

/// True when a pending journal transaction targets the task's own `eph-{task_id}`
/// path component (an exact component match, so `eph-task2` never marks `task`).
pub fn task_has_pending_tx(pending: &[TransactionRecord], task_id: &str) -> bool {
    let needle = format!("eph-{task_id}");
    pending.iter().any(|tx| {
        tx.entries.iter().any(|entry| {
            entry
                .target_path
                .components()
                .any(|c| c.as_os_str() == needle.as_str())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::super::rollback::RollbackOutcome;
    use super::*;
    use tempfile::{tempdir, TempDir};

    fn fixture() -> (TempDir, TempDir, Journal) {
        let worktree = tempdir().unwrap();
        let state = tempdir().unwrap();
        let journal = Journal::new(state.path().join("journal")).unwrap();
        (worktree, state, journal)
    }

    #[test]
    fn publish_creates_file_link_and_one_committed_transaction() {
        let (w, _s, j) = fixture();
        let out = publish_task_skill(&j, w.path(), "task_1", "skill body").unwrap();
        assert_eq!(fs::read_to_string(&out.skill_path).unwrap(), "skill body");
        assert_eq!(
            fs::read_link(&out.link_path).unwrap(),
            out.skill_path.parent().unwrap()
        );
        let transactions = j.committed_transactions().unwrap();
        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0].id, out.tx_id);
        assert_eq!(transactions[0].entries.len(), 2);
        assert!(out.skill_path.starts_with(w.path()));
        assert!(out.link_path.starts_with(w.path()));
    }

    #[test]
    fn bounded_publish_rejects_over_budget_without_writes() {
        let (w, _s, j) = fixture();
        let err =
            publish_task_skill_bounded(&j, w.path(), "task", &"word ".repeat(400), 10).unwrap_err();
        assert!(matches!(err, LeaseError::TokenBudget { max: 10, .. }));
        assert_eq!(fs::read_dir(w.path()).unwrap().count(), 0);
        assert!(j.list_transactions().unwrap().is_empty());
        publish_task_skill_bounded(&j, w.path(), "task", "body", 10).unwrap();
    }

    #[test]
    fn release_removes_unchanged_entries_and_owned_directory() {
        let (w, _s, j) = fixture();
        let out = publish_task_skill(&j, w.path(), "task", "body").unwrap();
        let report = release_task(&j, w.path(), "task").unwrap();
        assert_eq!(report.entries.len(), 2);
        assert!(report
            .entries
            .iter()
            .all(|(_, o)| *o == RollbackOutcome::Removed));
        assert!(!out.skill_path.parent().unwrap().exists());
        assert!(fs::symlink_metadata(out.link_path).is_err());
        assert!(w.path().join(".agents/skills").is_dir());
        assert!(w.path().join(".claude/skills").is_dir());
    }

    #[test]
    fn release_preserves_edited_skill_and_reports_conflict() {
        let (w, _s, j) = fixture();
        let out = publish_task_skill(&j, w.path(), "task", "body").unwrap();
        fs::write(&out.skill_path, "edited").unwrap();
        let report = release_task(&j, w.path(), "task").unwrap();
        assert_eq!(fs::read_to_string(&out.skill_path).unwrap(), "edited");
        assert!(report
            .entries
            .iter()
            .any(|(p, o)| p == &out.skill_path && matches!(o, RollbackOutcome::Conflict(_))));
        assert!(fs::symlink_metadata(out.link_path).is_err());
    }

    #[test]
    fn foreign_destinations_are_never_overwritten() {
        for link_collision in [false, true] {
            let (w, _s, j) = fixture();
            let foreign = if link_collision {
                w.path().join(".claude/skills/eph-task")
            } else {
                w.path().join(".agents/skills/eph-task/SKILL.md")
            };
            fs::create_dir_all(foreign.parent().unwrap()).unwrap();
            fs::write(&foreign, "foreign").unwrap();
            assert!(publish_task_skill(&j, w.path(), "task", "body").is_err());
            assert_eq!(fs::read_to_string(foreign).unwrap(), "foreign");
            assert!(j.list_transactions().unwrap().is_empty());
            assert!(!w
                .path()
                .join(if link_collision { ".agents" } else { ".claude" })
                .exists());
        }
    }

    #[test]
    fn invalid_task_ids_are_rejected_without_writes() {
        let (w, _s, j) = fixture();
        for id in ["../x", "a/b", "", ".", "a b", &"a".repeat(65)] {
            assert!(matches!(
                publish_task_skill(&j, w.path(), id, "body"),
                Err(LeaseError::InvalidTaskId)
            ));
        }
        assert_eq!(fs::read_dir(w.path()).unwrap().count(), 0);
    }

    #[test]
    fn stale_cleanup_requires_expiry_and_confirmed_inactivity() {
        let (w, _s, j) = fixture();
        let mut outputs = Vec::new();
        for task in ["inactive", "active", "unexpired"] {
            let out = publish_task_skill(&j, w.path(), task, "body").unwrap();
            let path = out.skill_path.parent().unwrap().join(".lease");
            let mut lease: Lease = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            lease.expires_at = if task == "unexpired" { 101 } else { 99 };
            atomic_write(&path, &serde_json::to_vec(&lease).unwrap()).unwrap();
            outputs.push(out);
        }
        let reports = cleanup_stale(&j, w.path(), 100, |id| id == "active").unwrap();
        assert_eq!(reports.len(), 1);
        assert!(!outputs[0].skill_path.exists());
        for out in &outputs[1..] {
            assert!(out.skill_path.exists());
            assert!(fs::symlink_metadata(&out.link_path)
                .unwrap()
                .file_type()
                .is_symlink());
        }
    }

    #[test]
    fn active_renewal_extends_expiry() {
        let (w, _s, j) = fixture();
        let out = publish_task_skill(&j, w.path(), "task", "body").unwrap();
        let path = out.skill_path.parent().unwrap().join(".lease");
        let mut lease: Lease = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        lease.expires_at = 1;
        atomic_write(&path, &serde_json::to_vec(&lease).unwrap()).unwrap();
        renew_lease(&j, w.path(), "task").unwrap();
        let updated: Lease = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert!(updated.expires_at >= current_timestamp());
        assert!(cleanup_stale(&j, w.path(), current_timestamp(), |_| false)
            .unwrap()
            .is_empty());
        assert!(out.skill_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn escaped_parent_and_changed_link_are_preserved() {
        let (w, _s, j) = fixture();
        let outside = tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), w.path().join(".agents")).unwrap();
        assert!(publish_task_skill(&j, w.path(), "task", "body").is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        fs::remove_file(w.path().join(".agents")).unwrap();
        let out = publish_task_skill(&j, w.path(), "task", "body").unwrap();
        fs::remove_file(&out.link_path).unwrap();
        std::os::unix::fs::symlink(outside.path(), &out.link_path).unwrap();
        let report = release_task(&j, w.path(), "task").unwrap();
        assert!(report
            .entries
            .iter()
            .any(|(p, o)| p == &out.link_path && matches!(o, RollbackOutcome::Conflict(_))));
        assert_eq!(fs::read_link(out.link_path).unwrap(), outside.path());
    }
    #[test]
    fn a_foreign_lease_cannot_select_another_tasks_transaction() {
        let (w, _s, j) = fixture();
        let first = publish_task_skill(&j, w.path(), "first", "first body").unwrap();
        let second = publish_task_skill(&j, w.path(), "second", "second body").unwrap();
        fs::copy(
            first.skill_path.parent().unwrap().join(".lease"),
            second.skill_path.parent().unwrap().join(".lease"),
        )
        .unwrap();
        assert!(release_task(&j, w.path(), "second").is_err());
        for out in [first, second] {
            assert!(out.skill_path.is_file());
            assert!(fs::symlink_metadata(out.link_path)
                .unwrap()
                .file_type()
                .is_symlink());
        }
    }

    #[test]
    fn test_secret_finding_blocks_publication_without_creating_link() {
        let (w, _s, j) = fixture();
        let fake_key = format!("AIza{}", "A".repeat(35));
        let content = format!("---\nname: demo\n---\nkey = {fake_key}\n");
        let result = publish_task_skill(&j, w.path(), "task_secret", &content);
        assert!(result.is_err());
        assert!(!w.path().join(".agents/skills/eph-task_secret").exists());
        assert!(!w.path().join(".claude/skills/eph-task_secret").exists());
    }

    #[test]
    fn gc_releases_expired_inactive_keeps_active_and_dry_run_changes_nothing() {
        let (w, _s, j) = fixture();
        let mut outputs = Vec::new();
        for task in ["inactive", "active", "unexpired"] {
            let out = publish_task_skill(&j, w.path(), task, "body").expect("publish task skill");
            let path = out.skill_path.parent().expect("parent dir").join(".lease");
            let mut lease: Lease =
                serde_json::from_slice(&fs::read(&path).expect("read lease")).expect("parse lease");
            lease.expires_at = if task == "unexpired" { 101 } else { 99 };
            atomic_write(&path, &serde_json::to_vec(&lease).expect("serialize lease"))
                .expect("write lease");
            outputs.push(out);
        }

        // Dry-run: list_stale identifies the expired inactive lease without mutating the filesystem.
        let stale = list_stale(&j, w.path(), 100, |id| id == "active").expect("list stale");
        assert_eq!(stale, vec!["inactive"]);
        for out in &outputs {
            assert!(out.skill_path.exists());
            assert!(fs::symlink_metadata(&out.link_path)
                .expect("metadata")
                .file_type()
                .is_symlink());
        }

        // Apply: cleanup_stale releases the expired inactive lease and preserves active and unexpired.
        let reports = cleanup_stale(&j, w.path(), 100, |id| id == "active").expect("cleanup stale");
        assert_eq!(reports.len(), 1);
        assert!(!outputs[0].skill_path.exists());
        assert!(fs::symlink_metadata(&outputs[0].link_path).is_err());
        for out in &outputs[1..] {
            assert!(out.skill_path.exists());
            assert!(fs::symlink_metadata(&out.link_path)
                .expect("metadata")
                .file_type()
                .is_symlink());
        }
    }

    #[test]
    fn gc_sees_a_transaction_written_after_start() {
        let (w, _s, j) = fixture();
        let out =
            publish_task_skill(&j, w.path(), "late_task", "body").expect("publish task skill");
        let path = out.skill_path.parent().expect("parent dir").join(".lease");
        let mut lease: Lease =
            serde_json::from_slice(&fs::read(&path).expect("read lease")).expect("parse lease");
        lease.expires_at = 99;
        atomic_write(&path, &serde_json::to_vec(&lease).expect("serialize lease"))
            .expect("write lease");

        let inactive_out =
            publish_task_skill(&j, w.path(), "inactive", "body").expect("publish task skill");
        let inactive_path = inactive_out
            .skill_path
            .parent()
            .expect("parent dir")
            .join(".lease");
        let mut inactive_lease: Lease =
            serde_json::from_slice(&fs::read(&inactive_path).expect("read lease"))
                .expect("parse lease");
        inactive_lease.expires_at = 99;
        atomic_write(
            &inactive_path,
            &serde_json::to_vec(&inactive_lease).expect("serialize lease"),
        )
        .expect("write lease");

        // Before cleanup starts, journal has no pending transactions.
        assert!(j.pending_transactions().expect("pending").is_empty());

        let j_clone = j.clone();
        let target_path = out.skill_path.clone();
        let is_active = move |task_id: &str| {
            if task_id == "late_task" {
                let entry = JournalEntry {
                    target_path: target_path.clone(),
                    action: JournalAction::Remove,
                    prior_state: BackupKind::Absent,
                    owned_hash: None,
                };
                let _ = j_clone.begin_transaction("tx_writer_late", vec![entry], false);
            }
            match j_clone.pending_transactions() {
                Ok(pending) => task_has_pending_tx(&pending, task_id),
                Err(_) => false,
            }
        };

        let reports = cleanup_stale(&j, w.path(), 100, is_active).expect("cleanup stale");

        assert_eq!(reports.len(), 1);
        assert!(!inactive_out.skill_path.exists());
        assert!(out.skill_path.exists());
        assert!(fs::symlink_metadata(&out.link_path)
            .expect("metadata")
            .file_type()
            .is_symlink());
    }
}
