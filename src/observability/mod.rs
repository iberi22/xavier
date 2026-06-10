//! # Observability Module
//!
//! Complete observability system for Xavier: logging, error tracking,
//! pattern detection, automated analysis, and self-healing.
//!
//! ## Architecture
//!
//! ```text
//! HTTP Request → [Middleware] → Handler
//!                    │
//!                    ▼
//!            [service_log DB] ← (SQLite + FTS5)
//!                    │
//!              [Detector] ← (cron: every 5 min)
//!                    │
//!              [Analyzer] ← (AgentRuntime + codebase scan)
//!                    │
//!         ┌──────────┼──────────┐
//!         ▼          ▼          ▼
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
pub mod middleware;
pub mod notifier;
pub mod service_log;

pub use analyzer::ErrorAnalyzer;
pub use detector::LogDetector;
pub use fixer::Fixer;
pub use middleware::{request_logger, ObservabilityState};
pub use notifier::Notifier;
pub use service_log::{LogEntry, LogLevel, LogSource, ServiceLogStore};

use std::sync::OnceLock;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

/// Global guard for the file logger — must be kept alive for the app's lifetime.
static LOGGER_GUARD: OnceLock<[tracing_appender::non_blocking::WorkerGuard; 1]> = OnceLock::new();

/// Initialize the tracing subscriber with:
/// - stdout (human-readable, colored, with level + target)
/// - file (JSON-structured, rotativo diario)
/// - env-filter configurable via `XAVIER_LOG` env var or default "info"
///
/// Should be called once at application startup.
pub fn init_logger(log_dir: &std::path::Path, level: &str) {
    if LOGGER_GUARD.get().is_some() {
        tracing::warn!("Logger already initialized, skipping");
        return;
    }

    let filter = EnvFilter::try_from_env("XAVIER_LOG").unwrap_or_else(|_| EnvFilter::new(level));

    // File appender — rotates daily, max 30 files retained
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
}
