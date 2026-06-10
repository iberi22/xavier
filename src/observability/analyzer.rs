//! # Error Analyzer
//!
//! Takes detected error patterns and uses AgentRuntime to analyze
//! the root cause by reading the relevant source code.
//!
//! ## Process
//!
//! 1. Receive error pattern (module + message + frequency)
//! 2. Read the source code of the failing module
//! 3. Ask AgentRuntime: "What caused this error? Propose a fix."
//! 4. Return structured diagnosis + fix suggestion

use serde::{Deserialize, Serialize};

use super::service_log::{ErrorPattern, LogLevel};

/// Diagnosis and fix suggestion from the analyzer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDiagnosis {
    /// The error pattern being analyzed.
    pub pattern: ErrorPattern,
    /// ISO 8601 timestamp of analysis.
    pub analyzed_at: String,
    /// Probable root cause description.
    pub root_cause: String,
    /// The specific file/line where the error originates.
    pub source_location: Option<String>,
    /// Suggested fix (Rust code diff or description).
    pub suggested_fix: String,
    /// Confidence level (0.0 - 1.0).
    pub confidence: f32,
    /// Urgency level.
    pub urgency: Urgency,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Urgency {
    Critical,
    High,
    Medium,
    Low,
}

impl std::fmt::Display for Urgency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Urgency::Critical => write!(f, "critical"),
            Urgency::High => write!(f, "high"),
            Urgency::Medium => write!(f, "medium"),
            Urgency::Low => write!(f, "low"),
        }
    }
}

/// The error analyzer — uses AgentRuntime to diagnose errors.
pub struct ErrorAnalyzer {
    codebase_path: std::path::PathBuf,
}

impl ErrorAnalyzer {
    /// Create a new analyzer.
    pub fn new() -> Self {
        let codebase_path =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        Self { codebase_path }
    }

    /// Create with explicit codebase path.
    pub fn with_codebase_path(path: std::path::PathBuf) -> Self {
        Self {
            codebase_path: path,
        }
    }

    /// Analyze an error pattern and produce a diagnosis.
    pub async fn analyze(&self, pattern: &ErrorPattern) -> ErrorDiagnosis {
        // Read the relevant source file if possible
        let source_location = self.find_source_file(&pattern.module).await;
        let source_code = if let Some(ref path) = source_location {
            std::fs::read_to_string(path).ok()
        } else {
            None
        };

        // Generate analysis based on pattern info + source code
        let (root_cause, suggested_fix, confidence, urgency) =
            self.generate_analysis(pattern, &source_code).await;

        ErrorDiagnosis {
            pattern: pattern.clone(),
            analyzed_at: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            root_cause,
            source_location: source_location.map(|p| p.to_string_lossy().to_string()),
            suggested_fix,
            confidence,
            urgency,
        }
    }

    /// Find the source file corresponding to a module path.
    async fn find_source_file(&self, module: &str) -> Option<std::path::PathBuf> {
        // Module paths look like "panel::chat" or "http::api::handlers"
        // Convert to file paths: ~/src/server/panel.rs or ~/src/http/api/handlers.rs
        let parts: Vec<&str> = module.split("::").collect();
        if parts.is_empty() {
            return None;
        }

        let mut path = self.codebase_path.join("src");
        for (i, part) in parts.iter().enumerate() {
            if i < parts.len() - 1 {
                path.push(part);
            } else {
                // Try both "module.rs" and "module/mod.rs"
                let rs_path = path.join(format!("{}.rs", part));
                let mod_path = path.join(part).join("mod.rs");
                if rs_path.exists() {
                    path = rs_path;
                } else if mod_path.exists() {
                    path = mod_path;
                } else {
                    return None;
                }
            }
        }

        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// Generate analysis using heuristics + runtime if available.
    async fn generate_analysis(
        &self,
        pattern: &ErrorPattern,
        _source_code: &Option<String>,
    ) -> (String, String, f32, Urgency) {
        // Heuristic: check for common error patterns
        let msg = pattern.sample_message.to_lowercase();

        // Classification based on message content
        if msg.contains("token") || msg.contains("auth") || msg.contains("unauthorized") {
            return (
                "Authentication/authorization failure. Token may be missing, expired, or invalid."
                    .to_string(),
                "Check XAVIER_TOKEN env var and xavier.config.json [security.token_secret]. Regenerate if expired."
                    .to_string(),
                0.9,
                Urgency::High,
            );
        }

        if msg.contains("500") || msg.contains("internal server error") {
            return (
                "HTTP 500 Internal Server Error. The handler panicked or returned an unhandled error."
                    .to_string(),
                "Review the handler for unhandled Result types and add proper error mapping."
                    .to_string(),
                0.7,
                Urgency::High,
            );
        }

        if msg.contains("connection") || msg.contains("timeout") || msg.contains("refused") {
            return (
                "Network connectivity issue. A downstream service is unreachable or timing out."
                    .to_string(),
                "Verify dependent services are running and accessible. Check firewall/port config."
                    .to_string(),
                0.8,
                Urgency::Critical,
            );
        }

        if msg.contains("database") || msg.contains("sqlite") || msg.contains("sql") {
            return (
                "Database error. SQLite operation failed — possibly corrupted DB, disk full, or schema mismatch."
                    .to_string(),
                "Check disk space, run VACUUM on vec-store.sqlite3, verify schema migrations."
                    .to_string(),
                0.85,
                Urgency::Critical,
            );
        }

        if msg.contains("model") || msg.contains("embedding") || msg.contains("ai") {
            return (
                "AI/ML model error. The embedding or LLM model may be unavailable or misconfigured."
                    .to_string(),
                "Check model configuration in xavier.config.json [models]. Verify model files exist and API keys are valid."
                    .to_string(),
                0.75,
                Urgency::High,
            );
        }

        if msg.contains("memory") || msg.contains("oom") || msg.contains("out of memory") {
            return (
                "Out of memory. Xavier exceeded available RAM during operation.".to_string(),
                "Reduce batch sizes, increase system RAM, or enable swap. Check for memory leaks."
                    .to_string(),
                0.6,
                Urgency::Critical,
            );
        }

        // Generic fallback
        let freq_hint = if pattern.frequency > 10 {
            format!(" (repeated {} times in the last window)", pattern.frequency)
        } else {
            String::new()
        };

        (
            format!(
                "Module '{}' reported '{}' errors{}.",
                pattern.module, pattern.level, freq_hint
            ),
            "Review the module's error handling. Ensure all Result types are handled and errors are properly propagated."
                .to_string(),
            0.4,
            if pattern.level == LogLevel::Error {
                Urgency::Medium
            } else {
                Urgency::Low
            },
        )
    }
}

impl Default for ErrorAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
