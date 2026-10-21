//! Ephemeral task skill lifecycle: publish, secret block, release (tempdirs only).

use std::fs;
use tempfile::tempdir;
use xavier::context::skill_controller::lease::{publish_task_skill, release_task, LeaseError};
use xavier::context::skill_controller::rollback::RollbackOutcome;
use xavier::context::skill_controller::state::Journal;

xavier::isolate_test_process!();

#[test]
fn publish_secret_block_then_release() {
    let worktree = tempdir().unwrap();
    let state = tempdir().unwrap();
    let journal = Journal::new(state.path().join("journal")).unwrap();

    let out = publish_task_skill(&journal, worktree.path(), "task1", "safe body").unwrap();
    assert_eq!(fs::read_to_string(&out.skill_path).unwrap(), "safe body");
    assert!(fs::symlink_metadata(&out.link_path).is_ok());

    let secret = format!("key = \"AKIA{}\"", "IOSFODNN7EXAMPLE");
    let err = publish_task_skill(&journal, worktree.path(), "task2", &secret).unwrap_err();
    assert!(matches!(err, LeaseError::Policy(_)), "{err:?}");
    assert!(fs::symlink_metadata(worktree.path().join(".claude/skills/eph-task2")).is_err());
    assert!(!worktree.path().join(".agents/skills/eph-task2").exists());
    assert_eq!(journal.committed_transactions().unwrap().len(), 1);

    let report = release_task(&journal, worktree.path(), "task1").unwrap();
    assert!(report
        .entries
        .iter()
        .all(|(_, o)| *o == RollbackOutcome::Removed));
    assert!(!out.skill_path.exists());
    assert!(fs::symlink_metadata(&out.link_path).is_err());
    assert!(journal.committed_transactions().unwrap().is_empty());
}
