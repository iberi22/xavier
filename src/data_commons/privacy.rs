use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Default differential-privacy epsilon (Laplace mechanism) used when none is configured.
pub const DEFAULT_EPSILON: f64 = 1.0;
/// Default L1 sensitivity assumed for numeric fields.
pub const DEFAULT_SENSITIVITY: f64 = 1.0;
/// Default minimum equivalence-class size for the k-anonymity check.
pub const DEFAULT_K: usize = 5;
/// Quasi-identifier metadata fields used by the k-anonymity check. They are read from the
/// record's `metadata` object (missing field = `"*"`). A record is re-identifiable when the
/// combination of these values is shared by fewer than `k` records in the bundle.
pub const QUASI_IDENTIFIERS: [&str; 5] = ["source", "kind", "domain", "workspace", "namespace"];

/// Privacy levels for data commons training export and telemetry processing.
/// - `P0`/`P1`: legacy public/low-sensitivity levels; processed like `P2` (scrubbed).
/// - `P2`: Anonymized and PII scrubbed (emails, paths, API keys/tokens).
/// - `P3`: PII scrubbed + Differential Privacy (Laplace noise applied to numeric metrics).
/// - `P4`: Local-only data. Unscrubbed raw data for local model training; never leaves the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PrivacyLevel {
    P0,
    P1,
    #[default]
    P2,
    P3,
    P4,
}

impl PrivacyLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrivacyLevel::P0 => "P0",
            PrivacyLevel::P1 => "P1",
            PrivacyLevel::P2 => "P2",
            PrivacyLevel::P3 => "P3",
            PrivacyLevel::P4 => "P4",
        }
    }

    /// Parse "P0".."P4" (case-insensitive).
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "P0" => Some(PrivacyLevel::P0),
            "P1" => Some(PrivacyLevel::P1),
            "P2" => Some(PrivacyLevel::P2),
            "P3" => Some(PrivacyLevel::P3),
            "P4" => Some(PrivacyLevel::P4),
            _ => None,
        }
    }

    /// True when bundles of this level must never leave the node.
    pub fn is_local_only(&self) -> bool {
        matches!(self, PrivacyLevel::P4)
    }
}

/// Tunable privacy parameters (no hardcoded epsilon).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PrivacyConfig {
    /// Differential-privacy epsilon for the Laplace mechanism (must be finite and > 0).
    pub epsilon: f64,
    /// L1 sensitivity assumed for numeric fields.
    pub sensitivity: f64,
    /// Minimum equivalence-class size for k-anonymity (>= 1).
    pub k: usize,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            epsilon: DEFAULT_EPSILON,
            sensitivity: DEFAULT_SENSITIVITY,
            k: DEFAULT_K,
        }
    }
}

impl PrivacyConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !self.epsilon.is_finite() || self.epsilon <= 0.0 {
            return Err("epsilon must be a finite number > 0".to_string());
        }
        if !self.sensitivity.is_finite() || self.sensitivity <= 0.0 {
            return Err("sensitivity must be a finite number > 0".to_string());
        }
        if self.k == 0 {
            return Err("k must be >= 1".to_string());
        }
        Ok(())
    }
}

/// Counts of PII items found per category.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PiiCounts {
    pub emails: usize,
    pub paths: usize,
    pub api_keys: usize,
    pub wallets: usize,
    pub entities: usize,
}

impl PiiCounts {
    pub fn total(&self) -> usize {
        self.emails + self.paths + self.api_keys + self.wallets + self.entities
    }

    pub fn add(&mut self, other: &PiiCounts) {
        self.emails += other.emails;
        self.paths += other.paths;
        self.api_keys += other.api_keys;
        self.wallets += other.wallets;
        self.entities += other.entities;
    }
}

/// Pipeline for processing records with PII scrubbing and differential privacy noise.
pub struct PrivacyPipeline {
    config: PrivacyConfig,
}

struct PiiRegexes {
    email: Regex,
    path: Regex,
    api_key: Regex,
    wallet: Regex,
    entity: Regex,
}

static REGEXES: OnceLock<PiiRegexes> = OnceLock::new();

