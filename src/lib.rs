//! Xavier - Cognitive Memory System
#![cfg_attr(feature = "telegram", allow(dead_code))]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(clippy::vec_init_then_push)]
#![allow(clippy::useless_format)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::manual_clamp)]
#![allow(clippy::new_without_default)]
#![allow(clippy::chunks_exact_to_as_chunks)]
#![allow(clippy::useless_format_args)]
#![allow(clippy::redundant_reference)]
#![allow(clippy::redundant_clone)]
//!
//! A cognitive memory system with agent runtime, task management, and native UI.

extern crate self as xavier;
extern crate self as xavier_lib;

pub mod a2a;
pub mod agents;
pub mod api;
pub mod auth2;
pub mod auto_improvement;
pub mod checkpoint;
pub mod chronicle;
pub mod clavis;
pub mod cli;
pub mod codebase;
pub mod consistency;
pub mod consolidation;
pub mod context;
pub mod coordination;
pub mod crypto;
pub mod curation;
pub mod data_commons;
pub mod embedding;
pub mod enterprise;
pub mod error;
pub mod governance;
pub mod health;
pub mod humanchallenge;
pub mod maloca;
pub mod maturity;
pub mod memory;
pub mod mesh;
pub mod messaging;
pub mod middleware;
pub mod node_identity;
pub mod nodes;
pub mod notifications;
pub mod observability;
pub mod plugins;
pub mod polygon_anchor;
pub mod retrieval;
pub mod scheduler;
pub mod search;
pub mod secrets;
pub mod security;
pub mod self_manage;
pub mod server;
pub mod session;
pub mod settings;
pub mod storage;
pub mod sync;
pub mod tasks;
#[cfg(feature = "telegram")]
pub mod telegram;
pub mod tgd;
pub mod tools;
pub mod ui;
pub mod utils;
pub mod verification;
pub mod workspace;

// Hexagonal architecture modules
pub mod adapters;
pub mod app;
pub mod domain;
pub mod ports;
pub mod time;

use std::sync::Arc;

use memory::file_indexer::FileIndexer;
use workspace::WorkspaceRegistry;

use crate::app::security_service::SecurityService;

/// Application state for HTTP server
#[derive(Clone)]
pub struct AppState {
    pub workspace_registry: Arc<WorkspaceRegistry>,
    pub code_indexer: Arc<code_graph::indexer::Indexer>,
    pub code_query: Arc<code_graph::query::QueryEngine>,
    pub code_db: Arc<code_graph::db::CodeGraphDB>,
    pub indexer: FileIndexer,
    pub agent_indexer: crate::memory::agent_indexer::AgentIndexer,
    pub security_service: Arc<SecurityService>,
    pub code_graph_dump_path: Option<std::path::PathBuf>,
}
