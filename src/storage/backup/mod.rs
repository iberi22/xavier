//! SQLite Write-Ahead Logging (WAL) backup module.
//!
//! Note: Unreferenced custom WAL streamer was removed in WAVE-25.01.
//! For WAL handling, use canonical `checkpoint_wal` / `maybe_wal_checkpoint` in storage/code-graph stores.