fn regexes() -> &'static PiiRegexes {
    REGEXES.get_or_init(|| PiiRegexes {
        email: Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
        path: Regex::new(r"(?:/[a-zA-Z0-9_.-]+){2,}|[a-zA-Z]:\\(?:[a-zA-Z0-9_.-]+\\?)+").unwrap(),
        api_key: Regex::new(r"\b(?:sk-[a-zA-Z0-9_-]{20,}|ghp_[a-zA-Z0-9]{36}|AKIA[0-9A-Z]{16})\b")
            .unwrap(),
        wallet: Regex::new(
            r"\b(?:0x[a-fA-F0-9]{40}|xv1[a-zA-Z0-9_]{10,}|ed25519:[a-zA-Z0-9]{32,})\b",
        )
        .unwrap(),
        entity: Regex::new(r"(?i)\b(?:belal|iberi22)\b").unwrap(),
    })
}

impl Default for PrivacyPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl PrivacyPipeline {
    pub fn new() -> Self {
        Self {
            config: PrivacyConfig::default(),
        }
    }

    /// Pipeline with explicit epsilon / sensitivity / k.
    pub fn with_config(config: PrivacyConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &PrivacyConfig {
        &self.config
    }

    /// Count PII items in every string of `value` (keys named wallet/author/etc. are
    /// counted as well, since the pipeline replaces them wholesale).
    pub fn count_pii(&self, value: &Value) -> PiiCounts {
        let mut counts = PiiCounts::default();
        self.count_pii_into(value, &mut counts);
        counts
    }

    fn count_pii_into(&self, value: &Value, counts: &mut PiiCounts) {
        match value {
            Value::String(s) => counts.add(&self.count_pii_str(s)),
            Value::Array(arr) => arr.iter().for_each(|v| self.count_pii_into(v, counts)),
            Value::Object(map) => map.values().for_each(|v| self.count_pii_into(v, counts)),
            _ => {}
        }
    }

    fn count_pii_str(&self, input: &str) -> PiiCounts {
        let r = regexes();
        PiiCounts {
            emails: r.email.find_iter(input).count(),
            paths: r.path.find_iter(input).count(),
            api_keys: r.api_key.find_iter(input).count(),
            wallets: r.wallet.find_iter(input).count(),
            entities: r.entity.find_iter(input).count(),
        }
    }

    /// Number of numeric leaves in `value` (the fields Laplace noise is applied to at P3).
    pub fn count_numeric_leaves(value: &Value) -> usize {
        match value {
            Value::Number(_) => 1,
            Value::Array(arr) => arr.iter().map(Self::count_numeric_leaves).sum(),
            Value::Object(map) => map.values().map(Self::count_numeric_leaves).sum(),
            _ => 0,
        }
    }

    /// Process a batch of JSON records according to the specified `PrivacyLevel`.
    pub fn process_batch(&self, records: Vec<Value>, level: PrivacyLevel) -> Vec<Value> {
        if level == PrivacyLevel::P4 {
            return records;
        }

        records
            .into_iter()
            .map(|mut record| {
                self.scrub_value(&mut record, level);
                record
            })
            .collect()
    }

    /// Process tuple records: `(id, content, domain_tags, challenge_type, confidence)`
    pub fn process_tuple_batch(
        &self,
        records: Vec<(String, String, Vec<String>, String, f64)>,
        level: PrivacyLevel,
    ) -> Vec<(String, String, Vec<String>, String, f64)> {
        if level == PrivacyLevel::P4 {
            return records;
        }

        records
            .into_iter()
            .map(|(id, content, tags, ctype, confidence)| {
                let scrubbed_content = self.scrub_string(&content);
                let scrubbed_confidence = if level == PrivacyLevel::P3 {
                    add_laplace_noise(confidence, self.config.sensitivity, self.config.epsilon)
                } else {
                    confidence
                };
                (id, scrubbed_content, tags, ctype, scrubbed_confidence)
            })
            .collect()
    }

    fn scrub_value(&self, val: &mut Value, level: PrivacyLevel) {
        match val {
            Value::String(s) => {
                *s = self.scrub_string(s);
            }
            Value::Number(n) if level == PrivacyLevel::P3 => {
                if let Some(f) = n.as_f64() {
                    let noisy = add_laplace_noise(f, self.config.sensitivity, self.config.epsilon);
                    if let Some(num) = serde_json::Number::from_f64(noisy) {
                        *n = num;
                    }
                }
            }
            Value::Number(_) => {}
            Value::Array(arr) => {
                for v in arr {
                    self.scrub_value(v, level);
                }
            }
            Value::Object(map) => {
                for (k, v) in map {
                    let k_lower = k.to_lowercase();
                    if k_lower == "wallet"
                        || k_lower == "wallet_address"
                        || k_lower == "wallet_addr"
                    {
                        if let Value::String(_) = v {
                            *v = Value::String("[WALLET]".to_string());
                            continue;
                        }
                    }
                    if level == PrivacyLevel::P3
                        && (k_lower == "author"
                            || k_lower == "user"
                            || k_lower == "creator"
                            || k_lower == "maintainer")
                    {
                        if let Value::String(_) = v {
                            *v = Value::String("[PERSON_A]".to_string());
                            continue;
                        }
                    }
                    if level == PrivacyLevel::P3 && (k_lower == "org" || k_lower == "organization")
                    {
                        if let Value::String(_) = v {
                            *v = Value::String("[ORG_X]".to_string());
                            continue;
                        }
                    }
                    self.scrub_value(v, level);
                }
            }
            _ => {}
        }
    }

    pub fn scrub_string(&self, input: &str) -> String {
        let r = regexes();
        let s = r.email.replace_all(input, "[EMAIL]");
        let s = r.api_key.replace_all(&s, "[API_KEY]");
        let s = r.wallet.replace_all(&s, "[WALLET]");
        let s = r.path.replace_all(&s, "[PATH]");
        let s = r.entity.replace_all(&s, "[PERSON_A]");
        s.to_string()
    }
}

/// One equivalence class that violates k-anonymity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KViolation {
    pub quasi_identifiers: BTreeMap<String, String>,
    pub size: usize,
}

