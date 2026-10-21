//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "d2_clavis_wiring_red.rs"]
mod d2_clavis_wiring_red;

#[path = "data_commons.rs"]
mod data_commons;

#[path = "e2e/decentralized_login_e2e.rs"]
mod decentralized_login_e2e;

#[path = "delivery_scripts.rs"]
mod delivery_scripts;

#[path = "distributed_tracer_test.rs"]
mod distributed_tracer_test;

#[path = "docs_adr.rs"]
mod docs_adr;

#[path = "doctor_bounded_test.rs"]
mod doctor_bounded_test;

#[path = "e2e_chat_local.rs"]
mod e2e_chat_local;

#[path = "e2e_ecosystem_verification.rs"]
mod e2e_ecosystem_verification;

#[path = "embedding_fallback_resilience_test.rs"]
mod embedding_fallback_resilience;

#[path = "embedding_fallback_test.rs"]
mod embedding_fallback_test;

#[path = "embedding_local_integration.rs"]
mod embedding_local_integration;

#[path = "encrypt_private_e2e.rs"]
mod encrypt_private_e2e;

#[path = "env_guard.rs"]
mod env_guard;

#[path = "error_test.rs"]
mod error_test;

#[path = "skill_usage_telemetry.rs"]
mod skill_usage_telemetry;
