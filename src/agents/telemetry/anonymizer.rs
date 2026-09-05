//! High-throughput PII anonymizer and differential privacy scrubber for telemetry.
//!
//! Provides zero-allocation or low-allocation scrubbing of sensitive credentials,
//! API keys, tokens, emails, IP addresses, and applies differential privacy (Laplace noise)
//! for numeric telemetry metrics.

use regex::{Regex, RegexSet};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Strategy for handling sensitive text matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactionStrategy {
    /// Mask with generic tag e.g. `[REDACTED:API_KEY]`
    Tag,
    /// Mask with SHA-256 derived truncated hash e.g. `[HASH:a1b2c3d4]`
    TruncatedHash,
}

/// Configuration options for `TelemetryAnonymizer`.
#[derive(Debug, Clone)]
pub struct AnonymizerConfig {
    /// Strategy to use for replacing matched PII/tokens.
    pub strategy: RedactionStrategy,
    /// Enable differential privacy noise for numeric fields.
    pub enable_dp: bool,
    /// Epsilon value for Laplace mechanism in differential privacy.
    pub dp_epsilon: f64,
}

impl Default for AnonymizerConfig {
    fn default() -> Self {
        Self {
            strategy: RedactionStrategy::TruncatedHash,
            enable_dp: true,
            dp_epsilon: 1.0,
        }
    }
}

/// High-throughput PII anonymizer and DP scrubber.
#[derive(Clone)]
pub struct TelemetryAnonymizer {
    config: AnonymizerConfig,
    regex_set: Arc<RegexSet>,
    regexes: Arc<Vec<(Regex, &'static str)>>,
}

impl std::fmt::Debug for TelemetryAnonymizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelemetryAnonymizer")
            .field("config", &self.config)
            .finish()
    }
}

impl Default for TelemetryAnonymizer {
    fn default() -> Self {
        Self::new(AnonymizerConfig::default())
    }
}

impl TelemetryAnonymizer {
    /// Creates a new `TelemetryAnonymizer` with pre-compiled patterns.
    pub fn new(config: AnonymizerConfig) -> Self {
        let pattern_defs: Vec<(&str, &'static str)> = vec![
            // API Keys & Tokens
            (r"\bsk-[a-zA-Z0-9_-]{20,}\b", "API_KEY"),
            (r"\bghp_[a-zA-Z0-9]{36}\b", "GITHUB_PAT"),
            (r"\bglpat-[a-zA-Z0-9_-]{20,}\b", "GITLAB_PAT"),
            (r"\bxoxb-[a-zA-Z0-9_-]{10,}\b", "SLACK_TOKEN"),
            (r"\bAKIA[0-9A-Z]{16}\b", "AWS_ACCESS_KEY"),
            // JWT Tokens
            (
                r"\beyJ[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\b",
                "JWT",
            ),
            // Emails
            (r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}", "EMAIL"),
            // IPv4
            (
                r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b",
                "IPV4",
            ),
            // IPv6
            (
                r"\b(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}\b|\b(?:[0-9a-fA-F]{1,4}:){1,7}:|:(?::[0-9a-fA-F]{1,4}){1,7}\b",
                "IPV6",
            ),
        ];

        let pattern_strings: Vec<&str> = pattern_defs.iter().map(|(p, _)| *p).collect();
        let regex_set =
            RegexSet::new(&pattern_strings).expect("Invalid regex patterns in TelemetryAnonymizer");

        let regexes: Vec<(Regex, &'static str)> = pattern_defs
            .into_iter()
            .map(|(p, label)| (Regex::new(p).expect("Invalid regex"), label))
            .collect();

        Self {
            config,
            regex_set: Arc::new(regex_set),
            regexes: Arc::new(regexes),
        }
    }

    /// Fast check if an input string contains any PII pattern.
    pub fn is_sensitive(&self, input: &str) -> bool {
        self.regex_set.is_match(input)
    }

    /// Anonymizes input text by scrubbing sensitive matches.
    pub fn anonymize(&self, input: &str) -> String {
        // High-throughput short circuit
        if !self.regex_set.is_match(input) {
            return input.to_string();
        }

        let mut output = input.to_string();

        for (regex, label) in self.regexes.iter() {
            output = regex
                .replace_all(&output, |caps: &regex::Captures| {
                    let matched = &caps[0];
                    match self.config.strategy {
                        RedactionStrategy::Tag => format!("[REDACTED:{}]", label),
                        RedactionStrategy::TruncatedHash => {
                            let mut hasher = Sha256::new();
                            hasher.update(matched.as_bytes());
                            let result = hasher.finalize();
                            let hash_hex = crate::crypto::hex_encode(result);
                            format!("[{}:{}]", label, &hash_hex[..8])
                        }
                    }
                })
                .to_string();
        }

        output
    }

    /// Applies Laplace differential privacy noise to a numeric value.
    ///
    /// Given sensitivity `b` and privacy budget `epsilon`, generates sample from `Laplace(0, b / epsilon)`.
    pub fn add_differential_privacy_noise(&self, value: f64, sensitivity: f64) -> f64 {
        if !self.config.enable_dp || self.config.dp_epsilon <= 0.0 {
            return value;
        }

        let scale = sensitivity / self.config.dp_epsilon;
        let u: f64 = rand::random::<f64>() - 0.5;
        let noise = -scale * u.signum() * (1.0 - 2.0 * u.abs()).ln();

        value + noise
    }
}