/// Result of the k-anonymity check over `QUASI_IDENTIFIERS`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KAnonymityReport {
    pub k: usize,
    pub quasi_identifiers: Vec<String>,
    pub equivalence_classes: usize,
    /// Size of the smallest equivalence class (0 when there are no records).
    pub min_class_size: usize,
    /// Violating classes (capped at `MAX_REPORTED_VIOLATIONS`).
    pub violations: Vec<KViolation>,
    pub violating_records: usize,
    pub satisfied: bool,
}

const MAX_REPORTED_VIOLATIONS: usize = 20;

/// Group records by the tuple of quasi-identifier values found in `record["metadata"]`
/// (missing/non-scalar = `"*"`) and require every class to hold at least `k` records.
pub fn check_k_anonymity(records: &[Value], k: usize) -> KAnonymityReport {
    let mut classes: BTreeMap<Vec<String>, usize> = BTreeMap::new();
    for record in records {
        let key: Vec<String> = QUASI_IDENTIFIERS
            .iter()
            .map(
                |field| match record.get("metadata").and_then(|m| m.get(*field)) {
                    Some(Value::String(s)) if !s.is_empty() => s.clone(),
                    Some(Value::Number(n)) => n.to_string(),
                    Some(Value::Bool(b)) => b.to_string(),
                    _ => "*".to_string(),
                },
            )
            .collect();
        *classes.entry(key).or_insert(0) += 1;
    }
    let min_class_size = classes.values().copied().min().unwrap_or(0);
    let mut violations = Vec::new();
    let mut violating_records = 0;
    for (key, size) in &classes {
        if *size < k {
            violating_records += size;
            if violations.len() < MAX_REPORTED_VIOLATIONS {
                violations.push(KViolation {
                    quasi_identifiers: QUASI_IDENTIFIERS
                        .iter()
                        .map(|f| f.to_string())
                        .zip(key.iter().cloned())
                        .collect(),
                    size: *size,
                });
            }
        }
    }
    KAnonymityReport {
        k,
        quasi_identifiers: QUASI_IDENTIFIERS.iter().map(|f| f.to_string()).collect(),
        equivalence_classes: classes.len(),
        min_class_size,
        violations,
        violating_records,
        satisfied: violating_records == 0,
    }
}

/// Record counts reported in the audit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditCounts {
    pub total_found: usize,
    pub included: usize,
    pub train: usize,
    pub eval: usize,
    pub excluded_no_consent: usize,
    pub excluded_revoked: usize,
    pub excluded_other: BTreeMap<String, usize>,
}

