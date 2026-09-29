//! Applies an already-rechecked `ProjectionPlan` to disk: one symlink per selected package,
//! published through a temporary link + rename + parent fsync (D9), never a whole discovery
//! directory (D2). `apply` is never the default: callers choose `dry_run` explicitly (D4).
//!
//! Every mutation goes through the J04 journal so a crash mid-apply leaves a recoverable,
//! `Pending` transaction (D14). Target roots are locked, in sorted order, for the duration of a
//! real apply so two concurrent callers cannot interleave writes to the same root. A package is
//! secret-scanned immediately before publication and blocked on any finding (D10); a `Conflict`
//! entry (an unmanaged destination) is reported and left untouched, never overwritten or deleted.
//! This module writes nothing on a Tokio worker without `spawn_blocking`; it reads no environment
//! variable itself, so no `.env.example` key is added here.

use super::manifest::{Manifest, TargetMode};
use super::plan::{
    PlanAction, PlanEntry, PlanRemoval, PlannerError, ProjectionPlan, ResolvedRoots,
};
use super::planner;
use super::policy::{scan_package, FsPolicy, PolicyError, RootKind};
use super::state::{Journal, JournalAction, StateError};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

/// Subdirectory of the journal's base directory holding per-root advisory lock files.
pub const LOCKS_SUBDIR: &str = "locks";

