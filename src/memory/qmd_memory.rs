// Facade module: re-exports everything from the sibling qmd/ module tree.
//
// This file serves as the public entry point for the qmd_memory module.
// Implementation lives in src/memory/qmd/.
// src/memory/mod.rs declares both `pub mod qmd;` and `pub mod qmd_memory;`.

pub use crate::memory::qmd::*;
