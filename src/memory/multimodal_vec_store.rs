//! Multimodal Virtual Vector Manager for SQLite
//!
//! Provides isolated virtual vector tables per data modality (e.g., Legal Text 1536d,
//! CLIP Image 768d, Code 384d, Video 768d, Audio 512d) to prevent dimension mismatch
//! crashes in sqlite-vec.

use std::collections::HashMap;
use std::sync::RwLock;

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::memory::sqlite_vec_store::vector;

/// Modality types for isolated vector tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataTypeKind {
    Legal,
    Code,
    Image,
    Video,
    Audio,
}

impl DataTypeKind {
    /// Returns the isolated SQLite table name for this modality.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Legal => "vec_legal",
            Self::Code => "vec_code",
            Self::Image => "vec_image",
            Self::Video => "vec_video",
            Self::Audio => "vec_audio",
        }
    }

    /// Default dimension for the given data type kind.
    pub fn default_dim(&self) -> usize {
        match self {
            Self::Legal => 1536,
            Self::Code => 384,
            Self::Image => 768,
            Self::Video => 768,
            Self::Audio => 512,
        }
    }
}

/// Vector search result item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VectorSearchResult {
    pub item_id: String,
    pub distance: f32,
    pub payload_json: String,
}

/// Manages multi-modal virtual vector tables isolated per modality dimension.
#[derive(Debug, Default)]
pub struct MultimodalVecManager {
    dimensions: RwLock<HashMap<DataTypeKind, usize>>,
}

impl MultimodalVecManager {
    /// Creates a new `MultimodalVecManager` instance.
    pub fn new() -> Self {
        Self {
            dimensions: RwLock::new(HashMap::new()),
        }
    }

    /// Registers the sqlite-vec extension if available.
    pub fn register_extension(&self) -> Result<()> {
        vector::register_sqlite_vec_extension()
    }