/// Everything `AnonymizationAudit::evaluate` needs.
pub struct AuditInput<'a> {
    pub level: PrivacyLevel,
    pub config: PrivacyConfig,
    pub counts: AuditCounts,
    pub scrubbed: PiiCounts,
    pub residual: PiiCounts,
    pub numeric_fields_noised: usize,
    pub k_report: KAnonymityReport,
    pub sources: Vec<String>,
    pub workspace: Option<String>,
    pub domain: Option<String>,
    pub generated_at: &'a str,
}

/// Content of `anonymization_audit.json`. `passed` means "this bundle may leave the node".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnonymizationAudit {
    pub schema_version: String,
    pub generated_at: String,
    /// "P0".."P4".
    pub privacy_level: String,
    pub local_only: bool,
    pub k: usize,
    pub epsilon: f64,
    /// True when Laplace noise was actually applied (P3 only).
    pub dp_applied: bool,
    pub sources: Vec<String>,
    pub workspace: Option<String>,
    pub domain: Option<String>,
    pub counts: AuditCounts,
    /// PII items found in the raw records and replaced by the scrubber.
    pub scrubbed: PiiCounts,
    /// PII items still detectable after scrubbing (must be 0 to pass).
    pub residual_pii: PiiCounts,
    pub numeric_fields_noised: usize,
    pub k_anonymity: KAnonymityReport,
    pub passed: bool,
    pub reasons: Vec<String>,
}

impl AnonymizationAudit {
    pub fn evaluate(input: AuditInput<'_>) -> Self {
        let local_only = input.level.is_local_only();
        let dp_applied = input.level == PrivacyLevel::P3;
        let mut reasons = Vec::new();
        if local_only {
            reasons.push(
                "P4 bundle is local-only: raw data, never allowed to leave the node".to_string(),
            );
        } else {
            if input.counts.included == 0 {
                reasons.push("bundle has no records".to_string());
            }
            if input.residual.total() > 0 {
                reasons.push(format!(
                    "residual PII detected after scrubbing ({} items)",
                    input.residual.total()
                ));
            }
            if !input.k_report.satisfied {
                reasons.push(format!(
                    "k-anonymity violated: smallest equivalence class has {} record(s), k={} ({} record(s) in {} violating class(es))",
                    input.k_report.min_class_size,
                    input.k_report.k,
                    input.k_report.violating_records,
                    input.k_report.violations.len()
                ));
            }
            if dp_applied && !(input.config.epsilon.is_finite() && input.config.epsilon > 0.0) {
                reasons.push("differential privacy epsilon is not > 0".to_string());
            }
        }
        AnonymizationAudit {
            schema_version: "1.0.0".to_string(),
            generated_at: input.generated_at.to_string(),
            privacy_level: input.level.as_str().to_string(),
            local_only,
            k: input.config.k,
            epsilon: input.config.epsilon,
            dp_applied,
            sources: input.sources,
            workspace: input.workspace,
            domain: input.domain,
            counts: input.counts,
            scrubbed: input.scrubbed,
            residual_pii: input.residual,
            numeric_fields_noised: input.numeric_fields_noised,
            k_anonymity: input.k_report,
            passed: reasons.is_empty(),
            reasons,
        }
    }
}

