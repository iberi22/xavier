use crate::ports::outbound::schema_init::SchemaInitializer;
use crate::security::detections::Threat;
use anyhow::Result;
use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::utils::connection_pool::LibsqlConnectionPool;

pub struct SecurityThreatStore {
    pool: LibsqlConnectionPool,
}

impl SecurityThreatStore {
    pub fn new(pool: LibsqlConnectionPool) -> Self {
        Self { pool }
    }

    pub async fn save_threat(&self, threat: &Threat, component: &str) -> Result<()> {
        let conn = self.pool.get().await?;

        // 1. Get previous hash from the chain
        let stmt = conn
            .prepare(
                "SELECT threat_hash FROM security_threat_chain ORDER BY created_at DESC LIMIT 1",
            )
            .await?;
        let mut rows = stmt.query(()).await?;
        let prev_hash = if let Some(row) = rows.next().await? {
            row.get::<String>(0).ok()
        } else {
            None
        };

        // 2. Compute current threat hash for the chain
        let mut hasher = Sha256::new();
        if let Some(ref h) = prev_hash {
            hasher.update(h.as_bytes());
        }
        hasher.update(threat.severity.as_str().as_bytes());
        hasher.update(threat.category.as_str().as_bytes());
        hasher.update(threat.message.as_bytes());
        hasher.update(threat.evidence.as_bytes());
        hasher.update(component.as_bytes());
        let threat_hash = hex::encode(hasher.finalize());

        let id = ulid::Ulid::new().to_string();
        let now = Utc::now().to_rfc3339();

        // 3. Save threat and chain entry in a transaction
        let tx = conn.transaction().await?;

        tx.execute(
            "INSERT INTO security_threats (id, severity, layer, category, message, evidence, context, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            (
                id.clone(),
                threat.severity.as_str().to_string(),
                threat.layer.to_string(),
                threat.category.as_str().to_string(),
                threat.message.to_string(),
                threat.evidence.to_string(),
                component.to_string(),
                now.clone()
            ),
        ).await?;

        tx.execute(
            "INSERT INTO security_threat_chain (id, prev_hash, threat_hash, created_at)
             VALUES (?, ?, ?, ?)",
            (id, prev_hash, threat_hash, now),
        )
        .await?;

        tx.commit().await?;

        Ok(())
    }

    pub async fn init_schema_async(&self) -> Result<()> {
        let conn = self.pool.get().await?;
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
        ).await?;
        Ok(())
    }
}

impl SchemaInitializer for SecurityThreatStore {
    fn init_schema(&self) -> Result<()> {
        std::thread::scope(|s| {
            s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| {
                        anyhow::anyhow!("failed to build runtime for threat schema: {}", e)
                    })?;
                rt.block_on(self.init_schema_async())
            })
            .join()
            .map_err(|_| anyhow::anyhow!("threat schema thread panicked"))?
        })
    }
}
