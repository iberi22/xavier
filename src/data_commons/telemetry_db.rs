use rusqlite::{params, Connection, Result};
use std::path::Path;
use tracing::info;

pub type UsageLogRecord = (String, Vec<u8>, Vec<u8>, String, u64);

/// Tabla para almacenar logs de uso y ejecución (Telemetría Alterna).
/// Exclusivo para nodos mantenedores.
const INIT_SQL: &str = "
    CREATE TABLE IF NOT EXISTS usage_logs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        context_hash TEXT NOT NULL UNIQUE,
        encrypted_payload BLOB NOT NULL,
        encrypted_dek BLOB NOT NULL,
        maintainer_pubkey BLOB NOT NULL,
        timestamp INTEGER NOT NULL,
        wallet_address TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_logs(timestamp);
";

pub struct TelemetryDb {
    conn: Connection,
}

impl TelemetryDb {
    /// Inicializa la base de datos alterna en un archivo separado para no causar conflictos
    /// con la base principal que gestiona el Unified Storage (#75).
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(INIT_SQL)?;
        info!("Telemetry DB initialized correctly for Maintainer Node.");
        Ok(Self { conn })
    }

    /// Guarda un payload cifrado con ECIES (Caja Sellada Asimétrica).
    pub fn save_encrypted_log(
        &self,
        context_hash: &str,
        encrypted_payload: &[u8],
        encrypted_dek: &[u8],
        maintainer_pubkey: &[u8],
        wallet_address: &str,
    ) -> Result<()> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.conn.execute(
            "INSERT OR IGNORE INTO usage_logs (context_hash, encrypted_payload, encrypted_dek, maintainer_pubkey, timestamp, wallet_address)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![context_hash, encrypted_payload, encrypted_dek, maintainer_pubkey, timestamp, wallet_address],
        )?;

        Ok(())
    }

    /// Obtiene todos los logs cifrados para su análisis (solo el nodo mantenedor puede descifrarlos luego).
    pub fn get_recent_logs(&self, limit: u32) -> Result<Vec<UsageLogRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT context_hash, encrypted_payload, encrypted_dek, wallet_address, timestamp 
             FROM usage_logs ORDER BY timestamp DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map([limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, u64>(4)?,
            ))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }

        Ok(results)
    }

    /// Obtiene todos los logs registrados para exportación de entrenamiento.
    pub fn get_all_logs(&self) -> Result<Vec<UsageLogRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT context_hash, encrypted_payload, encrypted_dek, wallet_address, timestamp
             FROM usage_logs ORDER BY timestamp DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, u64>(4)?,
            ))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }

        Ok(results)
    }
}
