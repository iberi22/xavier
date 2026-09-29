//! PageIndex settings. Every `XAVIER_PAGEINDEX_*` env read lives in this file.

use std::path::PathBuf;

const DEFAULT_MAX_RESPONSE_CHARS: usize = 95_000;
const DEFAULT_LINES_PER_PAGE: u32 = 60;
const DEFAULT_SUMMARY_MAX_WORDS: usize = 60;
const DEFAULT_MIN_NODE_TOKENS: u32 = 200;
const DEFAULT_MAX_NODE_TOKENS: u32 = 6000;
const DEFAULT_ARM_WEIGHT: f32 = 1.0;
const DB_FILE_NAME: &str = "pageindex.sqlite3";

#[derive(Debug, Clone, PartialEq)]
pub struct PageIndexSettings {
    /// SQLite tree store path.
    pub db_path: PathBuf,
    /// Directories `path` ingest may read from; empty disables `path` ingest.
    pub ingest_roots: Vec<PathBuf>,
    pub max_response_chars: usize,
    pub lines_per_page: u32,
    pub summarize: bool,
    pub summary_max_words: usize,
    pub min_node_tokens: u32,
    pub max_node_tokens: u32,
    /// Adds the tree-node arm to gating fusion (off by default).
    pub arm_enabled: bool,
    /// RRF weight of the arm relative to the memory layers.
    pub arm_weight: f32,
}

impl PageIndexSettings {
    pub fn from_env() -> Self {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Build from an arbitrary key lookup (keeps tests free of process-env races).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let text = |k: &str| {
            get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        fn num<T: std::str::FromStr>(v: Option<String>, default: T) -> T {
            v.and_then(|s| s.parse().ok()).unwrap_or(default)
        }
        let db_path = text("XAVIER_PAGEINDEX_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(text("XAVIER_DATA_DIR").unwrap_or_else(|| ".".into()))
                    .join(DB_FILE_NAME)
            });
        let ingest_roots = text("XAVIER_PAGEINDEX_INGEST_ROOTS")
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        let summarize = text("XAVIER_PAGEINDEX_SUMMARIZE")
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        let arm_enabled = text("XAVIER_PAGEINDEX_ARM_ENABLED")
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        Self {
            db_path,
            ingest_roots,
            max_response_chars: num(
                text("XAVIER_PAGEINDEX_MAX_RESPONSE_CHARS"),
                DEFAULT_MAX_RESPONSE_CHARS,
            ),
            lines_per_page: num(
                text("XAVIER_PAGEINDEX_LINES_PER_PAGE"),
                DEFAULT_LINES_PER_PAGE,
            ),
            summarize,
            summary_max_words: num(
                text("XAVIER_PAGEINDEX_SUMMARY_MAX_WORDS"),
                DEFAULT_SUMMARY_MAX_WORDS,
            ),
            min_node_tokens: num(
                text("XAVIER_PAGEINDEX_MIN_NODE_TOKENS"),
                DEFAULT_MIN_NODE_TOKENS,
            ),
            max_node_tokens: num(
                text("XAVIER_PAGEINDEX_MAX_NODE_TOKENS"),
                DEFAULT_MAX_NODE_TOKENS,
            ),
            arm_enabled,
            arm_weight: num(text("XAVIER_PAGEINDEX_ARM_WEIGHT"), DEFAULT_ARM_WEIGHT),
        }
    }
}

impl Default for PageIndexSettings {
    fn default() -> Self {
        Self::from_lookup(|_| None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_settings_from_env_defaults() {
        let s = PageIndexSettings::from_lookup(|_| None);
        assert_eq!(s.db_path, PathBuf::from("./pageindex.sqlite3"));
        assert!(s.ingest_roots.is_empty());
        assert_eq!(s.max_response_chars, 95_000);
        assert_eq!(s.lines_per_page, 60);
        assert!(!s.summarize);
        assert_eq!(s.summary_max_words, 60);
        assert_eq!(s.min_node_tokens, 200);
        assert_eq!(s.max_node_tokens, 6000);
        assert!(!s.arm_enabled);
        assert_eq!(s.arm_weight, 1.0);
    }

    #[test]
    fn test_settings_from_env_overrides() {
        let env: HashMap<&str, &str> = HashMap::from([
            ("XAVIER_PAGEINDEX_DB", "/tmp/x/pi.db"),
            ("XAVIER_PAGEINDEX_INGEST_ROOTS", " /a , /b,, "),
            ("XAVIER_PAGEINDEX_MAX_RESPONSE_CHARS", "1234"),
            ("XAVIER_PAGEINDEX_LINES_PER_PAGE", "not-a-number"),
            ("XAVIER_PAGEINDEX_SUMMARIZE", "TRUE"),
        ]);
        let s = PageIndexSettings::from_lookup(|k| env.get(k).map(|v| v.to_string()));
        assert_eq!(s.db_path, PathBuf::from("/tmp/x/pi.db"));
        assert_eq!(
            s.ingest_roots,
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
        assert_eq!(s.max_response_chars, 1234);
        assert_eq!(s.lines_per_page, 60);
        assert!(s.summarize);
    }

    #[test]
    fn test_settings_db_falls_back_to_data_dir() {
        let s = PageIndexSettings::from_lookup(|k| {
            (k == "XAVIER_DATA_DIR").then(|| "/var/x".to_string())
        });
        assert_eq!(s.db_path, PathBuf::from("/var/x/pageindex.sqlite3"));
    }
}
