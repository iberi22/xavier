//! Rollback for the skill controller: restores journaled prior link objects or owned
//! copies for one transaction, but only where the destination's current identity still
//! matches exactly what that transaction wrote (D14; 01-ARCHITECTURE-AND-ADR.md:177-185).
//! A destination edited since apply is left untouched and reported as a conflict, never
//! overwritten. Entries are rolled back independently: one conflict never stops the rest
//! of the transaction from rolling back, and rollback never re-derives a plan. Restoring
//! to "nothing existed before" only ever unlinks the exact owned entry a transaction
//! created; this module never removes a directory. `dry_run` is the default: only an
//! explicit `dry_run = false` call writes. This module writes nothing on a Tokio worker
//! without `spawn_blocking`; it reads no environment variable itself, so no
//! `.env.example` key is added here.

use super::backup::{atomic_write, BackupError, BackupKind, MAX_BACKUP_BYTES};
use super::policy::{FsPolicy, RootKind};
use super::projector::{ProjectorError, RootLock, LOCKS_SUBDIR};
use super::state::{Journal, JournalAction, JournalEntry, StateError};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::symlink;
// Package destinations are directories, so a directory link is the Windows equivalent.
#[cfg(windows)]
use std::os::windows::fs::symlink_dir as symlink;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RollbackError {
    #[error("journal error: {0}")]
    State(#[from] StateError),
    #[error("transaction '{0}' not found")]
    TransactionNotFound(String),
    #[error("could not acquire target root lock: {0}")]
    Lock(#[from] ProjectorError),
}

/// What happened to one journaled entry during rollback (or would happen, under `dry_run`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollbackOutcome {
    /// The destination still held exactly what this transaction wrote; it now points at
    /// (or, for a copy, now holds) the prior owned object again.
    RestoredLink {
        to: PathBuf,
    },
    RestoredCopy,
    /// The destination still held exactly what this transaction wrote and there was
    /// nothing before it: the owned entry it created is gone.
    Removed,
    /// The destination already matches the prior state: an earlier rollback already
    /// completed this entry.
    AlreadyRolledBack,
    /// The destination no longer matches what this transaction wrote: left untouched.
    Conflict(String),
}

/// Result of one `rollback_transaction` call: what happened (or would happen) per entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackReport {
    pub tx_id: String,
    pub dry_run: bool,
    pub entries: Vec<(PathBuf, RollbackOutcome)>,
}

/// Roll back every entry of one journaled transaction, independently. An entry whose
/// destination still matches this transaction's own write is restored to its prior state
/// (or removed, if there was none); anything else is reported as a conflict and left
/// intact, so a rollback can never destroy a change made since apply. Repeated calls are
/// idempotent: an entry already at its prior state is reported, not re-mutated.
pub fn rollback_transaction(
    journal: &Journal,
    policy: &FsPolicy,
    tx_id: &str,
    dry_run: bool,
) -> Result<RollbackReport, RollbackError> {
    let tx = journal
        .get_transaction(tx_id)?
        .ok_or_else(|| RollbackError::TransactionNotFound(tx_id.to_string()))?;

    let root_paths: BTreeSet<PathBuf> = tx
        .entries
        .iter()
        .filter_map(|entry| entry.target_path.parent().map(Path::to_path_buf))
        .collect();
    let _locks: Vec<RootLock> = if dry_run {
        Vec::new()
    } else {
        let lock_dir = journal.base_dir().join(LOCKS_SUBDIR);
        root_paths
            .iter()
            .map(|root| RootLock::acquire(&lock_dir, root))
            .collect::<Result<_, _>>()?
    };

    let entries = tx
        .entries
        .iter()
        .rev()
        .map(|entry| {
            (
                entry.target_path.clone(),
                rollback_entry(policy, entry, dry_run),
            )
        })
        .collect();

    if !dry_run {
        journal.rollback_transaction(tx_id, false)?;
    }

    Ok(RollbackReport {
        tx_id: tx_id.to_string(),
        dry_run,
        entries,
    })
}

