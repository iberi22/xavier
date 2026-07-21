// SPDX-License-Identifier: MIT OR LICENSE-MESH
use crate::data_commons::maintainer::decrypt_as_maintainer;
use crate::data_commons::telemetry_db::TelemetryDb;
use chrono::{DateTime, Utc};
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct TrainingBundle {
    pub manifest: crate::data_commons::readiness::TrainingBundleManifest,
    pub train_split: Vec<serde_json::Value>,
    pub eval_split: Vec<serde_json::Value>,
    pub audit_summary: AuditSummary,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditSummary {
    pub total_records_found: usize,
    pub included_records: usize,
    pub excluded_records_no_consent: usize,
    pub excluded_records_revoked: usize,
}

pub struct TrainingExporter {
    db_path: std::path::PathBuf,
    schema_version: String,
}

impl TrainingExporter {
    pub fn new(db_path: &Path) -> Self {
        Self {
            db_path: db_path.to_path_buf(),
            schema_version: "1.0.0".to_string(),
        }
    }

    pub fn generate_bundle(
        &self,
        seed: u64,
        eval_ratio: f32,
        _generated_at: Option<DateTime<Utc>>,
    ) -> Result<TrainingBundle, String> {
        if !(0.0..=1.0).contains(&eval_ratio) {
            return Err("eval_ratio must be between 0.0 and 1.0".to_string());
        }

        let db = TelemetryDb::new(&self.db_path).map_err(|e| e.to_string())?;

        // In a real scenario, we might have a list of revoked wallets.
        // For now, we'll assume no revocations or a placeholder.
        let revoked_wallets: BTreeSet<String> = BTreeSet::new();

        let logs = db.get_all_logs().map_err(|e| e.to_string())?;
        let total_records_found = logs.len();

        let mut processed_records = Vec::new();
        let mut anonymized_sources = BTreeSet::new();
        let excluded_no_consent = 0;
        let mut excluded_revoked = 0;

        for (_hash, encrypted_payload, ephemeral_pubkey, wallet, _timestamp) in logs {
            // Check revocation
            if revoked_wallets.contains(&wallet) {
                excluded_revoked += 1;
                continue;
            }

            // In our current TelemetryDb, we only save logs if consent was given (see funnel.rs)
            // But if we had records without consent, we would filter them here.
            // For now, we'll assume they all have consent since they were saved.

            // The telemetry schema calls this column encrypted_dek, but the
            // current ECIES flow stores the ephemeral public key there.
            let mut ephemeral_pubkey_bytes = [0u8; 32];
            if ephemeral_pubkey.len() == 32 {
                ephemeral_pubkey_bytes.copy_from_slice(&ephemeral_pubkey);
            } else {
                continue;
            }

            match decrypt_as_maintainer(&encrypted_payload, &ephemeral_pubkey_bytes) {
                Ok(decrypted_json) => {
                    match serde_json::from_str::<serde_json::Value>(&decrypted_json) {
                        Ok(val) => {
                            processed_records.push(val);
                            anonymized_sources.insert(self.anonymize_id(&wallet, seed));
                        }
                        Err(_) => continue,
                    }
                }
                Err(_) => continue,
            }
        }

        let included_records = processed_records.len();

        // Deterministic split
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        processed_records.shuffle(&mut rng);

        let eval_size = (included_records as f32 * eval_ratio) as usize;
        let eval_split: Vec<serde_json::Value> = processed_records.drain(0..eval_size).collect();
        let train_split = processed_records;

        let mut split_counts = std::collections::HashMap::new();
        split_counts.insert("train".to_string(), train_split.len());
        split_counts.insert("eval".to_string(), eval_split.len());

        let manifest = crate::data_commons::readiness::TrainingBundleManifest {
            version: self.schema_version.clone(),
            usage_policy: "Research and model fine-tuning only".to_string(),
            reproducibility_seed: seed,
            split_counts,
            data_files: vec!["train.jsonl".to_string(), "eval.jsonl".to_string()],
        };

        let audit_summary = AuditSummary {
            total_records_found,
            included_records,
            excluded_records_no_consent: excluded_no_consent,
            excluded_records_revoked: excluded_revoked,
        };

        Ok(TrainingBundle {
            manifest,
            train_split,
            eval_split,
            audit_summary,
        })
    }

    fn anonymize_id(&self, wallet: &str, seed: u64) -> String {
        let mut hasher = Sha256::new();
        hasher.update(wallet.as_bytes());
        hasher.update(seed.to_be_bytes());
        let result = hasher.finalize();
        crate::crypto::hex_encode(result)[0..16].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_commons::maintainer::encrypt_for_maintainer;
    use crate::data_commons::telemetry_db::TelemetryDb;
    use tempfile::NamedTempFile;

    #[test]
    fn test_anonymization_consistency() {
        let exporter = TrainingExporter::new(Path::new("dummy.db"));
        let wallet = "xv1_test_wallet_address";
        let seed = 12345;

        let id1 = exporter.anonymize_id(wallet, seed);
        let id2 = exporter.anonymize_id(wallet, seed);
        let id3 = exporter.anonymize_id(wallet, 54321);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert_eq!(id1.len(), 16);
    }

    #[test]
    fn test_export_logic_with_mock_db() {
        let db_file = NamedTempFile::new().unwrap();
        let db = TelemetryDb::new(db_file.path()).unwrap();

        // Add some mock logs
        for i in 0..10 {
            let payload = serde_json::json!({"event": format!("test_{}", i), "value": i});
            let payload_str = serde_json::to_string(&payload).unwrap();
            let (encrypted, ephemeral_pub) = encrypt_for_maintainer(&payload_str).unwrap();
            let maintainer_pub =
                crate::data_commons::maintainer::get_maintainer_public_key().to_bytes();

            db.save_encrypted_log(
                &format!("hash_{}", i),
                &encrypted,
                &ephemeral_pub,
                &maintainer_pub,
                "xv1_test_wallet",
            )
            .unwrap();
        }

        let exporter = TrainingExporter::new(db_file.path());
        let seed = 42;
        let now = Utc::now();
        let bundle = exporter.generate_bundle(seed, 0.2, Some(now)).unwrap();

        assert_eq!(bundle.audit_summary.total_records_found, 10);
        assert_eq!(bundle.audit_summary.included_records, 10);
        assert_eq!(bundle.eval_split.len(), 2);
        assert_eq!(bundle.train_split.len(), 8);
        assert_eq!(bundle.manifest.reproducibility_seed, seed);
    }

    #[test]
    fn test_deterministic_split() {
        let db_file = NamedTempFile::new().unwrap();
        let db = TelemetryDb::new(db_file.path()).unwrap();

        for i in 0..20 {
            let payload = serde_json::json!({"i": i});
            let (encrypted, ephemeral_pub) =
                encrypt_for_maintainer(&serde_json::to_string(&payload).unwrap()).unwrap();
            db.save_encrypted_log(
                &format!("h_{}", i),
                &encrypted,
                &ephemeral_pub,
                &[0u8; 32],
                "w",
            )
            .unwrap();
        }

        let exporter = TrainingExporter::new(db_file.path());
        let seed = 99;
        let now = Utc::now();

        let bundle1 = exporter.generate_bundle(seed, 0.2, Some(now)).unwrap();
        let bundle2 = exporter.generate_bundle(seed, 0.2, Some(now)).unwrap();
        let bundle3 = exporter.generate_bundle(100, 0.2, Some(now)).unwrap();

        // Same seed should produce same split
        assert_eq!(bundle1.train_split, bundle2.train_split);
        assert_eq!(bundle1.eval_split, bundle2.eval_split);

        // Different seed should produce different split (highly likely)
        assert_ne!(bundle1.train_split, bundle3.train_split);
    }
}
