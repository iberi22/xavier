//! Nightly Consolidation Scheduler for TGD and Memory.
//!
//! Manages background execution of memory consolidation and TGD rule generation
//! on a cron-like schedule.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{info, error};

use crate::consolidation::ConsolidationTask;
use crate::workspace::WorkspaceContext;
use crate::tgd::TgdEngine;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProgressReport {
    pub processed: usize,
    pub total: usize,
    pub eta_secs: u64,
    pub errors: usize,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerState {
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_duration_ms: u64,
    pub items_processed: usize,
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self {
            last_run_at: None,
            last_duration_ms: 0,
            items_processed: 0,
        }
    }
}

pub struct TgdConsolidationScheduler {
    workspace: WorkspaceContext,
    tgd_engine: Option<TgdEngine>,
    progress: Arc<RwLock<ProgressReport>>,
    state_path: PathBuf,
    cancellation_token: CancellationToken,
}

impl TgdConsolidationScheduler {
    pub fn new(
        workspace: WorkspaceContext,
        tgd_engine: Option<TgdEngine>,
        state_path: PathBuf,
    ) -> Self {
        Self {
            workspace,
            tgd_engine,
            progress: Arc::new(RwLock::new(ProgressReport::default())),
            state_path,
            cancellation_token: CancellationToken::new(),
        }
    }

    pub fn progress(&self) -> Arc<RwLock<ProgressReport>> {
        Arc::clone(&self.progress)
    }

    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    pub async fn spawn(self: Arc<Self>, cron_expr: String) {
        let scheduler = Arc::clone(&self);
        tokio::spawn(async move {
            info!("🚀 TGD Consolidation Scheduler started with cron: {}", cron_expr);

            let schedule: cron::Schedule = match cron::Schedule::from_str(&cron_expr) {
                Ok(s) => s,
                Err(e) => {
                    error!("❌ Invalid cron expression: {}", e);
                    return;
                }
            };

            loop {
                let next = match schedule.upcoming(Utc).next() {
                    Some(n) => n,
                    None => break,
                };

                let now = Utc::now();
                if next > now {
                    let sleep_duration = next.signed_duration_since(now).to_std().unwrap_or(Duration::from_secs(0));
                    info!("📅 Next TGD consolidation scheduled for: {}", next);

                    tokio::select! {
                        _ = tokio::time::sleep(sleep_duration) => {},
                        _ = scheduler.cancellation_token.cancelled() => {
                            info!("🛑 TGD consolidation scheduler cancelled");
                            return;
                        }
                    }
                }

                if scheduler.cancellation_token.is_cancelled() {
                    break;
                }

                info!("⚙️ Starting scheduled TGD consolidation...");
                if let Err(e) = scheduler.run_once().await {
                    error!("❌ Scheduled consolidation failed: {}", e);
                }
            }
        });
    }

    pub async fn run_once(&self) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        {
            let mut p = self.progress.write().await;
            *p = ProgressReport {
                status: "running".to_string(),
                ..Default::default()
            };
        }

        let task = ConsolidationTask::default();

        // 1. Run Memory Consolidation
        info!("🧠 Phase 1: Memory Consolidation...");
        let stats = task.consolidate(&self.workspace, Some(Arc::clone(&self.progress))).await?;

        // 2. Run TGD if enabled
        info!("🧠 Phase 2: TGD Rule Generation...");
        task.run_tgd_if_enabled(&self.workspace, self.tgd_engine.as_ref()).await?;

        let duration = start.elapsed();
        let state = SchedulerState {
            last_run_at: Some(Utc::now()),
            last_duration_ms: duration.as_millis() as u64,
            items_processed: stats.selected,
        };

        self.save_state(&state).await?;

        {
            let mut p = self.progress.write().await;
            p.status = "completed".to_string();
            p.processed = stats.selected;
            p.errors = stats.errors;
        }

        info!("✅ Scheduled consolidation completed in {}ms", duration.as_millis());
        Ok(())
    }

    async fn save_state(&self, state: &SchedulerState) -> anyhow::Result<()> {
        if let Some(parent) = self.state_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let data = serde_json::to_vec_pretty(state)?;
        tokio::fs::write(&self.state_path, data).await?;
        Ok(())
    }

    pub async fn load_state(&self) -> anyhow::Result<SchedulerState> {
        if !self.state_path.exists() {
            return Ok(SchedulerState::default());
        }
        let data = tokio::fs::read_to_string(&self.state_path).await?;
        let state = serde_json::from_str(&data)?;
        Ok(state)
    }
}
