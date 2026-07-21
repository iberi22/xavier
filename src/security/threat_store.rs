//! Threat intelligence storage
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::codebase::connection_manager::ConnectionManager;
use crate::ports::outbound::schema_init::SchemaInitializer;
use crate::security::detections::Threat;
use anyhow::Result;
use chrono::Utc;
use rusqlite::params;
use sha2::{Digest, Sha256};
use tracing::log::warn;

pub struct SecurityThreatStore {
    project_id: String,
}

impl Default for SecurityThreatStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityThreatStore {
    /// Creates a new SecurityThreatStore, connecting to the security database.
    /// Logs a warning if the connection fails (non-fatal for threat store startup).
    pub fn new() -> Self {
        let project_id = "metrics";
        if let Err(e) = ConnectionManager::global().connect(project_id, ".") {
            warn!("SecurityThreatStore failed to connect: {}", e);
        }
        Self {
            project_id: project_id.to_string(),
        }
    }

    /// Save threat.
    pub async fn save_threat(&self, threat: &Threat, component: &str) -> Result<()> {
        let severity = threat.severity.as_str().to_string();
        let layer = threat.layer.to_string();
        let category = threat.category.as_str().to_string();
        let message = threat.message.to_string();
        let evidence = threat.evidence.to_string();
        let component = component.to_string();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            // 1. Get previous hash from the chain
            let prev_hash: Option<String> = conn.query_row(
                "SELECT threat_hash FROM security_threat_chain ORDER BY created_at DESC LIMIT 1",
                (),
                |row| row.get(0)
            ).ok();

            // 2. Compute current threat hash for the chain
            let mut hasher = Sha256::new();
            if let Some(ref h) = prev_hash {
                hasher.update(h.as_bytes());
            }
            hasher.update(severity.as_bytes());
            hasher.update(category.as_bytes());
            hasher.update(message.as_bytes());
            hasher.update(evidence.as_bytes());
            hasher.update(component.as_bytes());
            let threat_hash = crate::crypto::hex_encode(hasher.finalize());

            let id = ulid::Ulid::new().to_string();
            let now = Utc::now().to_rfc3339();

            // 3. Save threat and chain entry in a transaction
            let tx = conn.unchecked_transaction()?;

            tx.execute(
                "INSERT INTO security_threats (id, severity, layer, category, message, evidence, context, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    id,
                    severity,
                    layer,
                    category,
                    message,
                    evidence,
                    component,
                    now.clone()
                ],
            )?;

            tx.execute(
                "INSERT INTO security_threat_chain (id, prev_hash, threat_hash, created_at)
                 VALUES (?, ?, ?, ?)",
                params![id, prev_hash, threat_hash, now],
            )?;

            tx.commit()?;
            Ok(())
        }).await
    }

    /// Init schema async.
    pub async fn init_schema_async(&self) -> Result<()> {
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS security_threats (
                    id TEXT PRIMARY KEY,
                    severity TEXT NOT NULL,
                    layer TEXT NOT NULL,
                    category TEXT NOT NULL,
                    message TEXT NOT NULL,
                    evidence TEXT NOT NULL,
                    context TEXT DEFAULT '',
                    created_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS security_threat_chain (
                    id TEXT PRIMARY KEY,
                    prev_hash TEXT,
                    threat_hash TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_threats_severity ON security_threats(severity);
                CREATE INDEX IF NOT EXISTS idx_threats_created ON security_threats(created_at);
                CREATE INDEX IF NOT EXISTS idx_threat_chain_created ON security_threat_chain(created_at);
                "#,
            )?;
            Ok(())
        }).await
    }
}

impl SchemaInitializer for SecurityThreatStore {
    fn init_schema(&self) -> Result<()> {
        match tokio::runtime::Handle::try_current() {
            Ok(_) => tokio::task::block_in_place(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .map_err(|e| anyhow::anyhow!("failed to create temporary runtime: {}", e))?;
                rt.block_on(self.init_schema_async())
            }),
            Err(_) => {
                let runtime = tokio::runtime::Runtime::new()
                    .map_err(|e| anyhow::anyhow!("failed to create tokio runtime: {}", e))?;
                runtime.block_on(self.init_schema_async())
            }
        }
    }
}
