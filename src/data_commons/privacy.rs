use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

/// Privacy levels for data commons training export and telemetry processing.
/// - `P2`: Anonymized and PII scrubbed (emails, paths, API keys/tokens).
/// - `P3`: PII scrubbed + Differential Privacy (Laplace noise applied to numeric metrics).
/// - `P4`: Local-only data. Unscrubbed raw data for local model training.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivacyLevel {
    P2,
    P3,
    P4,
}

impl PrivacyLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrivacyLevel::P2 => "P2",
            PrivacyLevel::P3 => "P3",
            PrivacyLevel::P4 => "P4",
        }
    }
}

impl Default for PrivacyLevel {
    fn default() -> Self {
        PrivacyLevel::P2
    }
}

/// Pipeline for processing records with PII scrubbing and differential privacy noise.
pub struct PrivacyPipeline;

static EMAIL_REGEX: OnceLock<Regex> = OnceLock::new();
static PATH_REGEX: OnceLock<Regex> = OnceLock::new();
static API_KEY_REGEX: OnceLock<Regex> = OnceLock::new();

impl PrivacyPipeline {
    pub fn new() -> Self {
        Self
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
                    add_laplace_noise(confidence, 1.0, 1.0)
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
            Value::Number(n) => {
                if level == PrivacyLevel::P3 {
                    if let Some(f) = n.as_f64() {
                        let noisy = add_laplace_noise(f, 1.0, 1.0);
                        if let Some(num) = serde_json::Number::from_f64(noisy) {
                            *n = num;
                        }
                    }
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    self.scrub_value(v, level);
                }
            }
            Value::Object(map) => {
                for (_k, v) in map {
                    self.scrub_value(v, level);
                }
            }
            _ => {}
        }
    }

    pub fn scrub_string(&self, input: &str) -> String {
        let email_re = EMAIL_REGEX.get_or_init(|| {
            Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap()
        });
        let path_re = PATH_REGEX.get_or_init(|| {
            Regex::new(r"(?:/[a-zA-Z0-9_.-]+){2,}|(?:[a-zA-Z]:\\[a-zA-Z0-9_.-]+){2,}").unwrap()
        });
        let api_key_re = API_KEY_REGEX.get_or_init(|| {
            Regex::new(r"\b(?:sk-[a-zA-Z0-9_-]{20,}|ghp_[a-zA-Z0-9]{36}|AKIA[0-9A-Z]{16})\b").unwrap()
        });

        let s = email_re.replace_all(input, "[EMAIL]");
        let s = api_key_re.replace_all(&s, "[API_KEY]");
        let s = path_re.replace_all(&s, "[PATH]");
        s.to_string()
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
}
