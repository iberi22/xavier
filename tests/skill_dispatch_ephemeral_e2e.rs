#[path = "router_harness/mod.rs"]
mod router_harness;

use router_harness::RestoreEnv;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;
use xavier::context::skill_controller::lease::{publish_task_skill_bounded, release_task};
use xavier::context::skill_controller::state::Journal;
use xavier::context::skill_dispatcher::{SkillDispatchRequest, SkillDispatcher};
use xavier::context::skill_registry::SkillRegistry;
use xavier::context::token_estimate::estimate_tokens;
use xavier::memory::qmd_memory::QmdMemory;

xavier::isolate_test_process!();

#[tokio::test]
#[serial_test::serial]
#[allow(clippy::await_holding_lock)]
async fn skill_dispatch_ephemeral_lifecycle() {
    let _lock = crate::SUITE_ENV_LOCK.lock().await;
    let root = tempdir().unwrap();
    let _env = RestoreEnv::set_str(&[
        ("XAVIER_EMBEDDING_PROVIDER_MODE", "disabled"),
        ("HOME", root.path().to_str().unwrap()),
        ("XAVIER_CONFIG_PATH", root.path().to_str().unwrap()),
        ("XAVIER_RUNTIME_STATE_PATH", root.path().to_str().unwrap()),
        ("XAVIER_DATA_DIR", root.path().to_str().unwrap()),
    ]);
    let ws1 = root.path().join("ws1");
    let ws2 = root.path().join("ws2");
    let state_dir = root.path().join("state");
    let skills_dir = root.path().join("skills");

    fs::create_dir_all(&ws1).unwrap();
    fs::create_dir_all(&ws2).unwrap();
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(skills_dir.join("skill_a")).unwrap();
    fs::create_dir_all(skills_dir.join("skill_b")).unwrap();

    fs::write(
        skills_dir.join("skill_a/SKILL.md"),
        "---
name: skill-a
description: Solve the unique target challenge
---
Skill A logic.",
    )
    .unwrap();
    fs::write(
        skills_dir.join("skill_b/SKILL.md"),
        "---
name: skill-b
description: B
---
Skill B logic.",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(vec![skills_dir]);
    registry.reindex().await.unwrap();
    let skill_a_hash = registry
        .get("skill-a")
        .expect("skill-a indexed")
        .content_hash
        .clone();
    assert!(
        skill_a_hash.len() >= 12 && skill_a_hash.chars().all(|c| c.is_ascii_hexdigit()),
        "registry content hash is hex: {skill_a_hash}"
    );

    let documents = Arc::new(tokio::sync::RwLock::new(vec![]));
    let memory = Arc::new(QmdMemory::new_with_workspace(documents, "ws1"));

    let dispatcher = SkillDispatcher::new(registry, Some(memory));

    let task_text = "solve task for unique-target-xyz using skill-a";
    let max_tokens = 1000;

    let req = SkillDispatchRequest {
        task: task_text.to_string(),
        model_hint: None,
        max_tokens: Some(max_tokens),
        project: None,
    };

    let result = dispatcher.dispatch(&req).await.unwrap();
    assert_eq!(result.skill_name, "skill-a");

    let content = format!(
        "{}\n{}",
        result.context_pack.system_instructions, result.context_pack.composition
    );

    let journal = Journal::new(state_dir.join("journal")).unwrap();
    let out = publish_task_skill_bounded(&journal, &ws1, "task-1", &content, max_tokens).unwrap();

    let published = fs::read_to_string(&out.skill_path).unwrap();
    assert!(
        published.contains("unique-target-xyz"),
        "content derived from task: {}",
        published
    );
    assert!(published.contains("skill-a"), "skill-a name found");
    assert!(
        published.contains("Solve the unique target challenge"),
        "skill-a description found"
    );
    // Provenance: the pack stamps each selected skill with its registry content
    // hash (first 12 hex chars), never the skill body.
    let stamp = format!("- skill-a ({})", &skill_a_hash[..12]);
    assert!(
        published.contains(&stamp),
        "selected-skill provenance stamp with content hash {stamp:?} missing: {published}"
    );
    assert!(
        !published.contains("Skill A logic"),
        "skill body must not be published in the pack"
    );

    assert!(
        estimate_tokens(&published) <= max_tokens,
        "respects token bound"
    );

    let lease_path = out.skill_path.parent().unwrap().join(".lease");
    let lease_str = fs::read_to_string(&lease_path).unwrap();
    let lease_json: serde_json::Value = serde_json::from_str(&lease_str).unwrap();
    assert!(lease_json.get("tx_id").is_some(), "provenance fields set");
    assert!(
        lease_json.get("expires_at").is_some(),
        "provenance fields set"
    );

    assert!(
        !ws2.join(".agents/skills/eph-task-1").exists(),
        "second ws does not see lease"
    );

    release_task(&journal, &ws1, "task-1").unwrap();
    assert!(!out.skill_path.exists(), "lease removed");
    assert!(!lease_path.exists(), "lease file removed");
    assert!(
        !lease_path.parent().unwrap().exists(),
        "owned directory removed"
    );
    assert!(
        std::fs::symlink_metadata(&out.link_path).is_err(),
        "discovery symlink absent"
    );
}
