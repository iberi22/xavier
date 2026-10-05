use super::privacy::{
    check_k_anonymity, AnonymizationAudit, AuditCounts, AuditInput, PiiCounts, PrivacyConfig,
    PrivacyLevel, PrivacyPipeline,
};
pub use super::sources::{
    ChallengeSource, MemorySource, SourceBatch, SourceContext, TelemetrySource, TrainingSource,
};
use chrono::{DateTime, Utc};
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct TrainingBundle {
    pub manifest: crate::data_commons::readiness::TrainingBundleManifest,
    pub train_split: Vec<serde_json::Value>,
    pub eval_split: Vec<serde_json::Value>,
    pub audit_summary: AuditSummary,
    /// Written verbatim to `anonymization_audit.json`.
    pub audit: AnonymizationAudit,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditSummary {
    pub total_records_found: usize,
    pub included_records: usize,
    pub excluded_records_no_consent: usize,
    pub excluded_records_revoked: usize,
    #[serde(default)]
    pub excluded_other: BTreeMap<String, usize>,
}

pub struct TrainingExporter {
    db_path: std::path::PathBuf,
    schema_version: String,
    pub is_curated: bool,
    pub privacy_level: PrivacyLevel,
    pub privacy_config: PrivacyConfig,
    pub workspace: Option<String>,
    pub domain: Option<String>,
}

impl TrainingExporter {
    /// New.
    pub fn new(db_path: &Path) -> Self {
        Self {
            db_path: db_path.to_path_buf(),
            schema_version: "1.0.0".to_string(),
            is_curated: false,
            privacy_level: PrivacyLevel::P2,
            privacy_config: PrivacyConfig::default(),
            workspace: None,
            domain: None,
        }
    }

    /// Set whether personal models or exports train ONLY on curated data.
    pub fn with_curated_only(mut self, curated_only: bool) -> Self {
        self.is_curated = curated_only;
        self
    }

    /// Set the privacy level for anonymization and PII scrubbing.
    pub fn with_privacy_level(mut self, level: PrivacyLevel) -> Self {
        self.privacy_level = level;
        self
    }

    /// Set epsilon / sensitivity / k.
    pub fn with_privacy_config(mut self, config: PrivacyConfig) -> Self {
        self.privacy_config = config;
        self
    }

    /// Workspace / domain recorded in the manifest and audit (sources filter on their own).
    pub fn with_scope(mut self, workspace: Option<String>, domain: Option<String>) -> Self {
        self.workspace = workspace;
        self.domain = domain;
        self
    }

    /// Telemetry-only bundle (original behavior).
    pub fn generate_bundle(
        &self,
        seed: u64,
        eval_ratio: f32,
        _generated_at: Option<DateTime<Utc>>,
    ) -> Result<TrainingBundle, String> {
        self.check_params(eval_ratio)?;
        let batch = TelemetrySource::new(&self.db_path).collect_sync(seed)?;
        self.assemble(vec![("telemetry".to_string(), batch)], seed, eval_ratio)
    }

    /// Compose the given sources into one bundle.
    pub async fn generate_bundle_from_sources(
        &self,
        sources: &[Box<dyn TrainingSource>],
        seed: u64,
        eval_ratio: f32,
    ) -> Result<TrainingBundle, String> {
        self.check_params(eval_ratio)?;
        if sources.is_empty() {
            return Err("at least one training source is required".to_string());
        }
        let ctx = SourceContext {
            seed,
            privacy_level: self.privacy_level,
        };
        let mut batches = Vec::new();
        for source in sources {
            let batch = source
                .collect(&ctx)
                .await
                .map_err(|e| format!("source '{}': {}", source.name(), e))?;
            batches.push((source.name().to_string(), batch));
        }
        self.assemble(batches, seed, eval_ratio)
    }

    fn check_params(&self, eval_ratio: f32) -> Result<(), String> {
        if !(0.0..=1.0).contains(&eval_ratio) {
            return Err("eval_ratio must be between 0.0 and 1.0".to_string());
        }
        self.privacy_config.validate()
    }

    fn assemble(
        &self,
        batches: Vec<(String, SourceBatch)>,
        seed: u64,
        eval_ratio: f32,
    ) -> Result<TrainingBundle, String> {
        let mut counts = AuditCounts::default();
        let mut source_names = Vec::new();
        let mut records = Vec::new();
        for (name, batch) in batches {
            source_names.push(name);
            counts.total_found += batch.total_found;
            counts.excluded_no_consent += batch.excluded_no_consent;
            counts.excluded_revoked += batch.excluded_revoked;
            for (reason, n) in batch.excluded_other {
                *counts.excluded_other.entry(reason).or_insert(0) += n;
            }
            records.extend(batch.records);
        }
        let included_records = records.len();
        counts.included = included_records;

        // Privacy pipeline (P4 bypasses scrubbing entirely).
        let pipeline = PrivacyPipeline::with_config(self.privacy_config);
        let mut scrubbed = PiiCounts::default();
        let mut numeric_fields_noised = 0;
        let mut residual = PiiCounts::default();
        let mut processed = if self.privacy_level.is_local_only() {
            records
        } else {
            for r in &records {
                let c = pipeline.count_pii(r);
                scrubbed.add(&c);
                if self.privacy_level == PrivacyLevel::P3 {
                    numeric_fields_noised += PrivacyPipeline::count_numeric_leaves(r);
                }
            }
            pipeline.process_batch(records, self.privacy_level)
        };
        for r in &processed {
            let c = pipeline.count_pii(r);
            residual.add(&c);
        }
        let k_report = check_k_anonymity(&processed, self.privacy_config.k);

        // Deterministic split
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        processed.shuffle(&mut rng);
        let eval_size = (included_records as f32 * eval_ratio) as usize;
        let eval_split: Vec<serde_json::Value> = processed.drain(0..eval_size).collect();
        let train_split = processed;
        counts.train = train_split.len();
        counts.eval = eval_split.len();

        let mut split_counts = std::collections::HashMap::new();
        split_counts.insert("train".to_string(), train_split.len());
        split_counts.insert("eval".to_string(), eval_split.len());

        let manifest = crate::data_commons::readiness::TrainingBundleManifest {
            version: self.schema_version.clone(),
            usage_policy: "Research and model fine-tuning only".to_string(),
            reproducibility_seed: seed,
            split_counts,
            data_files: vec!["train.jsonl".to_string(), "eval.jsonl".to_string()],
            privacy_level: self.privacy_level.as_str().to_string(),
        };

        let audit_summary = AuditSummary {
            total_records_found: counts.total_found,
            included_records,
            excluded_records_no_consent: counts.excluded_no_consent,
            excluded_records_revoked: counts.excluded_revoked,
            excluded_other: counts.excluded_other.clone(),
        };

        let generated_at = Utc::now().to_rfc3339();
        let audit = AnonymizationAudit::evaluate(AuditInput {
            level: self.privacy_level,
            config: self.privacy_config,
            counts,
            scrubbed,
            residual,
            numeric_fields_noised,
            k_report,
            sources: source_names,
            workspace: self.workspace.clone(),
            domain: self.domain.clone(),
            generated_at: &generated_at,
        });

        Ok(TrainingBundle {
            manifest,
            train_split,
            eval_split,
            audit_summary,
            audit,
        })
    }

    #[cfg(test)]
    fn anonymize_id(&self, wallet: &str, seed: u64) -> String {
        TelemetrySource::anonymize_id(wallet, seed)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct DatasetMetadata {
    pub id: String,
    pub size: usize,
    pub clearance: String,
    pub language: String,
    pub segment: String,
}

/// Scan `data_dir` for subdirectories that contain `bundle_manifest.json`.
pub fn scan_datasets(data_dir: &Path) -> Result<Vec<DatasetMetadata>, String> {
    let mut datasets = Vec::new();
    if !data_dir.exists() {
        return Ok(datasets);
    }

    let entries = std::fs::read_dir(data_dir).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            let manifest_path = path.join("bundle_manifest.json");
            if manifest_path.exists() {
                if let Ok(metadata) = load_dataset_metadata(&path) {
                    datasets.push(metadata);
                }
            }
        }
    }
    // Sort datasets by id for deterministic output
    datasets.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(datasets)
}

/// Load a dataset's metadata from its directory.
pub fn load_dataset_metadata(dataset_dir: &Path) -> Result<DatasetMetadata, String> {
    let manifest_path = dataset_dir.join("bundle_manifest.json");
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let val: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    let id = dataset_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Sum train and eval counts from split_counts
    let mut size = 0;
    if let Some(split_counts) = val.get("split_counts").and_then(|v| v.as_object()) {
        for count in split_counts.values() {
            if let Some(c) = count.as_u64() {
                size += c as usize;
            }
        }
    }

    let clearance = val
        .get("clearance")
        .and_then(|v| v.as_str())
        .unwrap_or("INTERNAL")
        .to_string();

    let language = val
        .get("language")
        .and_then(|v| v.as_str())
        .unwrap_or("en")
        .to_string();

    let segment = val
        .get("segment")
        .and_then(|v| v.as_str())
        .unwrap_or("telemetry")
        .to_string();

    Ok(DatasetMetadata {
        id,
        size,
        clearance,
        language,
        segment,
    })
}

/// List datasets from `data_dir` as TrainingBundleManifest list.
pub fn list_datasets(
    data_dir: &Path,
) -> Result<Vec<crate::data_commons::readiness::TrainingBundleManifest>, String> {
    let mut manifests = Vec::new();
    if !data_dir.exists() {
        return Ok(manifests);
    }

    let entries = std::fs::read_dir(data_dir).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            let manifest_path = path.join("bundle_manifest.json");
            if manifest_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                    if let Ok(m) = serde_json::from_str::<
                        crate::data_commons::readiness::TrainingBundleManifest,
                    >(&content)
                    {
                        manifests.push(m);
                    }
                }
            }
        }
    }
    Ok(manifests)
}

