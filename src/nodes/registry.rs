//! Persistent SQLite Registry for SWAL Nodes
//!
//! Stores node metadata, certificates, provider info, visibility, and status.
//! Secrets and private keys are NEVER stored in this database.

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::nodes::cert::NodeCertificate;
use crate::nodes::{NodeRecord, NodeStatus, NodeVisibility, Provider};

/// Persistent node registry backed by SQLite.
pub struct NodeRegistry {
    conn: Mutex<Connection>,
    db_path: Option<PathBuf>,
}

impl NodeRegistry {
    /// Initialize table schema in connection.
    fn init_db(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS node_registry (
                node_id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                visibility TEXT NOT NULL,
                status TEXT NOT NULL,
                pubkey TEXT NOT NULL,
                cert_json TEXT,
                host_key_fingerprint TEXT,
                lease_id TEXT,
                created_at INTEGER NOT NULL,
                last_heartbeat INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_node_visibility ON node_registry(visibility);
            CREATE INDEX IF NOT EXISTS idx_node_status ON node_registry(status);",
        )
        .context("Failed to initialize node_registry SQLite schema")?;
        Ok(())
    }

    /// Open or create the default node registry database.
    ///
    /// Respects the `XAVIER_NODE_REGISTRY_PATH` environment variable if set.
    /// Otherwise defaults to `~/.xavier/node_registry.db`.
    pub fn open_default() -> Result<Self> {
        if let Ok(path_str) = std::env::var("XAVIER_NODE_REGISTRY_PATH") {
            let path = PathBuf::from(path_str);
            return Self::open_path(path);
        }

        let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not resolve home directory"))?;
        let xavier_dir = home.join(".xavier");
        if !xavier_dir.exists() {
            std::fs::create_dir_all(&xavier_dir)
                .context("Failed to create ~/.xavier directory for node registry")?;
        }
        let db_path = xavier_dir.join("node_registry.db");
        Self::open_path(db_path)
    }