/// Decide, then (unless `dry_run`) perform, the rollback of one journal entry. Never
/// returns `Err`: any failure that would otherwise abort the call is reported as a
/// `Conflict` instead, so one bad entry cannot stop the rest of the transaction.
fn rollback_entry(policy: &FsPolicy, entry: &JournalEntry, dry_run: bool) -> RollbackOutcome {
    let destination = entry.target_path.as_path();
    if matches_backup(destination, &entry.prior_state) {
        return RollbackOutcome::AlreadyRolledBack;
    }
    if !matches_action(destination, &entry.action) {
        return RollbackOutcome::Conflict(format!(
            "{} no longer matches what this transaction wrote",
            destination.display()
        ));
    }
    let Some(parent) = destination.parent() else {
        return RollbackOutcome::Conflict("destination has no parent directory".to_string());
    };
    if let Err(e) = policy.authorize(RootKind::Target, parent) {
        return RollbackOutcome::Conflict(format!("target root is not authorized: {e}"));
    }
    match &entry.prior_state {
        BackupKind::Absent => {
            if dry_run {
                return RollbackOutcome::Removed;
            }
            match remove_owned(destination) {
                Ok(()) => RollbackOutcome::Removed,
                Err(e) => RollbackOutcome::Conflict(format!("could not remove: {e}")),
            }
        }
        BackupKind::Symlink { target } => {
            if let Err(e) = policy.authorize(RootKind::Source, target) {
                return RollbackOutcome::Conflict(format!(
                    "prior link target is no longer authorized: {e}"
                ));
            }
            if dry_run {
                return RollbackOutcome::RestoredLink { to: target.clone() };
            }
            match atomic_symlink_restore(destination, target) {
                Ok(()) => RollbackOutcome::RestoredLink { to: target.clone() },
                Err(e) => RollbackOutcome::Conflict(format!("could not restore link: {e}")),
            }
        }
        BackupKind::OwnedCopy { backup_path, .. } => {
            let Some(backup_path) = backup_path else {
                return RollbackOutcome::Conflict("no backup content on disk".to_string());
            };
            if dry_run {
                return RollbackOutcome::RestoredCopy;
            }
            match restore_copy(destination, backup_path) {
                Ok(()) => RollbackOutcome::RestoredCopy,
                Err(e) => RollbackOutcome::Conflict(format!("could not restore content: {e}")),
            }
        }
    }
}

/// Whether the destination, right now, already holds this prior state.
fn matches_backup(destination: &Path, prior: &BackupKind) -> bool {
    match prior {
        BackupKind::Absent => is_absent(destination),
        BackupKind::Symlink { target } => is_symlink_pointing_to(destination, target),
        BackupKind::OwnedCopy { hash, .. } => is_file_with_hash(destination, hash),
    }
}

/// Whether the destination, right now, still holds exactly what this action wrote.
fn matches_action(destination: &Path, action: &JournalAction) -> bool {
    match action {
        JournalAction::LinkSymlink { source } => is_symlink_pointing_to(destination, source),
        JournalAction::Remove => is_absent(destination),
        JournalAction::WriteCopy { hash, .. } => is_file_with_hash(destination, hash),
    }
}

fn is_absent(path: &Path) -> bool {
    matches!(fs::symlink_metadata(path), Err(e) if e.kind() == io::ErrorKind::NotFound)
}

/// Exact `readlink` comparison, never a canonicalized one: the journal recorded the raw
/// link bytes, and a rollback restores that same object identity, not a resolved target.
fn is_symlink_pointing_to(path: &Path, expected: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::read_link(path).map(|l| l == expected).unwrap_or(false)
        }
        _ => false,
    }
}

/// Mirrors `BackupManager::backup_target`'s bound check: a destination over the cap is
/// never read, and is conservatively reported as not matching (a conflict), the same as an
/// unrelated file.
fn is_file_with_hash(path: &Path, expected_hash: &str) -> bool {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_BACKUP_BYTES => {
            hash_file(path).map(|h| h == expected_hash).unwrap_or(false)
        }
        _ => false,
    }
}

