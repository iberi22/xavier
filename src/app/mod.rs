// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Application layer module with use case implementations
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod health_service;
pub mod memory_usecase;
pub mod proxy_use_case;
pub mod qmd_memory_adapter;
pub mod security_service;
// pub mod session_service; // removed — types not available in current domain layout
pub mod verification_service;

#[cfg(test)]
mod proxy_use_case_tests;
