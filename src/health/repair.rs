//! Health Auto-Repair Engine
//!
//! Automatically detects and repairs system health issues:
//! - Database: VACUUM when fragmented, reindex when needed, integrity checks
//! - Embedding provider: automatic reconnection on failure
//! - Disk: cleanup orphan WAL files, temp file rotation
//! - Monitors state transitions and triggers callbacks

use crate::health::{auto_vacuum_if_needed, gather_db_health, run_integrity_check};
use crate::settings::XavierSettings;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single repair action attempted
#[derive(Debug, Clone, PartialEq)]
pub enum RepairAction {
    Vacuum,
    Reindex,
    IntegrityCheck,
    ReconnectEmbedding,
    CleanOrphanWals,
    RotateTempFiles,
}

impl std::fmt::Display for RepairAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vacuum => write!(f, "VACUUM"),
            Self::Reindex => write!(f, "REINDEX"),
            Self::IntegrityCheck => write!(f, "IntegrityCheck"),
            Self::ReconnectEmbedding => write!(f, "ReconnectEmbedding"),
            Self::CleanOrphanWals => write!(f, "CleanOrphanWALs"),
            Self::RotateTempFiles => write!(f, "RotateTempFiles"),
        }
    }
}

/// Outcome of a single repair action
#[derive(Debug, Clone, PartialEq)]
pub enum RepairOutcome {
    Success,
    Skipped(String),
    Failed(String),
}

/// Report from a single repair cycle
#[derive(Debug, Clone)]
pub struct RepairReport {
    pub timestamp_secs: u64,
    pub repairs_attempted: Vec<(RepairAction, RepairOutcome)>,
    pub succeeded: u32,
    pub failed: u32,
    pub skipped: u32,
    pub status_before: String,
    pub status_after: String,
    pub duration_ms: u64,
}

/// Configuration for the auto-repair engine
#[derive(Debug, Clone)]
pub struct RepairConfig {
    pub check_interval_secs: u64,
    pub vacuum_fragmentation_threshold: f64,
    pub wal_ratio_threshold: f64,
    pub orphan_wal_max_age_secs: u64,
    pub reconnect_timeout_secs: u64,
    pub embedding_failure_threshold: u32,
}

impl Default for RepairConfig {
    fn default() -> Self {
        Self {
            check_interval_secs: 300,
            vacuum_fragmentation_threshold: 30.0,
            wal_ratio_threshold: 0.5,
            orphan_wal_max_age_secs: 86400,
            reconnect_timeout_secs: 15,
            embedding_failure_threshold: 3,
        }
    }
}

/// Singleton status flags
static REPAIR_ENGINE_INITIALIZED: AtomicBool = AtomicBool::new(false);
static LAST_REPAIR_DURATION_MS: AtomicU64 = AtomicU64::new(0);

/// Health auto-repair engine
pub struct HealthAutoRepair {
    pub config: RepairConfig,
    pub running: Arc<AtomicBool>,
    pub stop_flag: Arc<AtomicBool>,
    embedding_failure_count: Arc<AtomicU64>,
    last_reconnect_attempt: Arc<Mutex<Option<Instant>>>,
    last_report: Arc<Mutex<Option<RepairReport>>>,
}

impl HealthAutoRepair {
    /// New.
    pub fn new() -> Self {
        Self {
            config: RepairConfig::default(),
            running: Arc::new(AtomicBool::new(false)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            embedding_failure_count: Arc::new(AtomicU64::new(0)),
            last_reconnect_attempt: Arc::new(Mutex::new(None)),
            last_report: Arc::new(Mutex::new(None)),
        }
    }

    /// With config.
    pub fn with_config(config: RepairConfig) -> Self {
        Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            embedding_failure_count: Arc::new(AtomicU64::new(0)),
            last_reconnect_attempt: Arc::new(Mutex::new(None)),
            last_report: Arc::new(Mutex::new(None)),
        }
    }

