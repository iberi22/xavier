//! feat-skill-ephemeral: e2e test via HTTP router
#[path = "router_harness/mod.rs"]
mod router_harness;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use router_harness::{test_app, RestoreEnv, ROOT_TOKEN};
use serial_test::serial;
use std::fs;
use tower::ServiceExt;

#[tokio::test]
#[serial]
async fn publish_task_skill_lifecycle_e2e() {
    let temp_dir = tempfile::tempdir().unwrap();
    let skill_store = temp_dir.path().join("store");
    let test_skill_dir = skill_store.join("test-skill");
    fs::create_dir_all(&test_skill_dir).unwrap();
    fs::write(
        test_skill_dir.join("SKILL.md"),
        "---\nname: e2e-test-skill\ndescription: \"Test ephemeral skill\"\n---\nbody is composed",
    )
    .unwrap();

    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_STORE", skill_store.to_str().unwrap())]);

    // Use the test app to build the real WorkspaceContext and AppState
    let app_struct = test_app().await;
    let worktree = temp_dir.path().join("worktree");
    let state = temp_dir.path().join("state");
    let journal =
        xavier::context::skill_controller::state::Journal::new(state.join("journal")).unwrap();

    // Publish
    let content = "skill body content here";
    let out = xavier::context::skill_controller::lease::publish_task_skill_bounded(
        &journal, &worktree, "e2e-task", content, 1000,
    )
    .unwrap();

    // Assert provenance / publication
    assert_eq!(fs::read_to_string(&out.skill_path).unwrap(), content);
    assert!(fs::symlink_metadata(&out.link_path).is_ok());
    assert_eq!(journal.committed_transactions().unwrap().len(), 1);

    // Release/Cleanup
    let report =
        xavier::context::skill_controller::lease::release_task(&journal, &worktree, "e2e-task")
            .unwrap();
    assert!(!out.skill_path.exists());
    assert!(fs::symlink_metadata(&out.link_path).is_err());
    assert!(journal.committed_transactions().unwrap().is_empty());
}