#[derive(Debug, Error)]
pub enum ProjectorError {
    #[error("plan preconditions no longer hold: {0}")]
    Stale(#[from] PlannerError),
    #[error("filesystem policy rejected the operation: {0}")]
    Policy(#[from] PolicyError),
    #[error("journal error: {0}")]
    State(#[from] StateError),
    #[error("target '{target}' root '{root}' does not exist yet; create it before apply")]
    RootMissing { target: String, root: String },
    #[error("'{name}' is declared with copy mode, which this projector does not publish")]
    UnsupportedMode { name: String },
    #[error("target root '{root}' is held by another operation ({path}): {source}")]
    RootLocked {
        root: String,
        path: PathBuf,
        source: io::Error,
    },
    #[error("I/O error at {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
}

/// What happened to one plan entry during apply (or would happen, under `dry_run`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryOutcome {
    Created,
    Refreshed,
    Unchanged,
    SkippedConflict,
    /// The package's secret scan blocked publication; carries the redacted, class-and-location
    /// summary (D10). Never the matched bytes.
    BlockedSecret(String),
}

/// Result of one `apply_plan` call: what happened per entry and per removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    pub tx_id: String,
    pub dry_run: bool,
    pub applied: Vec<(String, String, EntryOutcome)>,
    pub removed: Vec<(String, String)>,
}

/// Recheck the plan, lock every involved target root in sorted order, then publish every
/// entry and remove every stale controller-owned link, each under its own journal entry.
/// A failure on one entry aborts the whole call (`Err`) but leaves every already-applied
/// entry in place and journaled; nothing already published is rolled back here (J06's job).
pub fn apply_plan(
    plan: &ProjectionPlan,
    manifest: &Manifest,
    policy: &FsPolicy,
    roots: &ResolvedRoots,
    journal: &Journal,
    tx_id: &str,
    dry_run: bool,
) -> Result<ApplyReport, ProjectorError> {
    planner::recheck(plan, manifest, policy, roots)?;

    let mut root_paths: BTreeSet<PathBuf> = plan
        .entries
        .iter()
        .map(|e| PathBuf::from(&e.root))
        .collect();
    root_paths.extend(plan.removals.iter().map(|r| PathBuf::from(&r.root)));

    let _locks: Vec<RootLock> = if dry_run {
        Vec::new()
    } else {
        let lock_dir = journal.base_dir().join(LOCKS_SUBDIR);
        root_paths
            .iter()
            .map(|root| RootLock::acquire(&lock_dir, root))
            .collect::<Result<_, _>>()?
    };

    journal.begin_transaction(tx_id, Vec::new(), dry_run)?;

    let mut applied = Vec::with_capacity(plan.entries.len());
    for entry in &plan.entries {
        let outcome = apply_entry(entry, roots, policy, journal, tx_id, dry_run)?;
        applied.push((entry.name.clone(), entry.target.clone(), outcome));
    }
    let mut removed = Vec::with_capacity(plan.removals.len());
    for removal in &plan.removals {
        remove_entry(removal, journal, tx_id, dry_run)?;
        removed.push((removal.name.clone(), removal.target.clone()));
    }

    if !dry_run {
        journal.commit_transaction(tx_id, false)?;
    }

    Ok(ApplyReport {
        tx_id: tx_id.to_string(),
        dry_run,
        applied,
        removed,
    })
}

/// Publish (or, under `dry_run`, describe) one plan entry. `Unchanged`/`Conflict` entries are
/// never journaled: there is nothing to write and nothing owned to touch.
fn apply_entry(
    entry: &PlanEntry,
    roots: &ResolvedRoots,
    policy: &FsPolicy,
    journal: &Journal,
    tx_id: &str,
    dry_run: bool,
) -> Result<EntryOutcome, ProjectorError> {
    match entry.action {
        PlanAction::Unchanged => return Ok(EntryOutcome::Unchanged),
        PlanAction::Conflict => return Ok(EntryOutcome::SkippedConflict),
        PlanAction::Create | PlanAction::Refresh => {}
    }
    if entry.mode != TargetMode::Symlink {
        return Err(ProjectorError::UnsupportedMode {
            name: entry.name.clone(),
        });
    }
    if !entry.root_present {
        return Err(ProjectorError::RootMissing {
            target: entry.target.clone(),
            root: entry.root.clone(),
        });
    }

    let source_root = roots
        .sources
        .get(&entry.source)
        .ok_or_else(|| PlannerError::UnresolvedSource(entry.source.clone()))?;
    let package = policy.authorize(RootKind::Source, &source_root.join(&entry.package))?;
    let scan = scan_package(&package)?;
    if scan.blocks_publication() {
        return Ok(EntryOutcome::BlockedSecret(scan.to_string()));
    }

    let target_root = policy.authorize(RootKind::Target, Path::new(&entry.root))?;
    let destination = target_root.path().join(&entry.name);
    let package_dir = package.path().to_path_buf();
    journal.apply_with_journal(
        tx_id,
        &destination,
        JournalAction::LinkSymlink {
            source: package_dir.clone(),
        },
        Some(entry.package_hash.clone()),
        dry_run,
        |target_path| {
            atomic_symlink_swap(target_path, &package_dir).map_err(|source| StateError::Io {
                path: target_path.to_path_buf(),
                source,
            })
        },
    )?;
    Ok(if entry.action == PlanAction::Create {
        EntryOutcome::Created
    } else {
        EntryOutcome::Refreshed
    })
}

/// Unlink one controller-owned destination the manifest no longer selects. `plan.removals` only
/// ever names entries the planner proved resolve into an authorized source root (never an
/// unmanaged file or directory) at plan time, but that proof can go stale between plan and apply
/// (a concurrent write, a bug in a future planner change). As defense in depth, the mutation itself
/// re-observes the destination and refuses to delete anything that is not a symlink: an unowned
/// regular file or directory that appears in its place is left untouched and the call fails closed.
fn remove_entry(
    removal: &PlanRemoval,
    journal: &Journal,
    tx_id: &str,
    dry_run: bool,
) -> Result<(), ProjectorError> {
    let destination = Path::new(&removal.root).join(&removal.name);
    journal.apply_with_journal(
        tx_id,
        &destination,
        JournalAction::Remove,
        None,
        dry_run,
        |target_path| match fs::symlink_metadata(target_path) {
            Ok(meta) if meta.file_type().is_symlink() => match fs::remove_file(target_path) {
                Ok(()) => Ok(()),
                Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(source) => Err(StateError::Io {
                    path: target_path.to_path_buf(),
                    source,
                }),
            },
            Ok(_) => Err(StateError::Io {
                path: target_path.to_path_buf(),
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "refusing to remove a destination that is not a Xavier-owned symlink",
                ),
            }),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(StateError::Io {
                path: target_path.to_path_buf(),
                source,
            }),
        },
    )?;
    Ok(())
}

/// D9: stage the link at a unique temporary name beside the destination, `rename` it into place,
/// then fsync the parent directory. Never places a link over an existing path: `symlink` fails
/// closed if the temporary name is somehow already taken.
fn atomic_symlink_swap(destination: &Path, source: &Path) -> io::Result<()> {
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination has no parent directory",
        )
    })?;
    let tmp = parent.join(format!(".tmp-{}", Uuid::new_v4().simple()));
    symlink(source, &tmp)?;
    if let Err(err) = fs::rename(&tmp, destination) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    File::open(parent)?.sync_all()
}

