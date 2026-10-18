//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "memory_write_integrity_e2e.rs"]
mod memory_write_integrity_e2e;

#[path = "mesh_auth_binding_test.rs"]
mod mesh_auth_binding_test;

#[path = "mesh_integration.rs"]
mod mesh_integration;

#[cfg(feature = "mesh")]
#[path = "mesh_iroh_test.rs"]
mod mesh_iroh_test;

#[path = "mesh_management_test.rs"]
mod mesh_management_test;

#[path = "mesh_network_test.rs"]
mod mesh_network_test;

#[cfg(feature = "mesh")]
#[path = "mesh_node_lifecycle_test.rs"]
mod mesh_node_lifecycle_test;

#[cfg(feature = "mesh")]
#[path = "mesh_peer_registry_test.rs"]
mod mesh_peer_registry_test;

#[path = "mesh_permissions_test.rs"]
mod mesh_permissions_test;

#[path = "mesh_security_sync_test.rs"]
mod mesh_security_sync_test;

#[path = "messaging_status.rs"]
mod messaging_status;

#[path = "mini_expert_integration_test.rs"]
mod mini_expert_integration_test;

#[path = "e2e/multi_node_sync.rs"]
mod multi_node_sync;

#[path = "e2e/multimodal_rag_pipeline_e2e.rs"]
mod multimodal_rag_pipeline_e2e;
