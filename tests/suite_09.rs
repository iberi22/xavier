//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "proxy_security_test.rs"]
mod proxy_security_test;

#[path = "quadratic_voting_edge_test.rs"]
mod quadratic_voting_edge_test;

#[path = "quadratic_voting_test.rs"]
mod quadratic_voting_test;

#[cfg(feature = "local-gllm")]
#[path = "quick_embed_test.rs"]
mod quick_embed_test;

#[path = "rate_limit_integration.rs"]
mod rate_limit_integration;

#[path = "e2e/rbac_e2e.rs"]
mod rbac_e2e;

#[path = "recovery_cli_e2e.rs"]
mod recovery_cli_e2e;

#[path = "recovery_flow.rs"]
mod recovery_flow;

#[path = "recovery_unseal_e2e.rs"]
mod recovery_unseal_e2e;

#[path = "regeneration_edge_test.rs"]
mod regeneration_edge_test;

#[path = "router_harness_auth.rs"]
mod router_harness_auth;

#[path = "router_mesh_peers_auth.rs"]
mod router_mesh_peers_auth;

#[path = "router_notifications_auth.rs"]
mod router_notifications_auth;

#[path = "router_proxy_agents_auth.rs"]
mod router_proxy_agents_auth;
