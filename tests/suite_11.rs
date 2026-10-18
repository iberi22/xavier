//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "skill_controller_core_adapters.rs"]
mod skill_controller_core_adapters;

#[path = "skill_controller_other_adapters.rs"]
mod skill_controller_other_adapters;

#[path = "skill_route_clearance.rs"]
mod skill_route_clearance;

#[path = "skill_semantic_rank_eval.rs"]
mod skill_semantic_rank_eval;

#[path = "sqlite_vec_validation.rs"]
mod sqlite_vec_validation;

#[path = "stability_ingestion_tests.rs"]
mod stability_ingestion_tests;

#[path = "storage_isolation.rs"]
mod storage_isolation;

#[path = "stress_tests.rs"]
mod stress_tests;

#[path = "swal_benchmarks.rs"]
mod swal_benchmarks;

#[path = "sync_check_handler_cached_result.rs"]
mod sync_check_handler_cached_result;

#[path = "sync_test.rs"]
mod sync_test;

#[path = "telecom_agent_fallback_test.rs"]
mod telecom_agent_fallback_test;

#[path = "telecom_concurrency_stress_test.rs"]
mod telecom_concurrency_stress_test;

#[path = "telecom_file_transfer_e2e_test.rs"]
mod telecom_file_transfer_e2e_test;