pub fn add_laplace_noise(value: f64, sensitivity: f64, epsilon: f64) -> f64 {
    if epsilon <= 0.0 {
        return value;
    }
    let scale = sensitivity / epsilon;
    let u: f64 = rand::random::<f64>() - 0.5;
    let noise = -scale * u.signum() * (1.0 - 2.0 * u.abs()).ln();
    value + noise
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pii_scrubbing() {
        let pipeline = PrivacyPipeline::new();
        let input = serde_json::json!({
            "email": "user@example.com",
            "path": "/home/user/secret.txt",
            "key": "sk-1234567890123456789012345",
            "clean": "hello world"
        });

        let processed = pipeline.process_batch(vec![input], PrivacyLevel::P2);
        let record = &processed[0];

        assert_eq!(record["email"], "[EMAIL]");
        assert_eq!(record["path"], "[PATH]");
        assert_eq!(record["key"], "[API_KEY]");
        assert_eq!(record["clean"], "hello world");
    }

    #[test]
    fn test_p4_bypasses_scrubbing() {
        let pipeline = PrivacyPipeline::new();
        let input = serde_json::json!({
            "email": "user@example.com"
        });

        let processed = pipeline.process_batch(vec![input], PrivacyLevel::P4);
        assert_eq!(processed[0]["email"], "user@example.com");
    }

    #[test]
    fn test_tuple_batch_scrubbing() {
        let pipeline = PrivacyPipeline::new();
        let records = vec![(
            "id1".to_string(),
            "Contact user@example.com at /var/log/sys".to_string(),
            vec!["tag".to_string()],
            "decision".to_string(),
            0.95,
        )];

        let processed = pipeline.process_tuple_batch(records, PrivacyLevel::P2);
        assert_eq!(processed[0].1, "Contact [EMAIL] at [PATH]");
        assert_eq!(processed[0].4, 0.95);
    }

    #[test]
    fn test_p3_comprehensive_scrubbing() {
        let pipeline = PrivacyPipeline::new();
        let input = serde_json::json!({
            "instruction": "Deploy smart contract from C:\\Users\\belal\\project\\main.rs with wallet 0x71C84918370533a26017b3738b7156da7014EA52",
            "author": "belal",
            "wallet": "xv1_test_wallet_node_77",
            "file_path": "/home/belal/proyectosSWAL/apps/xavier/src/lib.rs",
            "confidence": 0.99,
            "count": 100.0
        });

        let processed = pipeline.process_batch(vec![input], PrivacyLevel::P3);
        let record = &processed[0];

        // 1. Zero file paths
        assert!(!record["instruction"]
            .as_str()
            .unwrap()
            .contains("C:\\Users\\belal"));
        assert_eq!(record["file_path"], "[PATH]");
        assert!(record["instruction"].as_str().unwrap().contains("[PATH]"));

        // 2. Zero wallet addresses
        assert!(!record["instruction"]
            .as_str()
            .unwrap()
            .contains("0x71C84918370533a26017b3738b7156da7014EA52"));
        assert_eq!(record["wallet"], "[WALLET]");
        assert!(record["instruction"].as_str().unwrap().contains("[WALLET]"));

        // 3. Entity pseudonymization
        assert_eq!(record["author"], "[PERSON_A]");
        assert!(!record["instruction"].as_str().unwrap().contains("belal"));

        // 4. Laplace noise applied
        assert_ne!(record["confidence"].as_f64().unwrap(), 0.99);
        assert_ne!(record["count"].as_f64().unwrap(), 100.0);
    }

    fn rec(kind: &str, i: usize) -> Value {
        serde_json::json!({"instruction": format!("q{i}"), "response": "a", "metadata": {"kind": kind, "source": "memory"}})
    }

    #[test]
    fn test_k_anonymity_detects_small_classes() {
        let mut records: Vec<Value> = (0..5).map(|i| rec("file", i)).collect();
        assert!(check_k_anonymity(&records, 5).satisfied);
        records.push(rec("decision", 99));
        let report = check_k_anonymity(&records, 5);
        assert!(!report.satisfied);
        assert_eq!(report.min_class_size, 1);
        assert_eq!(report.violating_records, 1);
        assert_eq!(report.violations[0].quasi_identifiers["kind"], "decision");
        // configurable k
        assert!(check_k_anonymity(&records, 1).satisfied);
    }

    #[test]
    fn test_epsilon_is_configurable_and_validated() {
        let cfg = PrivacyConfig {
            epsilon: 0.0,
            ..PrivacyConfig::default()
        };
        assert!(cfg.validate().is_err());
        assert!(PrivacyConfig::default().validate().is_ok());
        // Tiny epsilon => huge noise scale; huge epsilon => near-zero noise.
        let tight = PrivacyPipeline::with_config(PrivacyConfig {
            epsilon: 1e9,
            ..PrivacyConfig::default()
        });
        let out = tight.process_batch(vec![serde_json::json!({"n": 10.0})], PrivacyLevel::P3);
        assert!((out[0]["n"].as_f64().unwrap() - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_privacy_level_parse() {
        assert_eq!(PrivacyLevel::parse("p3"), Some(PrivacyLevel::P3));
        assert_eq!(PrivacyLevel::parse("P0"), Some(PrivacyLevel::P0));
        assert_eq!(PrivacyLevel::parse("P9"), None);
        assert!(PrivacyLevel::P4.is_local_only());
        assert!(!PrivacyLevel::P3.is_local_only());
    }
}
