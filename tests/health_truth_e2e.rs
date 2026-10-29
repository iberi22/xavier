//! feat-health-truth: Unmeasured embedding coverage must not report healthy.

use serial_test::serial;
use xavier::health::{collect_health, CheckStatus};
use xavier::settings::XavierSettings;

#[path = "router_harness/mod.rs"]
mod router_harness;

xavier::isolate_test_process!();

#[tokio::test]
#[serial]
async fn unmeasured_subsystems_report_unknown_or_degraded_not_healthy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("vec-store.sqlite3");
    let _env = router_harness::RestoreEnv::set(&[
        ("XAVIER_MEMORY_VEC_PATH", path.as_path()),
        ("XAVIER_MEMORY_SQLITE_PATH", path.as_path()),
    ]);
    // Avoid probing a real embedding provider; coverage still reads the real store.
    let _embedder_env = router_harness::RestoreEnv::set_str(&[("XAVIER_EMBEDDER", "disabled")]);
    {
        let conn = rusqlite::Connection::open(&path).expect("open empty store");
        conn.execute_batch("CREATE TABLE memory_records (id TEXT PRIMARY KEY, embedding BLOB);")
            .expect("create empty memory schema");
    }

    let mut settings = XavierSettings::default();
    settings.memory.data_dir = tmp.path().to_string_lossy().into_owned();
    settings.memory.vec_path = path.to_string_lossy().into_owned();
    settings.memory.sqlite_path = settings.memory.vec_path.clone();
    settings.memory.file_path = tmp
        .path()
        .join("memory.json")
        .to_string_lossy()
        .into_owned();
    settings.embedding.embedder = "disabled".to_string();

    // This is the public collector used by the HTTP server, not its cached response.
    let health = collect_health(&settings, None).await;
    assert_eq!(health.embedding_coverage.total, 0);
    assert_eq!(health.embedding_coverage.indexed, 0);
    assert_eq!(health.embedding_coverage.percent, 0.0);
    assert_eq!(health.embedding_coverage.status, "unknown");

    let coverage_check = health
        .checks
        .iter()
        .find(|check| check.name == "embedding_coverage")
        .expect("collector must check embedding coverage");
    assert!(matches!(coverage_check.status, CheckStatus::Warn));
    assert_ne!(health.status, "healthy");
    assert!(health
        .degraded_reasons
        .iter()
        .any(|reason| reason == "subsystem:embedding_coverage"));
}
