//! Telemetry source: decrypts Data Commons telemetry logs (original exporter behavior).

use super::{SourceBatch, SourceContext, TrainingSource};
use crate::data_commons::maintainer::decrypt_as_maintainer;
use crate::data_commons::telemetry_db::TelemetryDb;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub struct TelemetrySource {
    db_path: PathBuf,
}

impl TelemetrySource {
    pub fn new(db_path: &Path) -> Self {
        Self {
            db_path: db_path.to_path_buf(),
        }
    }

    /// Hash a wallet with the seed (SHA-256, first 16 hex chars).
    pub fn anonymize_id(wallet: &str, seed: u64) -> String {
        let mut hasher = Sha256::new();
        hasher.update(wallet.as_bytes());
        hasher.update(seed.to_be_bytes());
        let result = hasher.finalize();
        crate::crypto::hex_encode(result)[0..16].to_string()
    }

    /// Synchronous collection (the DB access is blocking anyway).
    pub fn collect_sync(&self, seed: u64) -> Result<SourceBatch, String> {
        let db = TelemetryDb::new(&self.db_path).map_err(|e| e.to_string())?;

        // No revocation list exists yet; kept as the hook for it.
        let revoked_wallets: BTreeSet<String> = BTreeSet::new();

        let logs = db.get_all_logs().map_err(|e| e.to_string())?;
        let mut batch = SourceBatch {
            total_found: logs.len(),
            ..SourceBatch::default()
        };

        for (_hash, encrypted_payload, ephemeral_pubkey, wallet, _timestamp) in logs {
            if revoked_wallets.contains(&wallet) {
                batch.excluded_revoked += 1;
                continue;
            }
            // Logs are only saved with consent (see funnel.rs), so none lack it here.

            // The telemetry schema calls this column encrypted_dek, but the
            // current ECIES flow stores the ephemeral public key there.
            let mut ephemeral_pubkey_bytes = [0u8; 32];
            if ephemeral_pubkey.len() == 32 {
                ephemeral_pubkey_bytes.copy_from_slice(&ephemeral_pubkey);
            } else {
                continue;
            }

            let Ok(decrypted_json) =
                decrypt_as_maintainer(&encrypted_payload, &ephemeral_pubkey_bytes)
            else {
                continue;
            };
            let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&decrypted_json) else {
                continue;
            };
            if let Some(obj) = val.as_object_mut() {
                if !obj.contains_key("metadata") {
                    obj.insert(
                        "metadata".to_string(),
                        serde_json::json!({
                            "consent_given": true,
                            "is_private": false,
                            "revoked": false
                        }),
                    );
                }
            }
            batch.records.push(val);
            batch
                .anonymized_sources
                .insert(Self::anonymize_id(&wallet, seed));
        }
        Ok(batch)
    }
}

#[async_trait]
impl TrainingSource for TelemetrySource {
    fn name(&self) -> &'static str {
        "telemetry"
    }

    async fn collect(&self, ctx: &SourceContext) -> Result<SourceBatch, String> {
        self.collect_sync(ctx.seed)
    }
}
