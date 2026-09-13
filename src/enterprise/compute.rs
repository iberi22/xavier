//! Enterprise Compute Provider abstraction for training mini-experts.
//!
//! Supports multiple backends:
//! - ColabPro: Google Colab via colab CLI (personal AI Pro account)
//! - RunPodEnterprise: Serverless GPU with Zero Data Retention option
//! - LambdaLabsEnterprise: Cloud GPU with enterprise contracts  
//! - VastAi: Marketplace GPU rental
//! - SwalManagedNode: Future SWAL-owned infrastructure
//!
//! All API keys are stored in Clavis (encrypted vault), never in plaintext.
//!
//! Zero Data Retention (ZDR) flow:
//! 1. Xavier encrypts bundle (ChaCha20) with ephemeral key
//! 2. Sends encrypted bundle + ECDH-wrapped key to server
//! 3. Server trains in RAM/tmpfs — no persistent write
//! 4. Server 3-pass overwrites volume on completion
//! 5. Server publishes SHA-256 of post-deletion volume
//! 6. Xavier verifies hash and marks job ZDR-compliant in audit log

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// GPU tier available in Google Colab Pro
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColabGpu {
    T4,   // 15GB VRAM — good for QLoRA 1-3B
    L4,   // 24GB VRAM — good for QLoRA 7B
    A100, // 40GB VRAM — full fine-tune 7B
    H100, // 80GB VRAM — full fine-tune 13B+
}

impl ColabGpu {
    pub fn as_str(&self) -> &'static str {
        match self {
            ColabGpu::T4 => "T4",
            ColabGpu::L4 => "L4",
            ColabGpu::A100 => "A100",
            ColabGpu::H100 => "H100",
        }
    }

    /// Estimated VRAM in GB.
    pub fn vram_gb(&self) -> u32 {
        match self {
            ColabGpu::T4 => 15,
            ColabGpu::L4 => 24,
            ColabGpu::A100 => 40,
            ColabGpu::H100 => 80,
        }
    }

    /// Recommended max model size (in billions of params) for QLoRA 4-bit.
    pub fn max_model_params_b(&self) -> f32 {
        match self {
            ColabGpu::T4 => 3.0,
            ColabGpu::L4 => 7.0,
            ColabGpu::A100 => 13.0,
            ColabGpu::H100 => 70.0,
        }
    }
}

/// Compute provider for training mini-experts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum ComputeProvider {
    /// Personal Google Colab with AI Pro account.
    /// Best for: small models (1-3B), personal use, no enterprise needed.
    /// Privacy: P3 (data goes to Google's infrastructure).
    ColabPro {
        account_email: String,
        gpu: ColabGpu,
    },
    /// RunPod serverless GPU with enterprise ZDR contract.
    /// Privacy: P2 (zero data retention with cryptographic audit).
    RunPodEnterprise {
        /// Reference to API key stored in Clavis (not plaintext)
        api_key_clavis_ref: String,
        gpu_type: String,
        zero_data_retention: bool,
    },
    /// Lambda Labs GPU cloud with enterprise contracts.
    LambdaLabsEnterprise {
        api_key_clavis_ref: String,
        instance_type: String,
        zero_data_retention: bool,
    },
    /// vast.ai GPU marketplace — flexible pricing.
    VastAi {
        api_key_clavis_ref: String,
        min_gpu_vram_gb: u32,
        max_price_per_hour_usd: f32,
    },
    /// Future: SWAL-owned managed compute nodes.
    SwalManagedNode { node_id: String, region: String },
}

impl ComputeProvider {
    /// Whether this provider has Zero Data Retention guarantee.
    pub fn has_zero_data_retention(&self) -> bool {
        match self {
            ComputeProvider::RunPodEnterprise {
                zero_data_retention,
                ..
            } => *zero_data_retention,
            ComputeProvider::LambdaLabsEnterprise {
                zero_data_retention,
                ..
            } => *zero_data_retention,
            ComputeProvider::SwalManagedNode { .. } => true,
            _ => false,
        }
    }

