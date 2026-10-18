//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "router_security_tokens_auth.rs"]
mod router_security_tokens_auth;

#[path = "router_settings_models_auth.rs"]
mod router_settings_models_auth;

#[path = "router_workspaces_db_auth.rs"]
mod router_workspaces_db_auth;

#[path = "secrets_import_env.rs"]
mod secrets_import_env;

#[path = "secrets_redaction_test.rs"]
mod secrets_redaction_test;

#[path = "security_redact.rs"]
mod security_redact;

#[path = "segment_space.rs"]
mod segment_space;

#[path = "semantic_compression_test.rs"]
mod semantic_compression_test;

#[path = "semantic_search_tests.rs"]
mod semantic_search_tests;

#[path = "server_e2e.rs"]
mod server_e2e;

#[path = "session_integration.rs"]
mod session_integration;

#[path = "session_sync_backoff_test.rs"]
mod session_sync_backoff_test;

#[path = "sevier_stress_test.rs"]
mod sevier_stress_test;

#[path = "e2e/shamir_cloud_sync_e2e.rs"]
mod shamir_cloud_sync_e2e;
