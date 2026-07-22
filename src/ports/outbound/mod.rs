//! Outbound port implementations for the hexagonal architecture.
//!
//! Contains adapter implementations for dependency injection ports:
//! - `embedding_port`: Embedding provider abstraction
//! - `health_check_port`: Health check provider trait
//! - `schema_init`: Database schema initialization
//! - `threat_detection_port`: Security threat detection port

pub mod embedding_port;
pub mod health_check_port;
pub mod schema_init;
pub mod threat_detection_port;

pub use embedding_port::EmbeddingPort;
pub use health_check_port::HealthCheckPort;
pub use threat_detection_port::ThreatDetectionPort;
