// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! QMD (Queryable Memory Document) integration
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
// Facade module: re-exports everything from the sibling qmd/ module tree.
//
// This file serves as the public entry point for the qmd_memory module.
// Implementation lives in src/memory/qmd/.
// src/memory/mod.rs declares both `pub mod qmd;` and `pub mod qmd_memory;`.

pub use crate::memory::qmd::*;
