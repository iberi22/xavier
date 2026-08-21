//! SQLite Write-Ahead Logging (WAL) continuous replication and recovery module.
//!
//! Provides continuous database replication, periodic full snapshotting,
//! incremental WAL segment streaming, and point-in-time recovery for Issue #1445.

pub mod wal_streamer;

pub use wal_streamer::{
    wal_path_for_db, BackupManifest, RecoveryReport, SnapshotMetadata, WalSegmentMetadata,
    WalStreamer, WalStreamerConfig,
};