    /// Ensures that the virtual vector table for a specific modality exists with the specified dimension.
    pub fn ensure_modality_table(
        &self,
        conn: &Connection,
        modality: DataTypeKind,
        dim: usize,
    ) -> Result<()> {
        let table_name = modality.as_str();

        // Register sqlite-vec auto extension if possible
        let _ = self.register_extension();

        // 1. Create modality metadata tracking table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS multimodal_modality_meta (
                modality TEXT PRIMARY KEY,
                dim INTEGER NOT NULL
            );",
            [],
        )?;

        conn.execute(
            "INSERT OR REPLACE INTO multimodal_modality_meta (modality, dim) VALUES (?1, ?2);",
            params![table_name, dim as i64],
        )?;

        // Update internal dimension cache
        if let Ok(mut dims) = self.dimensions.write() {
            dims.insert(modality, dim);
        }

        // 2. Try creating sqlite-vec virtual table using vec0
        let vec0_sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS {table_name} USING vec0(item_id text primary key, vector float[{dim}], payload_json text);"
        );

        if conn.execute(&vec0_sql, []).is_err() {
            // Fallback: create standard table for pure-Rust cosine distance matching
            let fallback_sql = format!(
                "CREATE TABLE IF NOT EXISTS {table_name} (
                    item_id TEXT PRIMARY KEY,
                    vector BLOB NOT NULL,
                    payload_json TEXT NOT NULL,
                    dim INTEGER NOT NULL
                );"
            );
            conn.execute(&fallback_sql, [])?;
        }

        Ok(())
    }

    /// Retrieves the registered dimension for a modality.
    pub fn get_modality_dim(&self, conn: &Connection, modality: DataTypeKind) -> Result<usize> {
        if let Ok(dims) = self.dimensions.read() {
            if let Some(&dim) = dims.get(&modality) {
                return Ok(dim);
            }
        }

        let table_name = modality.as_str();
        let dim_option: Option<i64> = conn
            .query_row(
                "SELECT dim FROM multimodal_modality_meta WHERE modality = ?1",
                params![table_name],
                |row| row.get(0),
            )
            .ok();

        if let Some(dim) = dim_option {
            let dim_usize = dim as usize;
            if let Ok(mut dims) = self.dimensions.write() {
                dims.insert(modality, dim_usize);
            }
            Ok(dim_usize)
        } else {
            Ok(modality.default_dim())
        }
    }

    /// Inserts or replaces a vector embedding for a given modality.
    pub fn insert_modality_embedding(
        &self,
        conn: &Connection,
        modality: DataTypeKind,
        item_id: &str,
        vector: &[f32],
        payload_json: &str,
    ) -> Result<()> {
        let table_name = modality.as_str();
        let expected_dim = self.get_modality_dim(conn, modality)?;

        if vector.len() != expected_dim {
            return Err(anyhow!(
                "Dimension mismatch for modality '{}': expected {} dimensions, got {}",
                table_name,
                expected_dim,
                vector.len()
            ));
        }

        // Try insert into virtual table vec0 (using DELETE + INSERT since vec0 virtual tables do not support INSERT OR REPLACE)
        let vec_json = serde_json::to_string(vector)?;
        let delete_virtual_sql = format!("DELETE FROM {table_name} WHERE item_id = ?1;");
        let insert_virtual_sql = format!(
            "INSERT INTO {table_name} (item_id, vector, payload_json) VALUES (?1, vec_f32(?2), ?3);"
        );

        let _ = conn.execute(&delete_virtual_sql, params![item_id]);
        if conn
            .execute(&insert_virtual_sql, params![item_id, vec_json, payload_json])
            .is_err()
        {
            // Fallback insert for standard table
            let blob_data = super::sqlite_vec_store::vector::serialize_embedding(vector);
            let insert_fallback_sql = format!(
                "INSERT OR REPLACE INTO {table_name} (item_id, vector, payload_json, dim) VALUES (?1, ?2, ?3, ?4);"
            );
            conn.execute(
                &insert_fallback_sql,
                params![item_id, blob_data, payload_json, expected_dim as i64],
            )?;
        }

        Ok(())
    }

    /// Searches for top_k nearest vector neighbors within a specific modality isolated table.
    pub fn search_modality(
        &self,
        conn: &Connection,
        modality: DataTypeKind,
        query_vector: &[f32],
        top_k: usize,
    ) -> Result<Vec<VectorSearchResult>> {
        let table_name = modality.as_str();
        let expected_dim = self.get_modality_dim(conn, modality)?;

        if query_vector.len() != expected_dim {
            return Err(anyhow!(
                "Query dimension mismatch for modality '{}': expected {} dimensions, got {}",
                table_name,
                expected_dim,
                query_vector.len()
            ));
        }

        let query_json = serde_json::to_string(query_vector)?;
        let search_virtual_sql = format!(
            "SELECT item_id, distance, payload_json FROM {table_name} WHERE vector MATCH vec_f32(?1) AND k = ?2 ORDER BY distance;"
        );

        let virtual_res: Result<Vec<VectorSearchResult>> = (|| {
            let mut stmt = conn.prepare(&search_virtual_sql)?;
            let rows = stmt.query_map(params![query_json, top_k as i64], |row| {
                Ok(VectorSearchResult {
                    item_id: row.get(0)?,
                    distance: row.get(1)?,
                    payload_json: row.get(2)?,
                })
            })?;
            let mut results = Vec::new();
            for r in rows {
                results.push(r?);
            }
            Ok(results)
        })();

        if let Ok(results) = virtual_res {
            return Ok(results);
        }

        // Fallback: Pure Rust Cosine Distance search over standard table
        let fallback_sql = format!("SELECT item_id, vector, payload_json FROM {table_name};");
        let mut stmt = conn.prepare(&fallback_sql)?;
        let mut rows = stmt.query([])?;
        let mut scored_items = Vec::new();

        while let Some(row) = rows.next()? {
            let item_id: String = row.get(0)?;
            let blob_data: Vec<u8> = row.get(1)?;
            let payload_json: String = row.get(2)?;

            let vec = super::sqlite_vec_store::vector::deserialize_embedding(&blob_data);
            let distance = compute_cosine_distance(query_vector, &vec);

            scored_items.push(VectorSearchResult {
                item_id,
                distance,
                payload_json,
            });
        }

        scored_items
            .sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(std::cmp::Ordering::Equal));
        scored_items.truncate(top_k);

        Ok(scored_items)
    }
}

/// Computes Cosine Distance (1.0 - cosine_similarity) between two vectors.
pub fn compute_cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 1.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    if norm_a <= 0.0 || norm_b <= 0.0 {
        return 1.0;
    }

    let similarity = dot / (norm_a.sqrt() * norm_b.sqrt());
    (1.0 - similarity).clamp(0.0, 2.0)
}
