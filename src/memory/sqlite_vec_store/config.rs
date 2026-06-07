//! Configuration for SQLite vector store
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::settings::XavierSettings;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::OnceLock;

pub const DB_FILENAME: &str = "xavier_memory_vec.db";
pub const DEFAULT_EMBEDDING_DIMENSIONS: usize = 768;
pub const DEFAULT_RRF_K: usize = 60;
pub const DEFAULT_VECTOR_WEIGHT: f32 = 0.40;
pub const DEFAULT_FTS_WEIGHT: f32 = 0.35;
pub const DEFAULT_KG_WEIGHT: f32 = 0.25;
pub const DEFAULT_QJL_THRESHOLD: usize = 30_000;
pub const QJL_MAGIC: &[u8; 4] = b"QJL2";
pub static SQLITE_VEC_EXTENSION_INIT: OnceLock<Result<(), String>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct VecSqliteStoreConfig {
    pub path: PathBuf,
    pub embedding_dimensions: usize,
}

impl VecSqliteStoreConfig {
    pub fn from_env() -> Self {
        let settings = XavierSettings::current();
        let embedding_dimensions = std::env::var("XAVIER_EMBEDDING_DIMENSIONS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or({
                if settings.memory.embedding_dimensions == 0 {
                    DEFAULT_EMBEDDING_DIMENSIONS
                } else {
                    settings.memory.embedding_dimensions
                }
            });

        Self {
            path: std::env::var("XAVIER_MEMORY_VEC_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    if settings.memory.vec_path.trim().is_empty() {
                        PathBuf::from(&settings.memory.data_dir).join(DB_FILENAME)
                    } else {
                        PathBuf::from(&settings.memory.vec_path)
                    }
                }),
            embedding_dimensions,
        }
    }

    pub fn detail(&self) -> String {
        format!(
            "{} ({}d embeddings)",
            self.path.display(),
            self.embedding_dimensions
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_default_constants() {
        assert_eq!(DB_FILENAME, "xavier_memory_vec.db");
        assert_eq!(DEFAULT_EMBEDDING_DIMENSIONS, 768);
        assert_eq!(DEFAULT_RRF_K, 60);
        assert_eq!(DEFAULT_QJL_THRESHOLD, 30_000);
        assert!(DEFAULT_VECTOR_WEIGHT > 0.0);
        assert!(DEFAULT_VECTOR_WEIGHT < 1.0);
        assert!(DEFAULT_FTS_WEIGHT > 0.0);
        assert!(DEFAULT_FTS_WEIGHT < 1.0);
        assert!(DEFAULT_KG_WEIGHT > 0.0);
        assert!(DEFAULT_KG_WEIGHT < 1.0);
        assert_eq!(QJL_MAGIC, b"QJL2");
    }

    #[test]
    fn test_weights_sum_to_one() {
        let total = DEFAULT_VECTOR_WEIGHT + DEFAULT_FTS_WEIGHT + DEFAULT_KG_WEIGHT;
        let diff = (total - 1.0).abs();
        assert!(diff < 0.001, "fusion weights should sum to ~1.0, got {}", total);
    }

    #[test]
    fn test_config_detail_format() {
        let config = VecSqliteStoreConfig {
            path: PathBuf::from("/tmp/test.db"),
            embedding_dimensions: 384,
        };
        let detail = config.detail();
        assert!(detail.contains("/tmp/test.db"));
        assert!(detail.contains("384d"));
    }

    #[test]
    fn test_config_construction() {
        let config = VecSqliteStoreConfig {
            path: PathBuf::from(":memory:"),
            embedding_dimensions: DEFAULT_EMBEDDING_DIMENSIONS,
        };
        assert_eq!(config.embedding_dimensions, DEFAULT_EMBEDDING_DIMENSIONS);
        assert_eq!(config.path, Path::new(":memory:"));
    }
}
