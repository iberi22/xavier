//! HumanChallenge Session Scanner
//!
//! Scans session messages/events for candidate HumanChallenge events
//! matching the 5 challenge types:
//! - Contradiction: conflicting facts or assertions
//! - Decision: explicit architectural or operational decisions
//! - Execution: critical tool or command execution
//! - Assumption: implicit or unverified assumptions
//! - Clarification: ambiguous instructions or missing requirements

use crate::humanchallenge::types::{ChallengeType, HumanChallengeEvent};
use crate::session::types::{SessionEvent, SessionEventType};

/// Session scanner rules and candidate detector
#[derive(Debug, Clone, Default)]
pub struct SessionScanner;

impl SessionScanner {
    pub fn new() -> Self {
        Self
    }

    /// Scans a slice of SessionEvent and extracts candidate HumanChallenge events.
    pub fn scan_session_events(&self, events: &[SessionEvent]) -> Vec<HumanChallengeEvent> {
        let mut candidates = Vec::new();

        for event in events {
            if let Some(content) = &event.content {
                if content.trim().is_empty() {
                    continue;
                }

                // Check for candidates across the 5 canonical types
                if let Some(candidate) = self.detect_contradiction(&event.session_id, content) {
                    candidates.push(candidate);
                } else if let Some(candidate) = self.detect_decision(&event.session_id, content) {
                    candidates.push(candidate);
                } else if let Some(candidate) = self.detect_execution(&event.session_id, event) {
                    candidates.push(candidate);
                } else if let Some(candidate) = self.detect_assumption(&event.session_id, content) {
                    candidates.push(candidate);
                } else if let Some(candidate) = self.detect_clarification(&event.session_id, content) {
                    candidates.push(candidate);
                }
            }
        }

        candidates
    }

    /// Detect Contradiction candidates
    fn detect_contradiction(&self, session_id: &str, content: &str) -> Option<HumanChallengeEvent> {
        let lower = content.to_lowercase();
        let contradiction_keywords = [
            "sin embargo", "por el contrario", "contradice", "en lugar de",
            "conflict", "contradiction", "previously stated", "inconsistent"
        ];

        if contradiction_keywords.iter().any(|kw| lower.contains(kw)) {
            Some(HumanChallengeEvent::new(
                session_id,
                ChallengeType::Contradiction,
                "Posible contradicción detectada en el diálogo de la sesión",
                content,
                0.85,
            ))
        } else {
            None
        }
    }

    /// Detect Decision candidates
    fn detect_decision(&self, session_id: &str, content: &str) -> Option<HumanChallengeEvent> {
        let lower = content.to_lowercase();
        let decision_keywords = [
            "decidimos", "se decide", "optamos por", "elegimos", "la decisión es",
            "we decided", "architecture choice", "decision:", "acordamos"
        ];

        if decision_keywords.iter().any(|kw| lower.contains(kw)) {
            Some(HumanChallengeEvent::new(
                session_id,
                ChallengeType::Decision,
                "Decisión explícita técnica o de diseño registrada",
                content,
                0.90,
            ))
        } else {
            None
        }
    }

    /// Detect Execution candidates
    fn detect_execution(&self, session_id: &str, event: &SessionEvent) -> Option<HumanChallengeEvent> {
        if matches!(event.event_type, SessionEventType::ToolCall | SessionEventType::ToolResult) {
            if let Some(content) = &event.content {
                let lower = content.to_lowercase();
                let exec_keywords = ["rm -rf", "drop table", "deploy", "release", "systemctl", "sudo", "migrate"];
                if exec_keywords.iter().any(|kw| lower.contains(kw)) {
                    return Some(HumanChallengeEvent::new(
                        session_id,
                        ChallengeType::Execution,
                        "Ejecución de herramienta o comando crítico",
                        content,
                        0.95,
                    ));
                }
            }
        }
        None
    }

    /// Detect Assumption candidates
    fn detect_assumption(&self, session_id: &str, content: &str) -> Option<HumanChallengeEvent> {
        let lower = content.to_lowercase();
        let assumption_keywords = [
            "asumiendo", "supongo", "asumimos", "assuming", "hypothesis",
            "presumiblemente", "sin verificar", "unverified"
        ];

        if assumption_keywords.iter().any(|kw| lower.contains(kw)) {
            Some(HumanChallengeEvent::new(
                session_id,
                ChallengeType::Assumption,
                "Suposición o hipótesis no verificada en el flujo",
                content,
                0.80,
            ))
        } else {
            None
        }
    }

    /// Detect Clarification candidates
    fn detect_clarification(&self, session_id: &str, content: &str) -> Option<HumanChallengeEvent> {
        let lower = content.to_lowercase();
        let clarification_keywords = [
            "por favor aclara", "se requiere clarificación", "ambiguo",
            "could you clarify", "need clarification", "unclear instruction"
        ];

        if clarification_keywords.iter().any(|kw| lower.contains(kw)) {
            Some(HumanChallengeEvent::new(
                session_id,
                ChallengeType::Clarification,
                "Petición de clarificación o desambiguación de instrucción",
                content,
                0.85,
            ))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_scan_all_5_challenge_types() {
        let scanner = SessionScanner::new();
        let events = vec![
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::Message,
                timestamp: Utc::now(),
                content: Some("Sin embargo esto contradice lo anterior".into()),
                metadata: None,
            },
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::Message,
                timestamp: Utc::now(),
                content: Some("Decidimos usar la arquitectura hexagonal".into()),
                metadata: None,
            },
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::ToolCall,
                timestamp: Utc::now(),
                content: Some("sudo systemctl restart xavier".into()),
                metadata: None,
            },
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::Message,
                timestamp: Utc::now(),
                content: Some("Asumiendo que el puerto 8006 está libre".into()),
                metadata: None,
            },
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::Message,
                timestamp: Utc::now(),
                content: Some("Por favor aclara la configuración de la base de datos".into()),
                metadata: None,
            },
        ];

        let candidates = scanner.scan_session_events(&events);
        assert_eq!(candidates.len(), 5);
        assert_eq!(candidates[0].challenge_type, ChallengeType::Contradiction);
        assert_eq!(candidates[1].challenge_type, ChallengeType::Decision);
        assert_eq!(candidates[2].challenge_type, ChallengeType::Execution);
        assert_eq!(candidates[3].challenge_type, ChallengeType::Assumption);
        assert_eq!(candidates[4].challenge_type, ChallengeType::Clarification);
    }
}
