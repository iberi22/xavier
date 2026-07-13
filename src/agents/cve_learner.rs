use serde::{Deserialize, Serialize};

/// Represents a Security Rule extracted from a CVE or an Open Source Pattern.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SecurityRule {
    pub rule_id: String,
    pub description: String,
    pub vulnerable_pattern: String,
    pub recommended_fix: String,
    pub source: String, // e.g., "CVE-2024-1234" or "Linux Kernel PR #909"
}

/// The CVE Learning Agent is responsible for ingesting vulnerability records
/// and translating them into actionable Security Rules for Xavier's CodeGraph.
pub struct CveLearningAgent {
    pub name: String,
}

impl CveLearningAgent {
    pub fn new() -> Self {
        Self {
            name: "Xavier-CVE-Learner".to_string(),
        }
    }

    /// Simulates fetching a CVE JSON 5.0 record on-demand and extracting a rule.
    pub fn ingest_cve_record(
        &self,
        cve_id: &str,
        raw_cve_json: &str,
    ) -> Result<SecurityRule, String> {
        // MOCK: In production, this parses the actual CVE JSON 5.0 schema.
        // It looks for "descriptions", "affected", and "workarounds".
        // Then it passes those strings through a local LLM to generate a deterministic Rule.

        if raw_cve_json.contains("buffer overflow") {
            Ok(SecurityRule {
                rule_id: format!("RULE_{}", cve_id),
                description: "Buffer overflow detected in C/Rust FFI boundary.".to_string(),
                vulnerable_pattern:
                    "unsafe { std::slice::from_raw_parts(ptr, len) } without bounds check"
                        .to_string(),
                recommended_fix:
                    "Implement rigorous bounds checking before unsafe block or use safe wrappers."
                        .to_string(),
                source: cve_id.to_string(),
            })
        } else if raw_cve_json.contains("SQL injection") {
            Ok(SecurityRule {
                rule_id: format!("RULE_{}", cve_id),
                description: "SQL Injection in dynamic queries.".to_string(),
                vulnerable_pattern: "format!(\"SELECT * FROM users WHERE id = {0}\", user_input)"
                    .to_string(),
                recommended_fix: "Use parameterized queries or ORM bindings (e.g., sqlx::query!)."
                    .to_string(),
                source: cve_id.to_string(),
            })
        } else {
            Err("Unrecognized CVE pattern format".to_string())
        }
    }

    /// Translates an extracted rule into an embedding instruction for the CodeGraph.
    pub fn apply_rule_to_codegraph(&self, rule: &SecurityRule) -> String {
        format!(
            "Injected [{src}]: Scanners will now flag '{pat}' and suggest '{fix}'",
            src = rule.source,
            pat = rule.vulnerable_pattern,
            fix = rule.recommended_fix
        )
    }
}

impl Default for CveLearningAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cve_ingestion_and_translation() {
        let agent = CveLearningAgent::new();
        let mock_cve_payload = "{\"cveMetadata\": {\"cveId\": \"CVE-2026-9999\"}, \"descriptions\": [{\"value\": \"A buffer overflow in the FFI parser...\"}]}";

        let rule = agent
            .ingest_cve_record("CVE-2026-9999", mock_cve_payload)
            .unwrap();

        assert_eq!(rule.source, "CVE-2026-9999");
        assert!(rule.vulnerable_pattern.contains("unsafe"));

        let action = agent.apply_rule_to_codegraph(&rule);
        assert!(action.contains("Injected [CVE-2026-9999]"));
    }
}
