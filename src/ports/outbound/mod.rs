//! Outbound port interfaces
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod schema_init;
pub mod threat_detection_port;

pub use threat_detection_port::ThreatDetectionPort;
