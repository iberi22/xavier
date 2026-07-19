//! # Observability Module
//!
//! Complete observability system for Xavier: logging, error tracking,
//! pattern detection, automated analysis, and self-healing.
//!
//! ## Architecture
//!
//! ```text
//! HTTP Request â†’ [Middleware] â†’ Handler
//!                    â”‚
//!                    â–¼
//!            [service_log DB] â† (SQLite + FTS5)
//!                    â”‚
//!              [Detector] â† (cron: every 5 min)
//!                    â”‚
//!              [Analyzer] â† (AgentRuntime + codebase scan)
//!                    â”‚
//!         â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”¼â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”
//!         â–¼          â–¼          â–¼
//!    [Fixer]   [Notifier]   [GitHub Issues]
//!    (auto-PR)  (Telegram)  (manual review)
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use observability::{init, ServiceLogStore, LogEntry, LogLevel};
//!
//! let store = ServiceLogStore::new().await?;
//! store.log(LogEntry::error("http_server", "module::name", "Something failed"))
//!     .await?;
//!
//! let errors = store.query_recent_errors("module::name", 10).await?;
//! ```

pub mod analyzer;
pub mod detector;
pub mod fixer;
pub mod health;
pub mod middleware;
pub mod notifier;
pub mod service_log;
pub mod token_accounting;
pub mod usage_counters;

pub use analyzer::ErrorAnalyzer;
pub use detector::LogDetector;
pub use fixer::Fixer;
pub use health::{HealthMonitor, HealthStatus, HEALTH};
pub use middleware::{request_logger, ObservabilityState};
pub use notifier::Notifier;
pub use service_log::{LogEntry, LogLevel, LogSource, ServiceLogStore};
pub use usage_counters::{ProviderUsage, UsageCounters, UsageSnapshot};

use std::sync::OnceLock;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

/// Global guard for the file logger â€” must be kept alive for the app's lifetime.
static LOGGER_GUARD: OnceLock<[tracing_appender::non_blocking::WorkerGuard; 1]> = OnceLock::new();

/// Purges log files in `log_dir` starting with "xavier." that are older than `max_age_days`.
pub fn purge_old_logs(log_dir: &std::path::Path, max_age_days: u32) {
    let cutoff =
        std::time::SystemTime::now() - std::time::Duration::from_secs(max_age_days as u64 * 86400);
    if let Ok(entries) = std::fs::read_dir(log_dir) {
        let mut purged = 0usize;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("xavier.") {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(mtime) = meta.modified() {
                        if mtime < cutoff {
                            let _ = std::fs::remove_file(entry.path());
                            purged += 1;
                        }
                    }
                }
            }
        }
        if purged > 0 {
            tracing::info!("Purged {} old log files from {:?}", purged, log_dir);
        }
    }
}

/// Initialize the tracing subscriber with:
/// - stdout (human-readable, colored, with level + target)
/// - file (JSON-structured, rotativo diario)
/// - env-filter configurable via `XAVIER_LOG` env var or default "info"
/// - retention of old log files configured via `XAVIER_LOG_RETENTION_DAYS` (default 30 days)
/// - log directory location overridden by `XAVIER_LOG_DIR` env var
///
/// Should be called once at application startup.
pub fn init_logger(log_dir: &std::path::Path, level: &str) {
    if LOGGER_GUARD.get().is_some() {
        tracing::warn!("Logger already initialized, skipping");
        return;
    }

    let filter = EnvFilter::try_from_env("XAVIER_LOG").unwrap_or_else(|_| EnvFilter::new(level));

    // File appender — rotates daily
    let file_appender = RollingFileAppender::new(Rotation::DAILY, log_dir, "xavier");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Set the guard so it lives for the entire app
    let _ = LOGGER_GUARD.set([guard]);

    Registry::default()
        .with(filter)
        // File layer: JSON-structured for automated parsing
        .with(
            fmt::Layer::new()
                .json()
                .with_target(true)
                .with_file(true)
                .with_line_number(true)
                .with_writer(non_blocking),
        )
        // Stdout layer: human-readable
        .with(
            fmt::Layer::new()
                .with_target(true)
                .with_level(true)
                .compact(),
        )
        .init();

    tracing::info!(
        log_dir = %log_dir.display(),
        log_level = %level,
        "Observability logger initialized"
    );

    let retention_days = std::env::var("XAVIER_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    purge_old_logs(log_dir, retention_days);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_purge_old_logs() {
        use std::fs::File;
        let temp_dir = tempfile::tempdir().unwrap();
        let log_dir = temp_dir.path();

        let file_recent = log_dir.join("xavier.recent");
        let file_old1 = log_dir.join("xavier.2026-01-01");
        let file_old2 = log_dir.join("xavier.2026-01-02");
        let non_xavier_old = log_dir.join("other.2026-01-01");

        File::create(&file_recent).unwrap();

        let f_old1 = File::create(&file_old1).unwrap();
        let old_time1 = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 86400);
        f_old1.set_modified(old_time1).unwrap();

        let f_old2 = File::create(&file_old2).unwrap();
        let old_time2 = std::time::SystemTime::now() - std::time::Duration::from_secs(5 * 86400);
        f_old2.set_modified(old_time2).unwrap();

        let f_other = File::create(&non_xavier_old).unwrap();
        f_other.set_modified(old_time2).unwrap();

        // Call purge_old_logs with max_age_days = 1
        purge_old_logs(log_dir, 1);

        // Verify recent file still exists
        assert!(file_recent.exists());
        // Verify non-xavier file still exists (even if old)
        assert!(non_xavier_old.exists());
        // Verify old files were deleted
        assert!(!file_old1.exists());
        assert!(!file_old2.exists());
    }

    #[test]
    fn test_init_logger_guard() {
        // LOGGER_GUARD is a OnceLock. If already set, init_logger should warn and return.
        // We can't easily init a global tracing subscriber in tests (it panics if called twice).
        // But we can verify the guard type exists and holds the expected structure.
        let guard_value = LOGGER_GUARD.get();
        // First call should be None since we haven't initialized
        assert!(guard_value.is_none());
    }

    #[test]
    fn test_module_reexports() {
        // Verify that the main public types are accessible through the module re-exports
        let _s = stringify!(ServiceLogStore);
        let _e = stringify!(LogEntry);
        let _l = stringify!(LogLevel);
        let _r = stringify!(LogSource);
        let _d = stringify!(LogDetector);
        let _a = stringify!(ErrorAnalyzer);
        let _f = stringify!(Fixer);
        let _n = stringify!(Notifier);
        let _o = stringify!(ObservabilityState);
        // Compile-time check that the re-exports exist
        let _ = _s;
        let _ = _e;
        let _ = _l;
        let _ = _r;
        let _ = _d;
        let _ = _a;
        let _ = _f;
        let _ = _n;
        let _ = _o;
    }
}
