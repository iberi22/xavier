//! U7 test group root: several integration test crates compiled into one binary.
//! Each module is an unchanged file under tests/, included with #[path]; the module
//! name is the former [[test]] target name, so nextest test names keep that prefix.
//! Feature-gated files keep their own #![cfg(feature)] semantics via the outer #[cfg].

// Grouped files share helper modules and keep per-file helpers that a single crate may not use.
#![allow(dead_code, clippy::duplicate_mod, clippy::module_inception)]

xavier::isolate_test_process!();

#[path = "telecom_packet_fuzz_test.rs"]
mod telecom_packet_fuzz_test;

#[path = "telecom_simulation_test.rs"]
mod telecom_simulation_test;

#[path = "e2e/telegram_bot_e2e.rs"]
mod telegram_bot_e2e;

#[path = "telemetry_anonymizer_edge_test.rs"]
mod telemetry_anonymizer_edge_test;

#[path = "telemetry_anonymizer_test.rs"]
mod telemetry_anonymizer_test;

#[path = "test_cross_encoder.rs"]
mod test_cross_encoder;

#[path = "test_gpud_sidecar.rs"]
mod test_gpud_sidecar;

#[path = "test_hc_e2e.rs"]
mod test_hc_e2e;

#[path = "test_humanchallenge.rs"]
mod test_humanchallenge;

#[path = "test_maloca_auth_gate.rs"]
mod test_maloca_auth_gate;

#[path = "test_maloca_http_e2e.rs"]
mod test_maloca_http_e2e;

#[path = "test_mesh_keystore.rs"]
mod test_mesh_keystore;

// Shim: src/notifications/db.rs (included by test_notifications_db) uses crate::storage.
mod storage {
    pub use xavier::storage::pragma::apply_pragmas;
}

#[allow(unused_imports)]
#[path = "test_notifications_db.rs"]
mod test_notifications_db;

#[path = "test_search_noise_and_get.rs"]
mod test_search_noise_and_get;