fn hash_file(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(crate::crypto::hex_encode(Sha256::digest(&bytes)))
}

fn remove_owned(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// D9-style atomic swap, own to this module since the projector's helper is private:
/// stage the link at a unique temporary name beside the destination, `rename` into
/// place, then fsync the parent directory.
fn atomic_symlink_restore(destination: &Path, target: &Path) -> io::Result<()> {
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination has no parent directory",
        )
    })?;
    let tmp = parent.join(format!(".tmp-{}", uuid::Uuid::new_v4().simple()));
    symlink(target, &tmp)?;
    if let Err(err) = fs::rename(&tmp, destination) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    fs::File::open(parent)?.sync_all()
}

fn restore_copy(destination: &Path, backup_path: &Path) -> Result<(), BackupError> {
    let bytes = fs::read(backup_path).map_err(|source| BackupError::Io {
        path: backup_path.to_path_buf(),
        source,
    })?;
    atomic_write(destination, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::skill_controller::manifest::Manifest;
    use crate::context::skill_controller::plan::{PlanAction, ResolvedRoots};
    use crate::context::skill_controller::planner;
    use crate::context::skill_controller::projector::apply_plan;
    use crate::context::skill_controller::state::TransactionState;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    const MANIFEST_1: &str = concat!(
        "version = 1\n[sources]\ncanonical = \"$STORE\"\n",
        "[[targets]]\nid = \"t1\"\ntools = [\"codex\"]\nroot = \"$ROOT\"\nmode = \"symlink\"\n",
        "[[placements]]\nsource = \"canonical\"\npath = \"pkg_a/keep\"\nname = \"keep\"\ntargets = [\"t1\"]\n",
    );
    const MANIFEST_2: &str = concat!(
        "version = 1\n[sources]\ncanonical = \"$STORE\"\n",
        "[[targets]]\nid = \"t1\"\ntools = [\"codex\"]\nroot = \"$ROOT\"\nmode = \"symlink\"\n",
        "[[placements]]\nsource = \"canonical\"\npath = \"pkg_a2/keep\"\nname = \"keep\"\ntargets = [\"t1\"]\n",
        "[[placements]]\nsource = \"canonical\"\npath = \"pkg_b/newone\"\nname = \"newone\"\ntargets = [\"t1\"]\n",
    );

    fn member(dir: &Path, relative: &str, body: &str) {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, body).expect("write");
    }

    fn policy_for(source: &Path, target: &Path) -> FsPolicy {
        FsPolicy::new(
            std::slice::from_ref(&source.to_path_buf()),
            std::slice::from_ref(&target.to_path_buf()),
        )
        .expect("policy")
    }

    fn journal_for(tmp: &Path) -> Journal {
        Journal::new(tmp.join("state")).expect("journal")
    }

    /// `(tempdir, source root, target root, resolved roots)`
    fn fixture() -> (TempDir, PathBuf, PathBuf, ResolvedRoots) {
        let tmp = TempDir::new().expect("tmp");
        let source = tmp.path().join("store");
        let target = tmp.path().join("tools/skills");
        fs::create_dir_all(&target).expect("mkdir target");
        member(
            &source,
            "pkg_a/keep/SKILL.md",
            "---\nname: keep\n---\n\nBody A.\n",
        );
        member(
            &source,
            "pkg_a2/keep/SKILL.md",
            "---\nname: keep\n---\n\nBody A2.\n",
        );
        member(
            &source,
            "pkg_b/newone/SKILL.md",
            "---\nname: newone\n---\n\nBody B.\n",
        );
        let roots = ResolvedRoots {
            sources: BTreeMap::from([("canonical".to_string(), source.clone())]),
            targets: BTreeMap::from([("t1".to_string(), target.clone())]),
        };
        (tmp, source, target, roots)
    }

    fn outcome_for<'a>(report: &'a RollbackReport, name: &str) -> &'a RollbackOutcome {
        report
            .entries
            .iter()
            .find(|(path, _)| path.ends_with(name))
            .map(|(_, outcome)| outcome)
            .unwrap_or_else(|| panic!("no rollback outcome recorded for '{name}'"))
    }

    /// Publish `keep` from `pkg_a` (tx-seed), then republish `keep` from `pkg_a2` and
    /// create `newone` from `pkg_b` in one transaction (tx-2): a Refresh and a Create in
    /// the same journaled tx. Returns the original raw link value of `keep`.
    fn seed_refresh_and_create(
        policy: &FsPolicy,
        roots: &ResolvedRoots,
        journal: &Journal,
    ) -> PathBuf {
        let manifest1 = Manifest::from_toml(MANIFEST_1).expect("manifest1");
        let plan1 = planner::build_plan(&manifest1, policy, roots).expect("plan1");
        apply_plan(&plan1, &manifest1, policy, roots, journal, "tx-seed", false)
            .expect("seed apply");
        let original_keep_link =
            fs::read_link(roots.targets.get("t1").expect("t1 root").join("keep"))
                .expect("readlink keep");

        let manifest2 = Manifest::from_toml(MANIFEST_2).expect("manifest2");
        let plan2 = planner::build_plan(&manifest2, policy, roots).expect("plan2");
        assert_eq!(
            plan2
                .entries
                .iter()
                .find(|e| e.name == "keep")
                .expect("keep entry")
                .action,
            PlanAction::Refresh
        );
        assert_eq!(
            plan2
                .entries
                .iter()
                .find(|e| e.name == "newone")
                .expect("newone entry")
                .action,
            PlanAction::Create
        );
        apply_plan(&plan2, &manifest2, policy, roots, journal, "tx-2", false).expect("apply2");
        original_keep_link
    }

    #[test]
    fn test_rollback_restores_unchanged_prior_link_and_removes_unchanged_new_link() {
        let (tmp, source, target, roots) = fixture();
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());
        let original_keep_link = seed_refresh_and_create(&policy, &roots, &journal);
        assert!(fs::symlink_metadata(target.join("newone")).is_ok());

        let report = rollback_transaction(&journal, &policy, "tx-2", false).expect("rollback");
        assert!(!report.dry_run && report.entries.len() == 2);
        assert_eq!(
            outcome_for(&report, "keep"),
            &RollbackOutcome::RestoredLink {
                to: original_keep_link.clone()
            }
        );
        assert_eq!(outcome_for(&report, "newone"), &RollbackOutcome::Removed);

        assert_eq!(
            fs::read_link(target.join("keep")).expect("readlink keep"),
            original_keep_link
        );
        assert!(fs::symlink_metadata(target.join("newone")).is_err());

        let tx = journal
            .get_transaction("tx-2")
            .expect("read tx")
            .expect("tx exists");
        assert_eq!(tx.state, TransactionState::RolledBack);
    }

    #[test]
    fn test_rollback_leaves_user_edited_destination_intact_and_reports_conflict() {
        let (tmp, source, target, roots) = fixture();
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());
        seed_refresh_and_create(&policy, &roots, &journal);

        fs::remove_file(target.join("newone")).expect("remove owned link");
        fs::write(target.join("newone"), b"user content").expect("user edit");

        let report = rollback_transaction(&journal, &policy, "tx-2", false).expect("rollback");
        assert!(matches!(
            outcome_for(&report, "newone"),
            RollbackOutcome::Conflict(_)
        ));
        assert_eq!(
            fs::read(target.join("newone")).expect("read"),
            b"user content",
            "the user's edit must survive untouched"
        );
        assert!(matches!(
            outcome_for(&report, "keep"),
            RollbackOutcome::RestoredLink { .. }
        ));
    }

    #[test]
    fn test_repeated_rollback_is_idempotent() {
        let (tmp, source, target, roots) = fixture();
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());
        seed_refresh_and_create(&policy, &roots, &journal);

        let first = rollback_transaction(&journal, &policy, "tx-2", false).expect("first rollback");
        assert_eq!(outcome_for(&first, "newone"), &RollbackOutcome::Removed);

        let second =
            rollback_transaction(&journal, &policy, "tx-2", false).expect("second rollback");
        assert_eq!(
            outcome_for(&second, "newone"),
            &RollbackOutcome::AlreadyRolledBack
        );
        assert_eq!(
            outcome_for(&second, "keep"),
            &RollbackOutcome::AlreadyRolledBack
        );
        assert!(
            fs::symlink_metadata(target.join("newone")).is_err(),
            "still gone, not recreated by a second rollback"
        );
    }

    #[test]
    fn test_dry_run_rollback_reports_without_touching_disk() {
        let (tmp, source, target, roots) = fixture();
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());
        seed_refresh_and_create(&policy, &roots, &journal);
        let before_keep = fs::read_link(target.join("keep")).expect("link before");

        let report = rollback_transaction(&journal, &policy, "tx-2", true).expect("dry rollback");
        assert!(report.dry_run);
        assert_eq!(
            fs::read_link(target.join("keep")).expect("link unchanged"),
            before_keep
        );
        assert!(
            fs::symlink_metadata(target.join("newone")).is_ok(),
            "dry run must not delete"
        );
        let tx = journal
            .get_transaction("tx-2")
            .expect("read tx")
            .expect("tx exists");
        assert_eq!(
            tx.state,
            TransactionState::Committed,
            "dry run must not change transaction state"
        );
    }

    /// The recorded hash genuinely matches the on-disk bytes: absent the size cap, this would
    /// look like an unchanged owned copy and its `Absent` prior state would delete it.
    #[test]
    fn test_oversized_destination_is_reported_as_conflict_without_reading() {
        let (tmp, source, target, _roots) = fixture();
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());

        let destination = target.join("huge");
        let oversized = vec![b'x'; (MAX_BACKUP_BYTES + 1) as usize];
        fs::write(&destination, &oversized).expect("write oversized");
        let hash = crate::crypto::hex_encode(Sha256::digest(&oversized));
        let entry = JournalEntry {
            target_path: destination.clone(),
            action: JournalAction::WriteCopy {
                source: source.join("pkg/huge"),
                hash,
            },
            prior_state: BackupKind::Absent,
            owned_hash: None,
        };
        journal
            .begin_transaction("tx-huge", vec![entry], false)
            .expect("begin tx");

        let report = rollback_transaction(&journal, &policy, "tx-huge", false).expect("rollback");
        assert!(matches!(
            outcome_for(&report, "huge"),
            RollbackOutcome::Conflict(_)
        ));
        assert_eq!(fs::read(&destination).expect("still there"), oversized);
    }

    #[test]
    fn test_rollback_fails_cleanly_when_target_root_is_locked() {
        let (tmp, source, target, roots) = fixture();
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());
        seed_refresh_and_create(&policy, &roots, &journal);
        let before_keep = fs::read_link(target.join("keep")).expect("link before");

        let lock_dir = journal.base_dir().join(LOCKS_SUBDIR);
        let _held = RootLock::acquire(&lock_dir, &target).expect("acquire lock");

        let err = rollback_transaction(&journal, &policy, "tx-2", false).expect_err("locked");
        assert!(matches!(err, RollbackError::Lock(_)));
        assert_eq!(
            fs::read_link(target.join("keep")).expect("unchanged"),
            before_keep
        );
        assert!(fs::symlink_metadata(target.join("newone")).is_ok());
        let tx = journal
            .get_transaction("tx-2")
            .expect("read")
            .expect("exists");
        assert_eq!(tx.state, TransactionState::Committed);
    }

    #[test]
    fn test_rollback_unknown_transaction_errors() {
        let tmp = TempDir::new().expect("tmp");
        let source = tmp.path().join("store");
        let target = tmp.path().join("target");
        fs::create_dir_all(&source).expect("mkdir source");
        fs::create_dir_all(&target).expect("mkdir target");
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());

        let err = rollback_transaction(&journal, &policy, "missing-tx", false)
            .expect_err("unknown transaction must fail");
        match err {
            RollbackError::TransactionNotFound(id) => assert_eq!(id, "missing-tx"),
            other => panic!("expected TransactionNotFound, got {other:?}"),
        }
    }
}
