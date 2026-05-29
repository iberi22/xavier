//! # Onboarding Auto-Configuration System
//!
//! Xavier scans the host environment and auto-configures itself optimally.
//!
//! ## Detection Pipeline
//! 1. **System scan** — CPU features, RAM, disk, GPU (CUDA/Vulkan/Metal)
//! 2. **Provider scan** — Probe external embedding providers (OpenAI, Google, Ollama, etc.)
//! 3. **Configuration generation** — Write optimal `xavier.config.json`
//! 4. **User prompt** (optional) — Ask user for API keys if providers detected
//!
//! ## Usage
//! ```rust
//! use onboarding::OnboardingEngine;
//! let engine = OnboardingEngine::new();
//! let report = engine.scan_and_configure().await?;
//! println!("{}", report.summary());
//! ```

pub mod configurator;
pub mod embedding;
pub mod scanner;

use configurator::AutoConfig;
use embedding::AutoEmbedder;
use scanner::{ProviderStatus, SystemCapabilities};

use std::fmt;

/// Full onboarding report
pub struct OnboardingReport {
    pub system: SystemCapabilities,
    pub providers: Vec<ProviderStatus>,
    pub config: AutoConfig,
    pub embedder: String,
}

impl fmt::Display for OnboardingReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "🔍 Xavier Onboarding Report")?;
        writeln!(f, "━━━━━━━━━━━━━━━━━━━━━━━━")?;
        writeln!(f, "{}", self.system)?;
        writeln!(f, "")?;
        writeln!(f, "📡 Providers Detected:")?;
        for p in &self.providers {
            writeln!(f, "  {}: {}", p.name, if p.available { "✅" } else { "❌" })?;
        }
        writeln!(f, "")?;
        writeln!(f, "⚙️  Embedder: {}", self.embedder)?;
        writeln!(f, "📝 Config: {}", self.config.summary())?;
        Ok(())
    }
}

/// Main onboarding engine
pub struct OnboardingEngine {
    pub auto_apply: bool,
}

impl Default for OnboardingEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl OnboardingEngine {
    pub fn new() -> Self {
        Self { auto_apply: false }
    }

    /// Scan system and configure Xavier optimally
    pub async fn scan_and_configure(&self) -> Result<OnboardingReport, String> {
        let system = scanner::scan_system().map_err(|e| format!("Scan failed: {e}"))?;
        let providers = scanner::probe_providers().await;
        let best_embedder = AutoEmbedder::select(&system, &providers);
        let config = configurator::generate_config(&system, &providers, &best_embedder);

        if self.auto_apply {
            configurator::apply_config(&config).map_err(|e| format!("Config apply failed: {e}"))?;
        }

        Ok(OnboardingReport {
            system,
            providers,
            config,
            embedder: best_embedder,
        })
    }
}