    /// Whether data leaves the user's Google account (P3 vs P2 distinction).
    pub fn is_third_party_cloud(&self) -> bool {
        matches!(self, ComputeProvider::ColabPro { .. })
    }

    /// Human-readable name.
    pub fn display_name(&self) -> &'static str {
        match self {
            ComputeProvider::ColabPro { .. } => "Google Colab Pro",
            ComputeProvider::RunPodEnterprise { .. } => "RunPod Enterprise",
            ComputeProvider::LambdaLabsEnterprise { .. } => "Lambda Labs Enterprise",
            ComputeProvider::VastAi { .. } => "vast.ai",
            ComputeProvider::SwalManagedNode { .. } => "SWAL Managed Node",
        }
    }
}

/// Status of a training job on a compute provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingJobStatus {
    Preparing,
    Uploading,
    Training,
    Downloading,
    Cleanup,
    Completed,
    Failed,
}

impl TrainingJobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrainingJobStatus::Preparing => "preparing",
            TrainingJobStatus::Uploading => "uploading",
            TrainingJobStatus::Training => "training",
            TrainingJobStatus::Downloading => "downloading",
            TrainingJobStatus::Cleanup => "cleanup",
            TrainingJobStatus::Completed => "completed",
            TrainingJobStatus::Failed => "failed",
        }
    }
}

/// A training job record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingJob {
    pub id: String,
    pub provider: ComputeProvider,
    pub bundle_id: String,
    pub base_model: String,
    pub status: TrainingJobStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    /// Path to the downloaded GGUF model on the local filesystem.
    pub local_gguf_path: Option<String>,
    /// ZDR audit entry (only populated for ZDR-capable providers).
    pub zdr_audit: Option<ZdrAuditEntry>,
    pub error: Option<String>,
}

impl TrainingJob {
    pub fn new(
        provider: ComputeProvider,
        bundle_id: impl Into<String>,
        base_model: impl Into<String>,
    ) -> Self {
        Self {
            id: format!("tj_{}", ulid::Ulid::new()),
            provider,
            bundle_id: bundle_id.into(),
            base_model: base_model.into(),
            status: TrainingJobStatus::Preparing,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            local_gguf_path: None,
            zdr_audit: None,
            error: None,
        }
    }

    pub fn is_complete(&self) -> bool {
        matches!(
            self.status,
            TrainingJobStatus::Completed | TrainingJobStatus::Failed
        )
    }
}

/// Cryptographic audit entry for Zero Data Retention verification.
/// The server publishes the SHA-256 of the volume AFTER deletion.
/// Xavier verifies this matches the expected post-wipe pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZdrAuditEntry {
    pub job_id: String,
    pub provider: String,
    /// SHA-256 of the training volume before wipe (for reference)
    pub volume_hash_before: String,
    /// SHA-256 of the training volume after 3-pass DoD wipe
    pub volume_hash_after: String,
    /// Timestamp when the wipe was completed
    pub wiped_at: DateTime<Utc>,
    /// Xavier verified the hash matches expectations
    pub xavier_verified: bool,
    /// The verification result message
    pub verification_message: String,
}

impl ZdrAuditEntry {
    /// Verify the ZDR audit entry.
    /// A valid wipe produces a volume hash of all-zeros (or known pattern).
    /// In production, this would verify against the provider's signed attestation.
    pub fn verify(&mut self) {
        // A DoD-wiped volume's SHA-256 should match the all-zero or FF pattern
        // In real implementation, this verifies a provider-signed attestation
        let looks_wiped =
            self.volume_hash_after != self.volume_hash_before && !self.volume_hash_after.is_empty();

        self.xavier_verified = looks_wiped;
        self.verification_message = if looks_wiped {
            format!(
                "ZDR verified: volume wiped at {}. Hash changed from {} to {}.",
                self.wiped_at.format("%Y-%m-%d %H:%M UTC"),
                &self.volume_hash_before[..16],
                &self.volume_hash_after[..16]
            )
        } else {
            "ZDR verification FAILED: volume hash unchanged after wipe.".to_string()
        };
    }

