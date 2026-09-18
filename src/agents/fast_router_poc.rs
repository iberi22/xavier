use serde::{Deserialize, Serialize};

/// Represents the classification result from our internal "System One" logic.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[allow(dead_code)]
pub struct SystemOneDecision {
    pub confidence: f32,
    pub is_mechanical: bool,
    pub has_pii: bool,
    pub route: Route,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[allow(dead_code)]
pub enum Route {
    /// Fast lane: Pure Rust logic or extremely lightweight local models.
    FastLane,
    /// System Two lane: Heavy LLM inference for complex reasoning.
    SystemTwoLane,
}

/// A Proof of Concept internal router inspired by the System One architectural pattern.
#[allow(dead_code)]
pub struct FastRouter {
    confidence_threshold: f32,
}

impl FastRouter {
    pub fn new(confidence_threshold: f32) -> Self {
        Self {
            confidence_threshold,
        }
    }

    /// Evaluates the context without generating strings (simulating constrained decoding / typed outputs).
    pub fn triage(
        &self,
        input: &str,
        is_tool_continuation: bool,
        has_error: bool,
    ) -> SystemOneDecision {
        // Simulated local classification logic:
        // In a real implementation, this could use a local ONNX model or Rust-based heuristics
        // to assign a confidence score and boolean flags.

        // If it's a clean tool continuation with no errors, it's a mechanical step.
        let is_mechanical = is_tool_continuation && !has_error;

        // Simulate PII detection logic (e.g., regex or local classifier)
        let has_pii = input.contains("SSN") || input.contains("password");

        let confidence = if is_mechanical && !has_pii {
            // Highly confident it's a mechanical, safe task.
            0.95
        } else if has_error {
            // Ambiguous or requires deep reasoning to fix the error.
            0.45
        } else {
            // Standard prompt, fallback to System Two.
            0.85
        };

        let route = if confidence >= self.confidence_threshold && is_mechanical {
            Route::FastLane
        } else {
            Route::SystemTwoLane
        };

        SystemOneDecision {
            confidence,
            is_mechanical,
            has_pii,
            route,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_router_mechanical_continuation() {
        let router = FastRouter::new(0.90);

        // A mechanical continuation with no error should hit the FastLane
        let decision = router.triage("Processing standard data...", true, false);
        assert_eq!(decision.route, Route::FastLane);
        assert!(decision.is_mechanical);
    }

    #[test]
    fn test_fast_router_error_fallback() {
        let router = FastRouter::new(0.90);

        // A continuation with an error should drop confidence and fallback to SystemTwoLane
        let decision = router.triage("Failed to parse JSON", true, true);
        assert_eq!(decision.route, Route::SystemTwoLane);
        assert!(decision.confidence < 0.90);
    }

    #[test]
    fn test_fast_router_pii_detection() {
        let router = FastRouter::new(0.90);

        // PII presence should be flagged
        let decision = router.triage("User password is 123", false, false);
        assert!(decision.has_pii);
        assert_eq!(decision.route, Route::SystemTwoLane);
    }
}
