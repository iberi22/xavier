// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! UI modules for Xavier
//!
//! This provides various UI components for different interfaces.

#[cfg(feature = "cli-interactive")]
pub mod dashboard;
#[cfg(feature = "cli-interactive")]
pub mod log_stream;
#[cfg(feature = "cli-interactive")]
pub mod memory_view;
