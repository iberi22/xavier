//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "access_flush_drain.rs"]
mod access_flush_drain;

#[path = "adversarial_challenge_2_stress.rs"]
mod adversarial_challenge_2_stress;

#[path = "agent_lease_hooks_tests.rs"]
mod agent_lease_hooks_tests;

#[path = "agent_task_lifecycle_tests.rs"]
mod agent_task_lifecycle_tests;

// Shim: src/crypto/airgap_capsule.rs (included by airgap_cli_hardening) uses crate::crypto.
#[allow(unused_imports)]
mod crypto {
    pub use xavier::crypto::{hex_decode, hex_encode};
}

#[allow(unused_imports)]
#[path = "airgap_cli_hardening.rs"]
mod airgap_cli_hardening;

#[path = "integration/auth_full_flow_test.rs"]
mod auth_full_flow_test;

#[path = "integration/auth_register_test.rs"]
mod auth_register_test;

#[path = "benchmark.rs"]
mod benchmark;

#[path = "chronicle_harvest_test.rs"]
mod chronicle_harvest_test;

#[path = "ci_version_gate.test.rs"]
mod ci_version_gate;

#[path = "clavis_auth_pipeline_e2e.rs"]
mod clavis_auth_pipeline_e2e;

#[path = "clavis_http_e2e.rs"]
mod clavis_http_e2e;

#[path = "clavis_integration.rs"]
mod clavis_integration;

#[path = "clavis_namespace_e2e.rs"]
mod clavis_namespace_e2e;

#[path = "clavis_vault_smoke_e2e.rs"]
mod clavis_vault_smoke_e2e;

#[path = "skill_store_config.rs"]
mod skill_store_config;