/// An exclusive, cross-process advisory lock on one target root, held for the duration of a real
/// apply. Backed by a plain `create_new` marker file under the journal's state directory (never
/// inside the target root itself, so it never shows up in a projection scan); released on drop.
#[derive(Debug)]
struct RootLock {
    path: PathBuf,
}

impl RootLock {
    fn acquire(lock_dir: &Path, root: &Path) -> Result<Self, ProjectorError> {
        use sha2::{Digest, Sha256};
        fs::create_dir_all(lock_dir).map_err(|source| ProjectorError::Io {
            path: lock_dir.to_path_buf(),
            source,
        })?;
        let hash = crate::crypto::hex_encode(Sha256::digest(root.to_string_lossy().as_bytes()));
        let short = &hash[..hash.len().min(16)];
        let path = lock_dir.join(format!("{short}.lock"));
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| ProjectorError::RootLocked {
                root: root.to_string_lossy().into_owned(),
                path: path.clone(),
                source,
            })?;
        Ok(Self { path })
    }
}

impl Drop for RootLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::skill_controller::state::TransactionState;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    const MANIFEST: &str = concat!(
        "version = 1\n[sources]\ncanonical = \"$STORE\"\n",
        "[[targets]]\nid = \"t1\"\ntools = [\"codex\"]\nroot = \"$ROOT\"\nmode = \"symlink\"\n",
        "[[placements]]\nsource = \"canonical\"\npath = \"demo1\"\nname = \"demo1\"\ntargets = [\"t1\"]\n",
        "[[placements]]\nsource = \"canonical\"\npath = \"demo2\"\nname = \"demo2\"\ntargets = [\"t1\"]\n",
    );

    fn member(dir: &Path, relative: &str, body: &str) {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, body).expect("write");
    }

    /// A fake credential assembled at runtime, so no fixture holds secret bytes.
    fn fake_secret() -> String {
        let mut key = String::from("AIza");
        for _ in 0..10 {
            key.push_str("SyD");
        }
        key
    }

    fn fixture() -> (TempDir, PathBuf, PathBuf, Manifest, ResolvedRoots) {
        let tmp = TempDir::new().expect("tmp");
        let source = tmp.path().join("store");
        let target = tmp.path().join("tools/skills");
        fs::create_dir_all(&target).expect("mkdir target");
        member(
            &source,
            "demo1/SKILL.md",
            "---\nname: demo1\n---\n\nBody.\n",
        );
        member(
            &source,
            "demo2/SKILL.md",
            "---\nname: demo2\n---\n\nBody.\n",
        );
        let roots = ResolvedRoots {
            sources: BTreeMap::from([("canonical".to_string(), source.clone())]),
            targets: BTreeMap::from([("t1".to_string(), target.clone())]),
        };
        let manifest = Manifest::from_toml(MANIFEST).expect("manifest");
        (tmp, source, target, manifest, roots)
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

    #[test]
    fn test_apply_plan_publishes_symlinks_and_second_apply_is_idempotent() {
        let (tmp, source, target, manifest, roots) = fixture();
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());

        let plan = planner::build_plan(&manifest, &policy, &roots).expect("plan");
        let report =
            apply_plan(&plan, &manifest, &policy, &roots, &journal, "tx-1", false).expect("apply");
        assert!(!report.dry_run && report.applied.len() == 2);
        assert!(report
            .applied
            .iter()
            .all(|(_, _, outcome)| *outcome == EntryOutcome::Created));

        let demo1_link = target.join("demo1");
        assert!(fs::symlink_metadata(&demo1_link)
            .expect("meta")
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::canonicalize(&demo1_link).expect("resolve link"),
            fs::canonicalize(source.join("demo1")).expect("resolve source")
        );
        let leftover_tmp = fs::read_dir(&target)
            .expect("read target")
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().starts_with(".tmp-"));
        assert!(
            !leftover_tmp,
            "no temporary swap entry should survive apply"
        );

        let committed = journal
            .get_transaction("tx-1")
            .expect("read tx")
            .expect("tx exists");
        assert_eq!(committed.state, TransactionState::Committed);

        let plan2 = planner::build_plan(&manifest, &policy, &roots).expect("plan2");
        let report2 = apply_plan(&plan2, &manifest, &policy, &roots, &journal, "tx-2", false)
            .expect("apply2");
        assert!(report2
            .applied
            .iter()
            .all(|(_, _, outcome)| *outcome == EntryOutcome::Unchanged));
    }

    #[test]
    fn test_apply_rejects_unmanaged_regular_file_collision_without_touching_it() {
        let (tmp, source, target, manifest, roots) = fixture();
        member(&target, "demo1", "hand written content\n");
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());

        let plan = planner::build_plan(&manifest, &policy, &roots).expect("plan");
        assert_eq!(
            plan.entries
                .iter()
                .find(|e| e.name == "demo1")
                .expect("entry")
                .action,
            PlanAction::Conflict
        );

        let report = apply_plan(
            &plan,
            &manifest,
            &policy,
            &roots,
            &journal,
            "tx-conflict",
            false,
        )
        .expect("apply");
        let demo1_outcome = report
            .applied
            .iter()
            .find(|(name, _, _)| name == "demo1")
            .expect("demo1 reported")
            .2
            .clone();
        assert_eq!(demo1_outcome, EntryOutcome::SkippedConflict);
        assert_eq!(
            fs::read_to_string(target.join("demo1")).expect("read"),
            "hand written content\n",
            "the unmanaged file must survive untouched"
        );
        assert!(fs::symlink_metadata(target.join("demo2"))
            .expect("meta")
            .file_type()
            .is_symlink());
    }

    #[test]
    fn test_fault_between_entries_leaves_recoverable_pending_journal_and_no_partial_mutation() {
        let (tmp, source, target, manifest, roots) = fixture();
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());
        let plan = planner::build_plan(&manifest, &policy, &roots).expect("plan");
        let tx_id = "tx-fault";
        journal
            .begin_transaction(tx_id, Vec::new(), false)
            .expect("begin");

        let demo1 = plan
            .entries
            .iter()
            .find(|e| e.name == "demo1")
            .expect("demo1");
        let demo2 = plan
            .entries
            .iter()
            .find(|e| e.name == "demo2")
            .expect("demo2");

        let outcome1 =
            apply_entry(demo1, &roots, &policy, &journal, tx_id, false).expect("entry1 applies");
        assert_eq!(outcome1, EntryOutcome::Created);

        // Fault injected at the boundary between two mutations: demo2's package vanishes.
        fs::remove_dir_all(source.join("demo2")).expect("simulate fault");
        let err = apply_entry(demo2, &roots, &policy, &journal, tx_id, false)
            .expect_err("entry2 must fail once its source vanished");
        assert!(matches!(err, ProjectorError::Policy(_)));

        let recovered = journal
            .get_transaction(tx_id)
            .expect("read tx")
            .expect("tx exists");
        assert_eq!(recovered.state, TransactionState::Pending);
        assert_eq!(
            recovered.entries.len(),
            1,
            "only the entry that actually mutated disk is journaled"
        );
        assert!(
            fs::symlink_metadata(target.join("demo1")).is_ok(),
            "the already-applied entry is left in place"
        );
        assert!(
            fs::symlink_metadata(target.join("demo2")).is_err(),
            "no unauthorized entry was created for the failed one"
        );
    }

    #[test]
    fn test_root_lock_blocks_a_second_concurrent_acquire_on_the_same_root() {
        let tmp = TempDir::new().expect("tmp");
        let lock_dir = tmp.path().join("locks");
        let root = tmp.path().join("tools/skills");
        fs::create_dir_all(&root).expect("mkdir");

        let first = RootLock::acquire(&lock_dir, &root).expect("first lock");
        let err = RootLock::acquire(&lock_dir, &root).expect_err("second lock must fail");
        assert!(matches!(err, ProjectorError::RootLocked { .. }));
        drop(first);
        RootLock::acquire(&lock_dir, &root).expect("lock is released after drop");
    }

    #[test]
    fn test_secret_finding_blocks_publication_without_creating_a_link() {
        let (tmp, source, target, manifest, roots) = fixture();
        member(
            &source,
            "demo1/references/notes.md",
            &format!("key = {}\n", fake_secret()),
        );
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());
        let plan = planner::build_plan(&manifest, &policy, &roots).expect("plan");

        let report = apply_plan(
            &plan,
            &manifest,
            &policy,
            &roots,
            &journal,
            "tx-secret",
            false,
        )
        .expect("apply");
        let demo1_outcome = report
            .applied
            .iter()
            .find(|(name, _, _)| name == "demo1")
            .expect("demo1 reported")
            .2
            .clone();
        assert!(matches!(demo1_outcome, EntryOutcome::BlockedSecret(_)));
        assert!(
            fs::symlink_metadata(target.join("demo1")).is_err(),
            "a package that fails the secret scan must not be published"
        );
    }

    #[test]
    fn test_apply_plan_removes_a_link_the_manifest_no_longer_selects() {
        const MANIFEST_DEMO2_ONLY: &str = concat!(
            "version = 1\n[sources]\ncanonical = \"$STORE\"\n",
            "[[targets]]\nid = \"t1\"\ntools = [\"codex\"]\nroot = \"$ROOT\"\nmode = \"symlink\"\n",
            "[[placements]]\nsource = \"canonical\"\npath = \"demo2\"\nname = \"demo2\"\ntargets = [\"t1\"]\n",
        );
        let (tmp, source, target, manifest, roots) = fixture();
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());

        let plan = planner::build_plan(&manifest, &policy, &roots).expect("plan");
        apply_plan(
            &plan, &manifest, &policy, &roots, &journal, "tx-seed", false,
        )
        .expect("seed apply");
        assert!(fs::symlink_metadata(target.join("demo1")).is_ok());

        let manifest2 = Manifest::from_toml(MANIFEST_DEMO2_ONLY).expect("manifest without demo1");
        let plan2 = planner::build_plan(&manifest2, &policy, &roots).expect("plan2");
        assert_eq!(plan2.removals.len(), 1);
        assert_eq!(plan2.removals[0].name, "demo1");

        let report = apply_plan(
            &plan2,
            &manifest2,
            &policy,
            &roots,
            &journal,
            "tx-remove",
            false,
        )
        .expect("remove apply");
        assert_eq!(
            report.removed,
            vec![("demo1".to_string(), "t1".to_string())]
        );
        assert!(
            fs::symlink_metadata(target.join("demo1")).is_err(),
            "the stale link must be gone"
        );
        assert!(
            fs::symlink_metadata(target.join("demo2")).is_ok(),
            "an unrelated, still-selected link must survive"
        );
    }

    #[test]
    fn test_remove_entry_is_idempotent_and_refuses_unmanaged_regular_file_collision() {
        let (tmp, source, target, _manifest, _roots) = fixture();
        let journal = journal_for(tmp.path());

        let link_path = target.join("demo1-link");
        symlink(source.join("demo1"), &link_path).expect("seed symlink");
        let removal = PlanRemoval {
            name: "demo1-link".to_string(),
            target: "t1".to_string(),
            root: target.display().to_string(),
            link: "unused".to_string(),
        };
        remove_entry(&removal, &journal, "tx-rm-1", false).expect("first removal");
        assert!(fs::symlink_metadata(&link_path).is_err());
        remove_entry(&removal, &journal, "tx-rm-2", false)
            .expect("second removal of an already-gone link is a no-op");

        member(&target, "rogue", "not a symlink\n");
        let rogue_removal = PlanRemoval {
            name: "rogue".to_string(),
            target: "t1".to_string(),
            root: target.display().to_string(),
            link: "unused".to_string(),
        };
        let err = remove_entry(&rogue_removal, &journal, "tx-rogue", false)
            .expect_err("a regular file must never be removed as if it were an owned symlink");
        assert!(matches!(err, ProjectorError::State(_)));
        assert_eq!(
            fs::read_to_string(target.join("rogue")).expect("read"),
            "not a symlink\n",
            "the unmanaged file must survive untouched"
        );
    }

    #[test]
    fn test_dry_run_apply_reports_without_touching_disk() {
        let (tmp, source, target, manifest, roots) = fixture();
        let policy = policy_for(&source, &target);
        let journal = journal_for(tmp.path());
        let plan = planner::build_plan(&manifest, &policy, &roots).expect("plan");

        let report =
            apply_plan(&plan, &manifest, &policy, &roots, &journal, "tx-dry", true).expect("dry");
        assert!(report.dry_run);
        assert!(report
            .applied
            .iter()
            .all(|(_, _, outcome)| *outcome == EntryOutcome::Created));
        assert!(fs::symlink_metadata(target.join("demo1")).is_err());
        assert!(!journal.transactions_dir().exists());
    }
}
