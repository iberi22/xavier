use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Manifest for a training bundle.
/// Grounded in requirements for fine-tuning readiness.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrainingBundleManifest {
    /// Version of the bundle format
    pub version: String,
    /// Detailed usage policy (mandatory)
    pub usage_policy: String,
    /// Seed used for reproducible splits and sampling
    pub reproducibility_seed: u64,
    /// Counts of records per split (e.g., "train", "eval")
    pub split_counts: HashMap<String, usize>,
    /// List of data files included in the bundle (usually .jsonl)
    pub data_files: Vec<String>,
}

/// Detailed report of the readiness check.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ReadinessReport {
    /// True if all safety gates passed
    pub is_ready: bool,
    /// List of validation errors found
    pub errors: Vec<String>,
    /// List of checks successfully performed
    pub checks_performed: Vec<String>,
}

pub struct ReadinessValidator {
    bundle_path: PathBuf,
}

impl ReadinessValidator {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            bundle_path: path.as_ref().to_path_buf(),
        }
    }

    /// Validate.
    pub fn validate(&self) -> ReadinessReport {
        let mut report = ReadinessReport::default();

        // 1. Check for bundle_manifest.json
        let manifest_path = self.bundle_path.join("bundle_manifest.json");
        let manifest = self.load_manifest(&manifest_path, &mut report).ok();

        // 2. Check for anonymization_audit.json
        let audit_path = self.bundle_path.join("anonymization_audit.json");
        if !audit_path.exists() {
            report
                .errors
                .push("Missing anonymization_audit.json".to_string());
        } else {
            report
                .checks_performed
                .push("Anonymization audit exists".to_string());
        }

        // 3. Detailed manifest validation
        if let Some(m) = &manifest {
            self.validate_manifest_fields(m, &mut report);

            // 4. Data file scanning
            for data_file in &m.data_files {
                let data_file_path = self.bundle_path.join(data_file);
                if !data_file_path.exists() {
                    report
                        .errors
                        .push(format!("Data file not found: {}", data_file));
                    continue;
                }

                if let Err(e) = self.scan_data_file(&data_file_path, &mut report) {
                    report
                        .errors
                        .push(format!("Error scanning {}: {}", data_file, e));
                }
            }
        }

        report.is_ready = report.errors.is_empty();
        report
    }

    fn load_manifest(
        &self,
        path: &Path,
        report: &mut ReadinessReport,
    ) -> Result<TrainingBundleManifest> {
        if !path.exists() {
            report
                .errors
                .push("Missing bundle_manifest.json".to_string());
            return Err(anyhow!("Manifest not found"));
        }

        let content = fs::read_to_string(path)?;
        let manifest: TrainingBundleManifest = serde_json::from_str(&content)?;
        report
            .checks_performed
            .push("Manifest loaded and parsed".to_string());
        Ok(manifest)
    }

    fn validate_manifest_fields(
        &self,
        manifest: &TrainingBundleManifest,
        report: &mut ReadinessReport,
    ) {
        if manifest.usage_policy.trim().is_empty() {
            report
                .errors
                .push("Usage policy is empty in manifest".to_string());
        } else {
            report
                .checks_performed
                .push("Usage policy present".to_string());
        }

        if manifest.reproducibility_seed == 0 {
            report.errors.push(
                "Nondeterministic output detected: reproducibility_seed is 0 or missing"
                    .to_string(),
            );
        } else {
            report
                .checks_performed
                .push("Reproducibility seed present".to_string());
        }

        let train_count = manifest.split_counts.get("train").cloned().unwrap_or(0);
        let eval_count = manifest.split_counts.get("eval").cloned().unwrap_or(0);

        if train_count == 0 {
            report
                .errors
                .push("Missing or empty 'train' split count".to_string());
        }
        if eval_count == 0 {
            report
                .errors
                .push("Missing or empty 'eval' split count".to_string());
        }

        if train_count > 0 && eval_count > 0 {
            report
                .checks_performed
                .push("Valid train/eval split counts".to_string());
        }
    }

    fn scan_data_file(&self, path: &Path, report: &mut ReadinessReport) -> Result<()> {
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);

        for (i, line) in reader.lines().enumerate() {
            let line = line?;
            let record: serde_json::Value = serde_json::from_str(&line)?;

            // Check metadata
            let metadata = record.get("metadata").and_then(|m| m.as_object());

            if let Some(meta) = metadata {
                let consent = meta
                    .get("consent_given")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let is_private = meta
                    .get("is_private")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let revoked = meta
                    .get("revoked")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if !consent {
                    report.errors.push(format!(
                        "Record at {}:{} missing consent",
                        path.display(),
                        i + 1
                    ));
                }
                if is_private {
                    report.errors.push(format!(
                        "Record at {}:{} is private and should be excluded",
                        path.display(),
                        i + 1
                    ));
                }
                if revoked {
                    report.errors.push(format!(
                        "Record at {}:{} is revoked and should be excluded",
                        path.display(),
                        i + 1
                    ));
                }
            } else {
                report.errors.push(format!(
                    "Record at {}:{} missing metadata block",
                    path.display(),
                    i + 1
                ));
            }
        }

        report
            .checks_performed
            .push(format!("Scanned {}", path.display()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    fn create_mock_bundle(
        dir: &Path,
        manifest_content: &str,
        audit_exists: bool,
        data_files: Vec<(&str, &str)>,
    ) -> Result<()> {
        if !manifest_content.is_empty() {
            let mut f = File::create(dir.join("bundle_manifest.json"))?;
            f.write_all(manifest_content.as_bytes())?;
        }
        if audit_exists {
            File::create(dir.join("anonymization_audit.json"))?;
        }
        for (name, content) in data_files {
            let mut f = File::create(dir.join(name))?;
            f.write_all(content.as_bytes())?;
        }
        Ok(())
    }

    #[test]
    fn test_valid_bundle() {
        let dir = tempdir().unwrap();
        let manifest = serde_json::json!({
            "version": "1.0",
            "usage_policy": "Internal research only",
            "reproducibility_seed": 42,
            "split_counts": { "train": 1, "eval": 1 },
            "data_files": ["train.jsonl"]
        });
        let record = serde_json::json!({
            "text": "sample",
            "metadata": { "consent_given": true, "is_private": false, "revoked": false }
        });

        create_mock_bundle(
            dir.path(),
            &manifest.to_string(),
            true,
            vec![("train.jsonl", &format!("{record}\n"))],
        )
        .unwrap();

        let validator = ReadinessValidator::new(dir.path());
        let report = validator.validate();
        assert!(
            report.is_ready,
            "Report should be ready, but errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn test_missing_manifest() {
        let dir = tempdir().unwrap();
        create_mock_bundle(dir.path(), "", true, vec![]).unwrap();

        let validator = ReadinessValidator::new(dir.path());
        let report = validator.validate();
        assert!(!report.is_ready);
        assert!(report
            .errors
            .iter()
            .any(|e| e.contains("Missing bundle_manifest.json")));
    }

    #[test]
    fn test_missing_audit() {
        let dir = tempdir().unwrap();
        let manifest = serde_json::json!({
            "version": "1.0",
            "usage_policy": "Policy",
            "reproducibility_seed": 1,
            "split_counts": { "train": 1, "eval": 1 },
            "data_files": []
        });
        create_mock_bundle(dir.path(), &manifest.to_string(), false, vec![]).unwrap();

        let validator = ReadinessValidator::new(dir.path());
        let report = validator.validate();
        assert!(!report.is_ready);
        assert!(report
            .errors
            .iter()
            .any(|e| e.contains("Missing anonymization_audit.json")));
    }

    #[test]
    fn test_nondeterministic_seed() {
        let dir = tempdir().unwrap();
        let manifest = serde_json::json!({
            "version": "1.0",
            "usage_policy": "Policy",
            "reproducibility_seed": 0,
            "split_counts": { "train": 1, "eval": 1 },
            "data_files": []
        });
        create_mock_bundle(dir.path(), &manifest.to_string(), true, vec![]).unwrap();

        let validator = ReadinessValidator::new(dir.path());
        let report = validator.validate();
        assert!(!report.is_ready);
        assert!(report
            .errors
            .iter()
            .any(|e| e.contains("Nondeterministic output detected")));
    }

    #[test]
    fn test_data_safety_violations() {
        let dir = tempdir().unwrap();
        let manifest = serde_json::json!({
            "version": "1.0",
            "usage_policy": "Policy",
            "reproducibility_seed": 123,
            "split_counts": { "train": 3, "eval": 1 },
            "data_files": ["data.jsonl"]
        });

        let r1 = serde_json::json!({ "metadata": { "consent_given": false } });
        let r2 = serde_json::json!({ "metadata": { "consent_given": true, "is_private": true } });
        let r3 = serde_json::json!({ "metadata": { "consent_given": true, "revoked": true } });

        let data = format!("{}\n{}\n{}\n", r1, r2, r3);

        create_mock_bundle(
            dir.path(),
            &manifest.to_string(),
            true,
            vec![("data.jsonl", &data)],
        )
        .unwrap();

        let validator = ReadinessValidator::new(dir.path());
        let report = validator.validate();
        assert!(!report.is_ready);
        assert_eq!(report.errors.len(), 3);
        assert!(report.errors.iter().any(|e| e.contains("missing consent")));
        assert!(report.errors.iter().any(|e| e.contains("is private")));
        assert!(report.errors.iter().any(|e| e.contains("is revoked")));
    }
}
