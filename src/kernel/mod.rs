//! Kernel CLI Execution Proxy & Token Savings Engine
//!
//! Provides intercepted subprocess execution with smart output condensation
//! (git, cargo/test, grep, diff, ls) to conserve LLM context window tokens.

pub mod runner;
pub mod filters;

pub use runner::{execute_proxy_command, ExecutionResult};
