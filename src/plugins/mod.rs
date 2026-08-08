//! Xavier Plugin Module.
//!
//! Provides the runtime loader and runner for external plugins.

pub mod runtime;

pub use runtime::XavierPluginRuntime;

/// Returns the current version of the Xavier plugin system.
pub fn get_version() -> &'static str {
    "0.1.0"
}

/// Helper function to create a new plugin runtime wrapping an existing PluginManager.
pub fn create_runtime(
    manager: std::sync::Arc<code_graph::plugin::PluginManager>,
) -> XavierPluginRuntime {
    XavierPluginRuntime::new(manager)
}
