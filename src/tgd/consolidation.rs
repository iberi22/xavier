// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Nightly Consolidation Scheduler for TGD and Memory.
//!
//! Manages background execution of memory consolidation and TGD rule generation
//! on a cron-like schedule.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::consolidation::ConsolidationTask;
use crate::tgd::TgdEngine;
use crate::workspace::WorkspaceContext;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProgressReport {
    pub processed: usize,
    pub total: usize,
    pub eta_secs: u64,
    pub errors: usize,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct SchedulerState {
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_duration_ms: u64,
    pub items_processed: usize,
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
            info!(
                "🚀 TGD Consolidation Scheduler started with cron: {}",
                cron_expr
            );

            let schedule: cron::Schedule = match cron::Schedule::from_str(&cron_expr) {
                Ok(s) => s,
                Err(e) => {
                    error!("❌ Invalid cron expression: {}", e);
                    return;
                }
            };

            while let Some(next) = schedule.upcoming(Utc).next() {

                let now = Utc::now();
                if next > now {
                    let sleep_duration = next
                        .signed_duration_since(now)
                        .to_std()
                        .unwrap_or(Duration::from_secs(0));
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

        let task = ConsolidationTask {
            enable_tgd_in_consolidation: true,
            ..Default::default()
        };

        // 1. Run Memory Consolidation
        info!("🧠 Phase 1: Memory Consolidation...");
        let stats = task
            .consolidate(&self.workspace, Some(Arc::clone(&self.progress)))
            .await?;

        // 2. Run TGD if enabled
        info!("🧠 Phase 2: TGD Rule Generation...");
        task.run_tgd_if_enabled(&self.workspace, self.tgd_engine.as_ref())
            .await?;

        // 3. Run TGD Memory Refinement
        info!("🧠 Phase 3: TGD Memory Refinement...");
        let refinement_stats = task
            .run_tgd_memory_refinement(&self.workspace, self.tgd_engine.as_ref())
            .await?;

        let mut final_stats = stats;
        final_stats.selected += refinement_stats.selected;
        final_stats.memories_refined = refinement_stats.memories_refined;
        final_stats.avg_score_improvement = refinement_stats.avg_score_improvement;
        final_stats.errors += refinement_stats.errors;

        let duration = start.elapsed();
        let state = SchedulerState {
            last_run_at: Some(Utc::now()),
            last_duration_ms: duration.as_millis() as u64,
            items_processed: final_stats.selected,
        };

        self.save_state(&state).await?;

        {
            let mut p = self.progress.write().await;
            p.status = "completed".to_string();
            p.processed = final_stats.selected;
            p.errors = final_stats.errors;
        }

        info!(
            "✅ Scheduled consolidation completed in {}ms",
            duration.as_millis()
        );

        // Log results to chronicle (via memory document)
        let date = Utc::now().format("%Y-%m-%d").to_string();
        let report_content = format!(
            "# Nightly TGD Consolidation Report\n\n- Date: {}\n- Duration: {}ms\n- Items Processed: {}\n- Memories Refined: {}\n- Avg Improvement: {:.4}\n- Errors: {}\n",
            date,
            duration.as_millis(),
            final_stats.selected,
            final_stats.memories_refined,
            final_stats.avg_score_improvement,
            final_stats.errors
        );

        let report_path = format!("logs/tgd/report-{}.md", date);
        self.workspace
            .workspace
            .memory
            .add_document_typed(
                report_path,
                report_content,
                serde_json::json!({
                    "memory_kind": "tgd_report",
                    "date": date,
                    "stats": final_stats
                }),
                None,
            )
            .await?;

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

/// Run a standalone nightly TGD consolidation: updates .xavier/tgd.md
/// with new generated rules and runs memory refinement.
pub async fn run_nightly_tgd() -> anyhow::Result<()> {
    use crate::consolidation::ConsolidationTask;
    use crate::workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceState};

    info!("🌙 Running nightly TGD consolidation...");

    // Build a minimal workspace context from env / defaults
    let workspace_id =
        std::env::var("XAVIER_DEFAULT_WORKSPACE_ID").unwrap_or_else(|_| "default".to_string());
    let root = std::env::var("XAVIER_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let workspace_state = WorkspaceState::new(
        WorkspaceConfig {
            id: workspace_id.clone(),
            token: std::env::var("XAVIER_TOKEN").unwrap_or_default(),
            plan: crate::workspace::PlanTier::Personal,
            memory_backend: crate::memory::store::MemoryBackend::Sqlite,
            storage_limit_bytes: None,
            request_limit: None,
            request_unit_limit: None,
            embedding_provider_mode: crate::workspace::EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: crate::workspace::SyncPolicy::CloudMirror,
        },
        crate::agents::RuntimeConfig::default(),
        root.join(".xavier"),
    )
    .await?;
    let workspace = WorkspaceContext {
        workspace_id,
        workspace: std::sync::Arc::new(workspace_state),
    };

    let task = ConsolidationTask {
        enable_tgd_in_consolidation: true,
        ..Default::default()
    };

    // Create a TGD engine from environment config
    let tgd_engine =
        crate::tgd::TgdEngine::new(crate::agents::provider::ModelProviderClient::from_env());

    // Run TGD rule generation
    task.run_tgd_if_enabled(&workspace, Some(&tgd_engine))
        .await?;

    // Run TGD memory refinement
    let stats = task
        .run_tgd_memory_refinement(&workspace, Some(&tgd_engine))
        .await?;

    // Update .xavier/tgd.md with latest status
    let tgd_status_path = root.join(".xavier/tgd.md");
    if let Some(parent) = tgd_status_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let report = format!(
        "# Nightly TGD Report\n\n- Time: {}\n- Memories refined: {}\n- Avg score improvement: {:.4}\n- Errors: {}\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        stats.memories_refined,
        stats.avg_score_improvement,
        stats.errors,
    );
    tokio::fs::write(&tgd_status_path, report).await?;

    info!(
        "✅ Nightly TGD complete — {} memories refined",
        stats.memories_refined
    );
    Ok(())
}
