//! Telemetry module for agent execution logs, sessions, and DP scrubbing.

pub mod anonymizer;

pub use anonymizer::{AnonymizerConfig, TelemetryAnonymizer};