    /// Open or create a registry at a specific filesystem path.
    pub fn open_path(path: impl AsRef<Path>) -> Result<Self> {
        let path_buf = path.as_ref().to_path_buf();
        if let Some(parent) = path_buf.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let conn = Connection::open(&path_buf)
            .with_context(|| format!("Failed to open SQLite database at {:?}", path_buf))?;
        Self::init_db(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
            db_path: Some(path_buf),
        })
    }

    /// Open an ephemeral in-memory registry for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .context("Failed to open in-memory SQLite connection")?;
        Self::init_db(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
            db_path: None,
        })
    }

    /// Get the database path if file-backed.
    pub fn db_path(&self) -> Option<&Path> {
        self.db_path.as_deref()
    }

    /// Register or overwrite a node record.
    pub fn register(&self, record: &NodeRecord) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("Failed to acquire registry DB lock"))?;

        let cert_json = record
            .cert
            .as_ref()
            .map(|c| serde_json::to_string(c))
            .transpose()
            .context("Failed to serialize NodeCertificate to JSON")?;

        conn.execute(
            "INSERT INTO node_registry (
                node_id, provider, visibility, status, pubkey, cert_json,
                host_key_fingerprint, lease_id, created_at, last_heartbeat
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(node_id) DO UPDATE SET
                provider = excluded.provider,
                visibility = excluded.visibility,
                status = excluded.status,
                pubkey = excluded.pubkey,
                cert_json = excluded.cert_json,
                host_key_fingerprint = excluded.host_key_fingerprint,
                lease_id = excluded.lease_id,
                last_heartbeat = excluded.last_heartbeat",
            params![
                record.node_id,
                record.provider.to_string(),
                record.visibility.to_string(),
                record.status.to_string(),
                record.pubkey,
                cert_json,
                record.host_key_fingerprint,
                record.lease_id,
                record.created_at as i64,
                record.last_heartbeat.map(|t| t as i64),
            ],
        )
        .context("Failed to insert or update node record")?;

        Ok(())
    }

    /// Retrieve a node record by its node ID.
    pub fn get(&self, node_id: &str) -> Result<Option<NodeRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("Failed to acquire registry DB lock"))?;

        let mut stmt = conn.prepare(
            "SELECT node_id, provider, visibility, status, pubkey, cert_json,
                    host_key_fingerprint, lease_id, created_at, last_heartbeat
             FROM node_registry WHERE node_id = ?1",
        )?;

        let record = stmt
            .query_row(params![node_id], |row| {
                let node_id: String = row.get(0)?;
                let provider_str: String = row.get(1)?;
                let visibility_str: String = row.get(2)?;
                let status_str: String = row.get(3)?;
                let pubkey: String = row.get(4)?;
                let cert_json: Option<String> = row.get(5)?;
                let host_key_fingerprint: Option<String> = row.get(6)?;
                let lease_id: Option<String> = row.get(7)?;
                let created_at: i64 = row.get(8)?;
                let last_heartbeat: Option<i64> = row.get(9)?;

                Ok((
                    node_id,
                    provider_str,
                    visibility_str,
                    status_str,
                    pubkey,
                    cert_json,
                    host_key_fingerprint,
                    lease_id,
                    created_at,
                    last_heartbeat,
                ))
            })
            .optional()?;

        match record {
            Some((
                id,
                p_str,
                v_str,
                s_str,
                pk,
                c_json,
                hk_fp,
                l_id,
                c_at,
                l_hb,
            )) => {
                let provider: Provider = p_str.parse().map_err(|e: anyhow::Error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                    )
                })?;
                let visibility: NodeVisibility = v_str.parse().map_err(|e: anyhow::Error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                    )
                })?;
                let status: NodeStatus = s_str.parse().map_err(|e: anyhow::Error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                    )
                })?;
                let cert: Option<NodeCertificate> = c_json
                    .as_deref()
                    .map(serde_json::from_str)
                    .transpose()
                    .context("Failed to deserialize NodeCertificate from JSON")?;

                Ok(Some(NodeRecord {
                    node_id: id,
                    provider,
                    visibility,
                    status,
                    pubkey: pk,
                    cert,
                    host_key_fingerprint: hk_fp,
                    lease_id: l_id,
                    created_at: c_at as u64,
                    last_heartbeat: l_hb.map(|t| t as u64),
                }))
            }
            None => Ok(None),
        }
    }

    /// List all registered nodes.
    pub fn list(&self) -> Result<Vec<NodeRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("Failed to acquire registry DB lock"))?;

        let mut stmt = conn.prepare(
            "SELECT node_id, provider, visibility, status, pubkey, cert_json,
                    host_key_fingerprint, lease_id, created_at, last_heartbeat
             FROM node_registry ORDER BY created_at DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            let node_id: String = row.get(0)?;
            let provider_str: String = row.get(1)?;
            let visibility_str: String = row.get(2)?;
            let status_str: String = row.get(3)?;
            let pubkey: String = row.get(4)?;
            let cert_json: Option<String> = row.get(5)?;
            let host_key_fingerprint: Option<String> = row.get(6)?;
            let lease_id: Option<String> = row.get(7)?;
            let created_at: i64 = row.get(8)?;
            let last_heartbeat: Option<i64> = row.get(9)?;

            Ok((
                node_id,
                provider_str,
                visibility_str,
                status_str,
                pubkey,
                cert_json,
                host_key_fingerprint,
                lease_id,
                created_at,
                last_heartbeat,
            ))
        })?;

        let mut list = Vec::new();
        for r in rows {
            let (
                id,
                p_str,
                v_str,
                s_str,
                pk,
                c_json,
                hk_fp,
                l_id,
                c_at,
                l_hb,
            ) = r?;
            let provider: Provider = p_str.parse()?;
            let visibility: NodeVisibility = v_str.parse()?;
            let status: NodeStatus = s_str.parse()?;
            let cert: Option<NodeCertificate> = c_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .context("Failed to deserialize NodeCertificate")?;

            list.push(NodeRecord {
                node_id: id,
                provider,
                visibility,
                status,
                pubkey: pk,
                cert,
                host_key_fingerprint: hk_fp,
                lease_id: l_id,
                created_at: c_at as u64,
                last_heartbeat: l_hb.map(|t| t as u64),
            });
        }

        Ok(list)
    }

    /// List only nodes declared with public visibility (for the public directory).
    pub fn list_public(&self) -> Result<Vec<NodeRecord>> {
        let all = self.list()?;
        Ok(all
            .into_iter()
            .filter(|n| n.visibility == NodeVisibility::Public)
            .collect())
    }

    /// Update node lifecycle status.
    pub fn update_status(&self, node_id: &str, status: NodeStatus) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("Failed to acquire registry DB lock"))?;

        let rows_affected = conn.execute(
            "UPDATE node_registry SET status = ?1 WHERE node_id = ?2",
            params![status.to_string(), node_id],
        )?;

        if rows_affected == 0 {
            return Err(anyhow!("Node '{}' not found in registry", node_id));
        }

        Ok(())
    }

    /// Update active lease ID for a node.
    pub fn update_lease(&self, node_id: &str, lease_id: Option<&str>) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("Failed to acquire registry DB lock"))?;

        let rows_affected = conn.execute(
            "UPDATE node_registry SET lease_id = ?1 WHERE node_id = ?2",
            params![lease_id, node_id],
        )?;

        if rows_affected == 0 {
            return Err(anyhow!("Node '{}' not found in registry", node_id));
        }

        Ok(())
    }

    /// Update the last heartbeat timestamp for a node.
    pub fn touch_heartbeat(&self, node_id: &str, timestamp: u64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("Failed to acquire registry DB lock"))?;

        let rows_affected = conn.execute(
            "UPDATE node_registry SET last_heartbeat = ?1 WHERE node_id = ?2",
            params![timestamp as i64, node_id],
        )?;

        if rows_affected == 0 {
            return Err(anyhow!("Node '{}' not found in registry", node_id));
        }

        Ok(())
    }

    /// Remove a node record from the registry.
    pub fn remove(&self, node_id: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("Failed to acquire registry DB lock"))?;

        conn.execute(
            "DELETE FROM node_registry WHERE node_id = ?1",
            params![node_id],
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_record(node_id: &str, visibility: NodeVisibility) -> NodeRecord {
        NodeRecord {
            node_id: node_id.to_string(),
            provider: Provider::Supabase,
            visibility,
            status: NodeStatus::Active,
            pubkey: "0123456789abcdef".to_string(),
            cert: None,
            host_key_fingerprint: None,
            lease_id: Some("lease-1234-uuid".to_string()),
            created_at: 1700000000,
            last_heartbeat: Some(1700000500),
        }
    }

    #[test]
    fn test_registry_crud_in_memory() {
        let registry = NodeRegistry::open_in_memory().unwrap();

        let rec = sample_record("xv1-node-alpha", NodeVisibility::Private);
        registry.register(&rec).unwrap();

        let fetched = registry.get("xv1-node-alpha").unwrap().unwrap();
        assert_eq!(fetched.node_id, "xv1-node-alpha");
        assert_eq!(fetched.provider, Provider::Supabase);
        assert_eq!(fetched.visibility, NodeVisibility::Private);
        assert_eq!(fetched.status, NodeStatus::Active);

        // Update status
        registry
            .update_status("xv1-node-alpha", NodeStatus::Degraded)
            .unwrap();
        let updated = registry.get("xv1-node-alpha").unwrap().unwrap();
        assert_eq!(updated.status, NodeStatus::Degraded);

        // Remove
        registry.remove("xv1-node-alpha").unwrap();
        assert!(registry.get("xv1-node-alpha").unwrap().is_none());
    }

    #[test]
    fn test_registry_disk_persistence_reopen() {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("test_registry.db");

        {
            let registry = NodeRegistry::open_path(&db_path).unwrap();
            let rec = sample_record("xv1-persist-node", NodeVisibility::Public);
            registry.register(&rec).unwrap();
        }

        // Reopen database from disk
        {
            let registry_reopened = NodeRegistry::open_path(&db_path).unwrap();
            let loaded = registry_reopened.get("xv1-persist-node").unwrap().unwrap();
            assert_eq!(loaded.node_id, "xv1-persist-node");
            assert_eq!(loaded.visibility, NodeVisibility::Public);
        }
    }

    #[test]
    fn test_list_public_filters_correctly() {
        let registry = NodeRegistry::open_in_memory().unwrap();

        let public_node = sample_record("xv1-public-1", NodeVisibility::Public);
        let private_node = sample_record("xv1-private-1", NodeVisibility::Private);

        registry.register(&public_node).unwrap();
        registry.register(&private_node).unwrap();

        let all = registry.list().unwrap();
        assert_eq!(all.len(), 2);

        let public_only = registry.list_public().unwrap();
        assert_eq!(public_only.len(), 1);
        assert_eq!(public_only[0].node_id, "xv1-public-1");
    }
}
