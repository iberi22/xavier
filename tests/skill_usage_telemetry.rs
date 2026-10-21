//! Dispatch -> usage summary -> ack -> coverage, on a tempdir database only.

use xavier::api::skills::usage_summary_blocking;
use xavier::context::skill_controller::telemetry::{SkillStage, UsageEvent};
use xavier::context::skill_dispatcher::{
    record_ack, SkillDispatchRequest, SkillDispatcher, SkillUsageError,
};
use xavier::context::skill_registry::SkillRegistry;

xavier::isolate_test_process!();

fn skill_row(summary: &serde_json::Value) -> serde_json::Value {
    summary["skills"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["skill_name"] == "trace-audit"))
        .cloned()
        .expect("trace-audit row")
}

#[tokio::test]
async fn dispatch_then_ack_reports_coverage() {
    let tmp = tempfile::tempdir().unwrap();
    let skills = tmp.path().join("skills");
    std::fs::create_dir(&skills).unwrap();
    std::fs::write(
        skills.join("SKILL.md"),
        "---\nname: trace-audit\ndescription: \"Audit the failed traces of a run\"\n---\n\nContent",
    )
    .unwrap();
    let mut registry = SkillRegistry::new(vec![skills]);
    registry.reindex().await.unwrap();
    let db = tmp.path().join("state").join("usage.sqlite3");
    let dispatcher = SkillDispatcher::new(registry, None).with_telemetry_db(db.clone());
    let result = dispatcher
        .dispatch(&SkillDispatchRequest {
            task: "audit the failed traces".to_string(),
            model_hint: None,
            max_tokens: Some(4000),
            project: Some("ws-test".to_string()),
        })
        .await
        .unwrap();
    assert_eq!(result.skill_name, "trace-audit");

    // Telemetry is emitted on a detached task: wait for it to land.
    let mut row = serde_json::Value::Null;
    for _ in 0..100 {
        if let Ok(summary) = usage_summary_blocking(&db) {
            if summary["skills"]
                .as_array()
                .is_some_and(|rows| !rows.is_empty())
            {
                row = skill_row(&summary);
                if row["delivered"] == 1 {
                    break;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(row["delivered"], 1);
    assert_eq!(row["invocation_coverage"], "unknown");

    let ack = |id: &str, stage| {
        record_ack(
            &db,
            &UsageEvent::new(id, "ws-test", "task-1", "harness", "trace-audit", stage),
        )
    };
    assert_eq!(ack("ack-i", SkillStage::Invoked).unwrap().affected, 1);
    assert_eq!(ack("ack-i", SkillStage::Invoked).unwrap().affected, 0);
    assert_eq!(ack("ack-c", SkillStage::Completed).unwrap().affected, 1);
    assert!(matches!(
        ack("ack-s", SkillStage::Selected),
        Err(SkillUsageError::NotAnAcknowledgment)
    ));

    let row = skill_row(&usage_summary_blocking(&db).unwrap());
    assert_eq!(row["invoked"], 1);
    assert_eq!(row["completed"], 1);
    assert_eq!(row["invocation_coverage"], "observed");
}