/// Get manifest by dataset ID.
pub fn get_manifest(data_dir: &Path, id: &str) -> Result<serde_json::Value, String> {
    let dataset_dir = data_dir.join(id);
    load_dataset_manifest(&dataset_dir)
}

/// Load a dataset's manifest.
pub fn load_dataset_manifest(dataset_dir: &Path) -> Result<serde_json::Value, String> {
    let manifest_path = dataset_dir.join("bundle_manifest.json");
    if !manifest_path.exists() {
        return Err("Manifest not found".to_string());
    }
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let val: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(val)
}

/// Load a dataset's `anonymization_audit.json`.
pub fn load_dataset_audit(dataset_dir: &Path) -> Result<serde_json::Value, String> {
    let audit_path = dataset_dir.join("anonymization_audit.json");
    if !audit_path.exists() {
        return Err("Audit not found".to_string());
    }
    let content = std::fs::read_to_string(&audit_path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

/// Load a dataset's split (train or eval) as JSONL string.
pub fn load_dataset_split(dataset_dir: &Path, split: &str) -> Result<String, String> {
    let file_name = format!("{}.jsonl", split);
    let split_path = dataset_dir.join(file_name);
    if !split_path.exists() {
        return Err(format!("Split {} not found", split));
    }
    std::fs::read_to_string(&split_path).map_err(|e| e.to_string())
}

/// Write a training bundle to disk, adding metadata fields.
pub fn write_bundle_to_dir(
    data_dir: &Path,
    id: &str,
    bundle: &TrainingBundle,
    clearance: Option<String>,
    language: Option<String>,
    segment: Option<String>,
) -> Result<(), String> {
    let dataset_dir = data_dir.join(id);
    std::fs::create_dir_all(&dataset_dir).map_err(|e| e.to_string())?;

    // Add extra metadata fields to manifest
    let mut manifest_val = serde_json::to_value(&bundle.manifest).map_err(|e| e.to_string())?;
    if let Some(obj) = manifest_val.as_object_mut() {
        obj.insert(
            "clearance".to_string(),
            serde_json::json!(clearance.unwrap_or_else(|| "INTERNAL".to_string())),
        );
        obj.insert(
            "language".to_string(),
            serde_json::json!(language.unwrap_or_else(|| "en".to_string())),
        );
        obj.insert(
            "segment".to_string(),
            serde_json::json!(segment.unwrap_or_else(|| "telemetry".to_string())),
        );
        obj.insert(
            "privacy_level".to_string(),
            serde_json::json!(bundle.manifest.privacy_level),
        );
        obj.insert(
            "local_only".to_string(),
            serde_json::json!(bundle.audit.local_only),
        );
        obj.insert(
            "sources".to_string(),
            serde_json::json!(bundle.audit.sources),
        );
        obj.insert(
            "workspace".to_string(),
            serde_json::json!(bundle.audit.workspace),
        );
        obj.insert("domain".to_string(), serde_json::json!(bundle.audit.domain));
    }

    std::fs::write(
        dataset_dir.join("bundle_manifest.json"),
        serde_json::to_string_pretty(&manifest_val).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    std::fs::write(
        dataset_dir.join("anonymization_audit.json"),
        serde_json::to_string_pretty(&bundle.audit).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let mut train_content = String::new();
    for record in &bundle.train_split {
        train_content.push_str(&serde_json::to_string(record).map_err(|e| e.to_string())?);
        train_content.push('\n');
    }
    std::fs::write(dataset_dir.join("train.jsonl"), train_content).map_err(|e| e.to_string())?;

    let mut eval_content = String::new();
    for record in &bundle.eval_split {
        eval_content.push_str(&serde_json::to_string(record).map_err(|e| e.to_string())?);
        eval_content.push('\n');
    }
    std::fs::write(dataset_dir.join("eval.jsonl"), eval_content).map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_commons::maintainer::encrypt_for_maintainer;
    use crate::data_commons::telemetry_db::TelemetryDb;
    use serial_test::serial;
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
    #[serial]
    fn test_export_logic_with_mock_db() {
        std::env::set_var(
            "XAVIER_MAINTAINER_PRIVATE_KEY_HEX",
            "7861766965725f6c6f63616c5f6d61696e7461696e65725f6465765f73656372",
        );
        let db_file = NamedTempFile::new().unwrap();
        let db = TelemetryDb::new(db_file.path()).unwrap();

        // Add some mock logs
        for i in 0..10 {
            let payload = serde_json::json!({"event": format!("test_{}", i), "value": i});
            let payload_str = serde_json::to_string(&payload).unwrap();
            let (encrypted, ephemeral_pub) = encrypt_for_maintainer(&payload_str).unwrap();
            let maintainer_pub = crate::data_commons::maintainer::get_maintainer_public_key()
                .unwrap()
                .to_bytes();

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
    #[serial]
    fn test_deterministic_split() {
        std::env::set_var(
            "XAVIER_MAINTAINER_PRIVATE_KEY_HEX",
            "7861766965725f6c6f63616c5f6d61696e7461696e65725f6465765f73656372",
        );
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

    #[test]
    fn test_list_datasets_and_get_manifest_helpers() {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path();

        let _exporter = TrainingExporter::new(Path::new("dummy.db"));
        let bundle = TrainingBundle {
            manifest: crate::data_commons::readiness::TrainingBundleManifest {
                version: "1.0.0".to_string(),
                usage_policy: "Test policy".to_string(),
                reproducibility_seed: 42,
                split_counts: std::collections::HashMap::new(),
                data_files: vec!["train.jsonl".to_string()],
                privacy_level: "P2".to_string(),
            },
            train_split: vec![],
            eval_split: vec![],
            audit_summary: AuditSummary {
                total_records_found: 0,
                included_records: 0,
                excluded_records_no_consent: 0,
                excluded_records_revoked: 0,
                excluded_other: BTreeMap::new(),
            },
            audit: AnonymizationAudit::evaluate(AuditInput {
                level: PrivacyLevel::P2,
                config: PrivacyConfig::default(),
                counts: AuditCounts::default(),
                scrubbed: PiiCounts::default(),
                residual: PiiCounts::default(),
                numeric_fields_noised: 0,
                k_report: check_k_anonymity(&[], 5),
                sources: vec![],
                workspace: None,
                domain: None,
                generated_at: "2026-01-01T00:00:00Z",
            }),
        };

        write_bundle_to_dir(
            data_dir,
            "test_ds_1",
            &bundle,
            Some("INTERNAL".to_string()),
            Some("en".to_string()),
            Some("test".to_string()),
        )
        .unwrap();

        let manifests = list_datasets(data_dir).unwrap();
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].reproducibility_seed, 42);

        let manifest_val = get_manifest(data_dir, "test_ds_1").unwrap();
        assert_eq!(manifest_val["reproducibility_seed"], 42);
        assert_eq!(manifest_val["clearance"], "INTERNAL");
    }

    #[test]
    #[serial]
    fn test_export_applies_privacy_scrubbing() {
        std::env::set_var(
            "XAVIER_MAINTAINER_PRIVATE_KEY_HEX",
            "7861766965725f6c6f63616c5f6d61696e7461696e65725f6465765f73656372",
        );
        let db_file = NamedTempFile::new().unwrap();
        let db = TelemetryDb::new(db_file.path()).unwrap();

        let payload = serde_json::json!({
            "email": "user@example.com",
            "message": "Contact user@example.com for info at /home/belal/secret.rs",
            "wallet": "0x71C84918370533a26017b3738b7156da7014EA52",
            "author": "belal",
            "confidence": 0.95
        });
        let payload_str = serde_json::to_string(&payload).unwrap();
        let (encrypted, ephemeral_pub) = encrypt_for_maintainer(&payload_str).unwrap();
        let maintainer_pub = crate::data_commons::maintainer::get_maintainer_public_key()
            .unwrap()
            .to_bytes();

        db.save_encrypted_log(
            "hash_privacy_1",
            &encrypted,
            &ephemeral_pub,
            &maintainer_pub,
            "xv1_test_wallet",
        )
        .unwrap();

        // 1. Export with P2 (Default) -> Should scrub email & set manifest privacy_level to P2
        let exporter_p2 =
            TrainingExporter::new(db_file.path()).with_privacy_level(PrivacyLevel::P2);
        let bundle_p2 = exporter_p2.generate_bundle(123, 0.0, None).unwrap();
        assert_eq!(bundle_p2.manifest.privacy_level, "P2");
        assert_eq!(bundle_p2.train_split.len(), 1);
        let record_p2 = &bundle_p2.train_split[0];
        assert_eq!(record_p2["email"], "[EMAIL]");
        assert!(record_p2["message"].as_str().unwrap().contains("[EMAIL]"));

        // 2. Export with P3 -> Should scrub email, wallet, path, pseudonymize entity & add Laplace noise
        let exporter_p3 =
            TrainingExporter::new(db_file.path()).with_privacy_level(PrivacyLevel::P3);
        let bundle_p3 = exporter_p3.generate_bundle(123, 0.0, None).unwrap();
        assert_eq!(bundle_p3.manifest.privacy_level, "P3");
        assert_eq!(bundle_p3.train_split.len(), 1);
        let record_p3 = &bundle_p3.train_split[0];
        assert_eq!(record_p3["email"], "[EMAIL]");
        assert_eq!(record_p3["wallet"], "[WALLET]");
        assert_eq!(record_p3["author"], "[PERSON_A]");
        assert!(!record_p3["message"]
            .as_str()
            .unwrap()
            .contains("/home/belal"));
        assert!(record_p3["message"].as_str().unwrap().contains("[PATH]"));
        assert_ne!(record_p3["confidence"].as_f64().unwrap(), 0.95);

        // 3. Export with P4 (Local Only) -> Should NOT scrub email & set manifest privacy_level to P4
        let exporter_p4 =
            TrainingExporter::new(db_file.path()).with_privacy_level(PrivacyLevel::P4);
        let bundle_p4 = exporter_p4.generate_bundle(123, 0.0, None).unwrap();
        assert_eq!(bundle_p4.manifest.privacy_level, "P4");
        assert_eq!(bundle_p4.train_split.len(), 1);
        let record_p4 = &bundle_p4.train_split[0];
        assert_eq!(record_p4["email"], "user@example.com");
        assert_eq!(
            record_p4["wallet"],
            "0x71C84918370533a26017b3738b7156da7014EA52"
        );
        // P4 is "Local Only": it bypasses the privacy pipeline entirely, so the
        // file path embedded in the message is intentionally preserved.
        assert_eq!(
            record_p4["message"],
            "Contact user@example.com for info at /home/belal/secret.rs"
        );
    }

    // ---- MX-03: sources, audit, k-anonymity -------------------------------------------

    use crate::memory::store::{FileMemoryStore, MemoryRecord, MemoryStore};
    use std::sync::Arc;

    fn mem(id: &str, kind: &str, title: &str, content: &str) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            workspace_id: "ws1".to_string(),
            path: format!("dev/{id}"),
            content: content.to_string(),
            metadata: serde_json::json!({"kind": kind, "title": title}),
            ..MemoryRecord::default()
        }
    }

    async fn tempdir_store(dir: &Path, n: usize, kind: &str) -> Arc<dyn MemoryStore> {
        let store = FileMemoryStore::new(dir.join("memories.json"))
            .await
            .unwrap();
        for i in 0..n {
            store
                .put(mem(
                    &format!("m{i:02}"),
                    kind,
                    &format!("module_{i}"),
                    &format!("Module {i} parses the config and returns typed errors."),
                ))
                .await
                .unwrap();
        }
        Arc::new(store)
    }

    fn exporter() -> TrainingExporter {
        TrainingExporter::new(Path::new("unused.db"))
    }

    #[tokio::test]
    async fn test_memory_source_produces_pairs_from_tempdir_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = tempdir_store(dir.path(), 6, "file").await;
        // Different kind, other workspace, too short: must not appear.
        store
            .put(mem(
                "x1",
                "decision",
                "use sqlite",
                "We chose sqlite because it is embedded and simple.",
            ))
            .await
            .unwrap();
        store.put(mem("x2", "file", "tiny", "short")).await.unwrap();
        let mut other_ws = mem(
            "x3",
            "file",
            "other",
            "Content from another workspace entirely.",
        );
        other_ws.workspace_id = "ws2".to_string();
        store.put(other_ws).await.unwrap();

        let src = MemorySource::new(store, "ws1").with_kinds(vec!["file".into()]);
        let ctx = SourceContext {
            seed: 1,
            privacy_level: PrivacyLevel::P2,
        };
        let batch = src.collect(&ctx).await.unwrap();
        assert_eq!(batch.records.len(), 6);
        assert_eq!(
            batch.excluded_other.get("memory:content_too_short"),
            Some(&1)
        );
        let r = &batch.records[0];
        assert_eq!(r["instruction"], "What does module_0 do in this codebase?");
        assert_eq!(
            r["response"],
            "Module 0 parses the config and returns typed errors."
        );
        assert_eq!(r["metadata"]["kind"], "file");
        assert_eq!(r["metadata"]["workspace"], "ws1");
    }

    #[tokio::test]
    async fn test_challenge_source_reads_training_eligible_votes() {
        use crate::humanchallenge::store::HumanChallengeStore;
        use crate::humanchallenge::types::{
            ChallengeType, CurationVerdict, CurationVote, HumanChallengeEvent,
        };
        let hc = HumanChallengeStore::in_memory().unwrap();
        let mut ev =
            HumanChallengeEvent::new("s1", ChallengeType::Decision, "Why sqlite?", "raw", 0.9);
        ev.privacy_p4_local_only = false;
        hc.save_event(&ev).unwrap();
        hc.save_curation_vote(&CurationVote::new(
            &ev.id,
            CurationVerdict::Refine,
            Some("Because embedded.".into()),
            true,
            vec!["rust".into()],
            true,
        ))
        .unwrap();
        // Not eligible -> never returned.
        hc.save_curation_vote(&CurationVote::new(
            &ev.id,
            CurationVerdict::Accept,
            Some("nope".into()),
            true,
            vec![],
            false,
        ))
        .unwrap();
        // Local-only event: skipped unless P4.
        let ev2 = HumanChallengeEvent::new("s1", ChallengeType::Decision, "Secret?", "raw", 0.9);
        hc.save_event(&ev2).unwrap();
        hc.save_curation_vote(&CurationVote::new(
            &ev2.id,
            CurationVerdict::Accept,
            Some("Local.".into()),
            true,
            vec![],
            true,
        ))
        .unwrap();

        let src = ChallengeSource::new(Arc::new(hc));
        let p3 = src
            .collect(&SourceContext {
                seed: 1,
                privacy_level: PrivacyLevel::P3,
            })
            .await
            .unwrap();
        assert_eq!(p3.records.len(), 1);
        assert_eq!(p3.records[0]["instruction"], "Why sqlite?");
        assert_eq!(p3.records[0]["response"], "Because embedded.");
        assert_eq!(p3.excluded_other.get("challenge:local_only"), Some(&1));
        let p4 = src
            .collect(&SourceContext {
                seed: 1,
                privacy_level: PrivacyLevel::P4,
            })
            .await
            .unwrap();
        assert_eq!(p4.records.len(), 2);
    }

    fn sources_of(store: Arc<dyn MemoryStore>) -> Vec<Box<dyn TrainingSource>> {
        vec![Box::new(MemorySource::new(store, "ws1"))]
    }

    #[tokio::test]
    async fn test_p4_bundle_audit_is_local_only_and_never_passes() {
        let dir = tempfile::tempdir().unwrap();
        let store = tempdir_store(dir.path(), 8, "file").await;
        let bundle = exporter()
            .with_privacy_level(PrivacyLevel::P4)
            .generate_bundle_from_sources(&sources_of(store), 7, 0.25)
            .await
            .unwrap();
        assert!(bundle.audit.local_only);
        assert!(!bundle.audit.passed);
        assert_eq!(bundle.audit.privacy_level, "P4");
        assert!(bundle
            .audit
            .reasons
            .iter()
            .any(|r| r.contains("local-only")));

        let out = tempfile::tempdir().unwrap();
        write_bundle_to_dir(out.path(), "b4", &bundle, None, None, None).unwrap();
        let audit = load_dataset_audit(&out.path().join("b4")).unwrap();
        assert_eq!(audit["local_only"], true);
        assert_eq!(audit["passed"], false);
        let manifest = get_manifest(out.path(), "b4").unwrap();
        assert_eq!(manifest["privacy_level"], "P4");
        assert_eq!(manifest["local_only"], true);
    }

    #[tokio::test]
    async fn test_p3_audit_passes_with_k_and_fails_when_k_violated() {
        let dir = tempfile::tempdir().unwrap();
        let store = tempdir_store(dir.path(), 8, "file").await;
        let ok = exporter()
            .with_privacy_level(PrivacyLevel::P3)
            .generate_bundle_from_sources(&sources_of(store.clone()), 7, 0.25)
            .await
            .unwrap();
        assert!(ok.audit.passed, "reasons: {:?}", ok.audit.reasons);
        assert!(!ok.audit.local_only);
        assert_eq!(ok.audit.k, 5);
        assert!(ok.audit.dp_applied);
        assert_eq!(ok.audit.counts.included, 8);

        // One lone record of another kind forms a class of size 1 < k.
        store
            .put(mem(
                "zz",
                "decision",
                "lone",
                "A single decision that is easy to re-identify.",
            ))
            .await
            .unwrap();
        let bad = exporter()
            .with_privacy_level(PrivacyLevel::P3)
            .generate_bundle_from_sources(&sources_of(store.clone()), 7, 0.25)
            .await
            .unwrap();
        assert!(!bad.audit.passed);
        assert!(!bad.audit.k_anonymity.satisfied);
        assert!(bad.audit.reasons.iter().any(|r| r.contains("k-anonymity")));

        // Configurable k: k=1 makes the same data pass.
        let relaxed = exporter()
            .with_privacy_level(PrivacyLevel::P3)
            .with_privacy_config(PrivacyConfig {
                k: 1,
                epsilon: 0.5,
                ..PrivacyConfig::default()
            })
            .generate_bundle_from_sources(&sources_of(store), 7, 0.25)
            .await
            .unwrap();
        assert!(relaxed.audit.passed);
        assert_eq!(relaxed.audit.epsilon, 0.5);

        let out = tempfile::tempdir().unwrap();
        write_bundle_to_dir(out.path(), "bad", &bad, None, None, None).unwrap();
        let audit = load_dataset_audit(&out.path().join("bad")).unwrap();
        assert_eq!(audit["passed"], false);
        assert!(out.path().join("bad/anonymization_audit.json").is_file());
    }

    #[tokio::test]
    async fn test_invalid_epsilon_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = tempdir_store(dir.path(), 5, "file").await;
        let err = exporter()
            .with_privacy_level(PrivacyLevel::P3)
            .with_privacy_config(PrivacyConfig {
                epsilon: 0.0,
                ..PrivacyConfig::default()
            })
            .generate_bundle_from_sources(&sources_of(store), 1, 0.2)
            .await;
        assert!(err.is_err());
    }
}