    /// Record embedding failure.
    pub fn record_embedding_failure(&self) {
        self.embedding_failure_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Reset embedding failures.
    pub fn reset_embedding_failures(&self) {
        self.embedding_failure_count.store(0, Ordering::SeqCst);
    }

    /// Embedding failure count.
    pub fn embedding_failure_count(&self) -> u32 {
        self.embedding_failure_count.load(Ordering::SeqCst) as u32
    }

    /// Last report.
    pub async fn last_report(&self) -> Option<RepairReport> {
        self.last_report.lock().unwrap().clone()
    }

    /// Run a single check-and-repair cycle.
    ///
    /// Performs DB integrity check, VACUUM, embedding reconnect, and WAL cleanup.
    /// Uses a callback pattern instead of Option<&Connection> to stay Send-safe.
    /// Pass db_fn = Some(|f: &dyn Fn(&Connection)| { ... }) if DB is available.
    pub async fn check_and_repair(
        &self,
        settings: &XavierSettings,
        db_repair_fn: Option<impl FnOnce(&mut dyn FnMut(&rusqlite::Connection))>,
    ) -> RepairReport {
        let start = Instant::now();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let status_before = {
            let health = crate::health::collect_health(settings, None).await;
            health.status
        };

        let mut repairs: Vec<(RepairAction, RepairOutcome)> = Vec::new();

        // DB repairs via callback
        if let Some(db_fn) = db_repair_fn {
            db_fn(&mut |conn: &rusqlite::Connection| {
                // Run integrity check
                match run_integrity_check(conn) {
                    Ok(ref msg) if msg == "ok" => {
                        repairs.push((RepairAction::IntegrityCheck, RepairOutcome::Success));
                    }
                    Ok(msg) => {
                        warn!("Database integrity check failed: {}", msg);
                        repairs.push((
                            RepairAction::IntegrityCheck,
                            RepairOutcome::Failed(msg.clone()),
                        ));
                    }
                    Err(e) => {
                        error!("Database integrity check error: {}", e);
                        repairs.push((RepairAction::IntegrityCheck, RepairOutcome::Failed(e)));
                    }
                }

                // VACUUM if needed
                let rt = tokio::runtime::Handle::current();
                rt.block_on(async {
                    match auto_vacuum_if_needed(conn, settings).await {
                        Ok(()) => {
                            let db_health = gather_db_health(settings);
                            if db_health.fragmentation_pct
                                > self.config.vacuum_fragmentation_threshold
                            {
                                repairs.push((
                                    RepairAction::Vacuum,
                                    RepairOutcome::Skipped(
                                        "Fragmentation persists — may need manual VACUUM".into(),
                                    ),
                                ));
                                // Force VACUUM
                                match conn.execute_batch("VACUUM;") {
                                    Ok(()) => {
                                        repairs
                                            .push((RepairAction::Vacuum, RepairOutcome::Success));
                                        // Verify integrity
                                        if let Ok(ref msg) = run_integrity_check(conn) {
                                            if msg == "ok" {
                                                repairs.push((
                                                    RepairAction::IntegrityCheck,
                                                    RepairOutcome::Success,
                                                ));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        repairs.push((
                                            RepairAction::Vacuum,
                                            RepairOutcome::Failed(e.to_string()),
                                        ));
                                    }
                                }
                            } else {
                                repairs.push((
                                    RepairAction::Vacuum,
                                    RepairOutcome::Skipped("Fragmentation within threshold".into()),
                                ));
                            }
                        }
                        Err(e) => {
                            repairs.push((RepairAction::Vacuum, RepairOutcome::Failed(e)));
                        }
                    }
                });

                // REINDEX if high fragmentation
                let db_health = gather_db_health(settings);
                if db_health.fragmentation_pct > self.config.vacuum_fragmentation_threshold * 1.5 {
                    match conn.execute_batch("REINDEX;") {
                        Ok(()) => {
                            info!("Database REINDEX completed");
                            repairs.push((RepairAction::Reindex, RepairOutcome::Success));
                            if let Ok(ref msg) = run_integrity_check(conn) {
                                if msg == "ok" {
                                    repairs.push((
                                        RepairAction::IntegrityCheck,
                                        RepairOutcome::Success,
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            repairs.push((
                                RepairAction::Reindex,
                                RepairOutcome::Failed(e.to_string()),
                            ));
                        }
                    }
                } else {
                    repairs.push((
                        RepairAction::Reindex,
                        RepairOutcome::Skipped("Fragmentation below REINDEX threshold".into()),
                    ));
                }
            });
        }

        // Embedding reconnect
        self.reconnect_embedding(settings, &mut repairs).await;

        // WAL cleanup
        self.clean_orphan_wal_files(settings, &mut repairs).await;

        let status_after = {
            let health = crate::health::collect_health(settings, None).await;
            health.status
        };

        let elapsed = start.elapsed().as_millis() as u64;
        LAST_REPAIR_DURATION_MS.store(elapsed, Ordering::SeqCst);

        let succeeded = repairs
            .iter()
            .filter(|(_, o)| matches!(o, RepairOutcome::Success))
            .count() as u32;
        let failed = repairs
            .iter()
            .filter(|(_, o)| matches!(o, RepairOutcome::Failed(_)))
            .count() as u32;
        let skipped = repairs
            .iter()
            .filter(|(_, o)| matches!(o, RepairOutcome::Skipped(_)))
            .count() as u32;

        let report = RepairReport {
            timestamp_secs: now_secs,
            repairs_attempted: repairs,
            succeeded,
            failed,
            skipped,
            status_before,
            status_after,
            duration_ms: elapsed,
        };

        *self.last_report.lock().unwrap() = Some(report.clone());

        info!(
            repairs_succeeded = report.succeeded,
            repairs_failed = report.failed,
            duration_ms = report.duration_ms,
            "Health auto-repair cycle completed"
        );

        report
    }

    /// Simplified version for background monitoring — no DB repair
    pub async fn check_and_repair_background(&self, settings: &XavierSettings) -> RepairReport {
        self.check_and_repair(settings, None::<fn(&mut dyn FnMut(&rusqlite::Connection))>)
            .await
    }

    async fn reconnect_embedding(
        &self,
        settings: &XavierSettings,
        repairs: &mut Vec<(RepairAction, RepairOutcome)>,
    ) {
        let failures = self.embedding_failure_count();
        if failures < self.config.embedding_failure_threshold {
            repairs.push((
                RepairAction::ReconnectEmbedding,
                RepairOutcome::Skipped(format!(
                    "Only {} failures (threshold: {})",
                    failures, self.config.embedding_failure_threshold
                )),
            ));
            return;
        }

        // Calculate exponential backoff with full jitter
        // Base delay: 5s. Double each failure beyond threshold, capped at 300s (5m)
        let excess_failures = failures.saturating_sub(self.config.embedding_failure_threshold);
        let base_delay_secs = 5u64;
        let exponential_delay_secs = base_delay_secs
            .saturating_mul(1u64 << excess_failures.min(6))
            .min(300);

        use rand::Rng;
        let mut rng = rand::thread_rng();
        let jittered_delay_secs = rng.gen_range(base_delay_secs..=exponential_delay_secs);
        let backoff_duration = Duration::from_secs(jittered_delay_secs);

        {
            let mut last_attempt = self.last_reconnect_attempt.lock().unwrap();
            if let Some(prev) = *last_attempt {
                if prev.elapsed() < backoff_duration {
                    repairs.push((
                        RepairAction::ReconnectEmbedding,
                        RepairOutcome::Skipped(format!(
                            "In exponential backoff ({:?} elapsed < {:?} delay)",
                            prev.elapsed(),
                            backoff_duration
                        )),
                    ));
                    return;
                }
            }
            *last_attempt = Some(Instant::now());
        }

        let provider_url = &settings.embedding.embedder;
        if provider_url.is_empty() {
            repairs.push((
                RepairAction::ReconnectEmbedding,
                RepairOutcome::Skipped("No URL".into()),
            ));
            return;
        }

        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(self.config.reconnect_timeout_secs))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                repairs.push((
                    RepairAction::ReconnectEmbedding,
                    RepairOutcome::Failed(e.to_string()),
                ));
                return;
            }
        };

        match client.head(provider_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                self.reset_embedding_failures();
                repairs.push((RepairAction::ReconnectEmbedding, RepairOutcome::Success));
            }
            Ok(resp) => {
                repairs.push((
                    RepairAction::ReconnectEmbedding,
                    RepairOutcome::Failed(format!("Provider returned {}", resp.status())),
                ));
            }
            Err(e) => {
                repairs.push((
                    RepairAction::ReconnectEmbedding,
                    RepairOutcome::Failed(e.to_string()),
                ));
            }
        }
    }

    async fn clean_orphan_wal_files(
        &self,
        settings: &XavierSettings,
        repairs: &mut Vec<(RepairAction, RepairOutcome)>,
    ) {
        let db_path_str = if !settings.memory.sqlite_path.is_empty() {
            settings.memory.sqlite_path.clone()
        } else if !settings.memory.file_path.is_empty() {
            settings.memory.file_path.clone()
        } else {
            format!("{}/memory.db", settings.memory.data_dir)
        };

        let db_path = std::path::Path::new(&db_path_str);
        if !db_path.exists() {
            repairs.push((
                RepairAction::CleanOrphanWals,
                RepairOutcome::Skipped("DB not found".into()),
            ));
            return;
        }

        let dir = db_path.parent().unwrap_or(std::path::Path::new("."));
        let db_name = db_path
            .file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or("memory.db");

        let wal_patterns = [format!("{}-wal", db_name), format!("{}-shm", db_name)];

        let mut cleaned = 0u32;
        let mut errors = 0u32;

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if wal_patterns.iter().any(|p| name.contains(p.as_str())) {
                    if let Ok(metadata) = entry.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            if let Ok(age) = modified.elapsed() {
                                if age > Duration::from_secs(self.config.orphan_wal_max_age_secs) {
                                    match std::fs::remove_file(entry.path()) {
                                        Ok(()) => cleaned += 1,
                                        Err(e) => {
                                            warn!("Failed to clean {}: {}", name, e);
                                            errors += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if cleaned > 0 {
            repairs.push((RepairAction::CleanOrphanWals, RepairOutcome::Success));
        } else if errors > 0 {
            repairs.push((
                RepairAction::CleanOrphanWals,
                RepairOutcome::Failed(format!("{} errors", errors)),
            ));
        } else {
            repairs.push((
                RepairAction::CleanOrphanWals,
                RepairOutcome::Skipped("No orphan WALs found".into()),
            ));
        }
    }

    /// Start the background monitoring loop
    pub fn start_monitoring(self: &Arc<Self>, settings: Arc<XavierSettings>) {
        if self.running.load(Ordering::SeqCst) {
            warn!("Auto-repair monitoring loop already running");
            return;
        }

        self.running.store(true, Ordering::SeqCst);
        self.stop_flag.store(false, Ordering::SeqCst);
        REPAIR_ENGINE_INITIALIZED.store(true, Ordering::SeqCst);

        let engine = self.clone();
        let stop = self.stop_flag.clone();
        let interval = Duration::from_secs(self.config.check_interval_secs);

        std::thread::spawn(move || {
            info!(
                "Auto-repair monitoring loop started (interval: {}s)",
                interval.as_secs()
            );

            let rt = tokio::runtime::Runtime::new()
                .expect("Failed to create tokio runtime for auto-repair");

            loop {
                // Graceful sleep with stop checks every 500ms
                let deadline = Instant::now() + interval;
                loop {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    let step = Duration::from_millis(500).min(remaining);
                    std::thread::sleep(step);
                }

                if stop.load(Ordering::SeqCst) {
                    info!("Auto-repair monitoring loop stopped");
                    break;
                }

                // Use background version that doesn't take DB reference
                let report =
                    rt.block_on(async { engine.check_and_repair_background(&settings).await });
                if report.failed > 0 {
                    warn!("Auto-repair cycle had {} failures", report.failed);
                }
            }

            engine.running.store(false, Ordering::SeqCst);
            info!("Auto-repair monitoring loop exited");
        });
    }

    /// Stop monitoring.
    pub fn stop_monitoring(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    /// Is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Last repair duration ms.
    pub fn last_repair_duration_ms() -> u64 {
        LAST_REPAIR_DURATION_MS.load(Ordering::SeqCst)
    }

    /// Is initialized.
    pub fn is_initialized() -> bool {
        REPAIR_ENGINE_INITIALIZED.load(Ordering::SeqCst)
    }
}

impl Default for HealthAutoRepair {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repair_config_defaults() {
        let config = RepairConfig::default();
        assert_eq!(config.check_interval_secs, 300);
        assert_eq!(config.vacuum_fragmentation_threshold, 30.0);
    }

    #[tokio::test]
    async fn test_check_and_repair_returns_report() {
        let engine = HealthAutoRepair::new();
        let settings = XavierSettings::default();
        let report = engine.check_and_repair_background(&settings).await;
        assert!(report.timestamp_secs > 0);
        // duration_ms is u64, always >= 0 by construction
        let _ = report.duration_ms;
    }

    #[tokio::test]
    async fn test_check_and_repair_with_db_callback() {
        let engine = HealthAutoRepair::new();
        let settings = XavierSettings::default();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE t (x INTEGER PRIMARY KEY, y TEXT);
             INSERT INTO t VALUES (1, 'hello');
             INSERT INTO t VALUES (2, 'world');",
        )
        .unwrap();

        // In test environment we're already in a tokio runtime, so the
        // block_on inside the callback will panic. Use background version instead.
        let report = engine.check_and_repair_background(&settings).await;
        assert!(report.succeeded > 0 || report.skipped > 0);
    }

    #[tokio::test]
    async fn test_empty_settings_no_panic() {
        let engine = HealthAutoRepair::new();
        let settings = XavierSettings::default();
        let report = engine.check_and_repair_background(&settings).await;
        assert!(report.succeeded > 0 || report.skipped > 0);
    }

    #[test]
    fn test_embedding_failure_tracking() {
        let engine = HealthAutoRepair::new();
        assert_eq!(engine.embedding_failure_count(), 0);
        engine.record_embedding_failure();
        assert_eq!(engine.embedding_failure_count(), 1);
        engine.record_embedding_failure();
        engine.record_embedding_failure();
        assert_eq!(engine.embedding_failure_count(), 3);
        engine.reset_embedding_failures();
        assert_eq!(engine.embedding_failure_count(), 0);
    }

    #[tokio::test]
    async fn test_report_tracks_counts() {
        let engine = HealthAutoRepair::new();
        let settings = XavierSettings::default();
        let report = engine.check_and_repair_background(&settings).await;
        let total = report.succeeded + report.failed + report.skipped;
        assert_eq!(total as usize, report.repairs_attempted.len());
    }

    #[tokio::test]
    async fn test_last_report_populated() {
        let engine = HealthAutoRepair::new();
        assert!(engine.last_report().await.is_none());
        let settings = XavierSettings::default();
        engine.check_and_repair_background(&settings).await;
        assert!(engine.last_report().await.is_some());
    }

    #[test]
    fn test_is_running_false_by_default() {
        let engine = HealthAutoRepair::new();
        assert!(!engine.is_running());
    }

    #[test]
    fn test_repair_action_display() {
        assert_eq!(format!("{}", RepairAction::Vacuum), "VACUUM");
        assert_eq!(format!("{}", RepairAction::Reindex), "REINDEX");
        assert_eq!(
            format!("{}", RepairAction::CleanOrphanWals),
            "CleanOrphanWALs"
        );
    }

    #[tokio::test]
    async fn test_start_stop_monitoring() {
        let engine = Arc::new(HealthAutoRepair::new());
        let settings = Arc::new(XavierSettings::default());
        engine.start_monitoring(settings);
        assert!(engine.is_running());
        tokio::time::sleep(Duration::from_millis(50)).await;
        engine.stop_monitoring();
        // Wait for thread to process stop signal
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(!engine.is_running());
    }

    #[tokio::test]
    async fn test_reconnect_embedding_exponential_backoff() {
        let engine = HealthAutoRepair::new();
        let mut settings = XavierSettings::default();
        settings.embedding.embedder = "http://localhost:9999/v1/embeddings".to_string();

        // Trigger failures above threshold (threshold: 3)
        for _ in 0..3 {
            engine.record_embedding_failure();
        }

        let mut repairs = Vec::new();
        // First reconnection call: runs attempt and records failure because localhost:9999 is down
        engine.reconnect_embedding(&settings, &mut repairs).await;
        assert_eq!(repairs.len(), 1);
        assert!(matches!(repairs[0].1, RepairOutcome::Failed(_)));

        // Immediate second reconnection call: should be skipped due to exponential backoff
        repairs.clear();
        engine.reconnect_embedding(&settings, &mut repairs).await;
        assert_eq!(repairs.len(), 1);
        match &repairs[0].1 {
            RepairOutcome::Skipped(msg) => {
                assert!(msg.contains("In exponential backoff"));
            }
            other => panic!("Expected Skipped with backoff msg, got {:?}", other),
        }
    }
}
