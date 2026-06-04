//! Codebase module — per-project SQLite databases.
//!
//! Each repository gets its own `.xavier/codebase.db` (git + code data)
//! plus a separate private conversations DB at `~/.xavier/conversations/{project_id}.db`.

pub mod conversations_db;
pub mod db;