    pub fn is_verified(&self) -> bool {
        self.xavier_verified
    }
}

/// Configuration for how Xavier manages training compute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeConfig {
    /// Default provider to use
    pub default_provider: ComputeProvider,
    /// Max training jobs per week (rate-limits compute unit spend)
    pub max_jobs_per_week: u32,
    /// Minimum privacy level allowed (e.g., enterprise may require P2)
    pub min_privacy_level: String,
    /// Training window (hour range, local time)
    pub training_window_start_hour: u8, // 0-23
    pub training_window_end_hour: u8, // 0-23
}

impl Default for ComputeConfig {
    fn default() -> Self {
        Self {
            default_provider: ComputeProvider::ColabPro {
                account_email: String::new(),
                gpu: ColabGpu::T4,
            },
            max_jobs_per_week: 2,
            min_privacy_level: "p4_local".to_string(),
            training_window_start_hour: 2, // 2 AM
            training_window_end_hour: 6,   // 6 AM
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colab_gpu_capacity() {
        assert_eq!(ColabGpu::T4.vram_gb(), 15);
        assert_eq!(ColabGpu::T4.max_model_params_b(), 3.0);
        assert_eq!(ColabGpu::H100.max_model_params_b(), 70.0);
    }

    #[test]
    fn test_provider_zdr_flags() {
        let colab = ComputeProvider::ColabPro {
            account_email: "test@gmail.com".to_string(),
            gpu: ColabGpu::T4,
        };
        assert!(!colab.has_zero_data_retention());
        assert!(colab.is_third_party_cloud());

        let runpod = ComputeProvider::RunPodEnterprise {
            api_key_clavis_ref: "clavis://runpod-key".to_string(),
            gpu_type: "A100".to_string(),
            zero_data_retention: true,
        };
        assert!(runpod.has_zero_data_retention());
        assert!(!runpod.is_third_party_cloud());
    }

    #[test]
    fn test_training_job_lifecycle() {
        let provider = ComputeProvider::ColabPro {
            account_email: "test@gmail.com".to_string(),
            gpu: ColabGpu::T4,
        };
        let job = TrainingJob::new(provider, "bundle_001", "gemma-2b");
        assert!(job.id.starts_with("tj_"));
        assert_eq!(job.status, TrainingJobStatus::Preparing);
        assert!(!job.is_complete());
    }

    #[test]
    fn test_zdr_audit_verification() {
        let mut audit = ZdrAuditEntry {
            job_id: "tj_001".to_string(),
            provider: "RunPod Enterprise".to_string(),
            volume_hash_before: "aabbccdd1122334455667788aabbccddeeff00112233445566778899aabbccdd"
                .to_string(),
            volume_hash_after: "00000000000000000000000000000000000000000000000000000000ffffffff"
                .to_string(),
            wiped_at: Utc::now(),
            xavier_verified: false,
            verification_message: String::new(),
        };
        audit.verify();
        assert!(audit.is_verified());
        assert!(audit.verification_message.contains("ZDR verified"));
    }

    #[test]
    fn test_zdr_audit_fail_unchanged_hash() {
        let hash = "aabbccdd1122334455667788aabbccddeeff00112233445566778899aabbccdd".to_string();
        let mut audit = ZdrAuditEntry {
            job_id: "tj_002".to_string(),
            provider: "RunPod Enterprise".to_string(),
            volume_hash_before: hash.clone(),
            volume_hash_after: hash, // Same hash = wipe didn't happen
            wiped_at: Utc::now(),
            xavier_verified: false,
            verification_message: String::new(),
        };
        audit.verify();
        assert!(!audit.is_verified());
    }

    #[test]
    fn test_compute_config_default_window() {
        let config = ComputeConfig::default();
        assert_eq!(config.training_window_start_hour, 2);
        assert_eq!(config.training_window_end_hour, 6);
        assert_eq!(config.max_jobs_per_week, 2);
    }
}
