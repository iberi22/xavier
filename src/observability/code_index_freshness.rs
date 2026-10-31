use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

#[derive(Debug, Serialize, PartialEq)]
pub struct CodeIndexFreshness {
    pub present: bool,
    pub last_indexed_at: Option<String>, /*RFC3339*/
    pub files: u64,
    pub symbols: u64,
    pub languages: BTreeMap<String, u64>,
    pub stale: bool,
}

pub fn probe(db_path: &Path, stale_after: Duration) -> CodeIndexFreshness {
    if !db_path.exists() {
        return CodeIndexFreshness {
            present: false,
            last_indexed_at: None,
            files: 0,
            symbols: 0,
            languages: BTreeMap::new(),
            stale: false,
        };
    }

    let mut freshness = CodeIndexFreshness {
        present: true,
        last_indexed_at: None,
        files: 0,
        symbols: 0,
        languages: BTreeMap::new(),
        stale: true,
    };

    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY;
    let conn = match Connection::open_with_flags(db_path, flags) {
        Ok(c) => c,
        Err(_) => return freshness, // Retains stale: true, zeros.
    };

    if conn.busy_timeout(Duration::from_millis(500)).is_err() {
        return freshness;
    }

    // 1. Files count
    if let Ok(mut stmt) = conn.prepare("SELECT COUNT(*) FROM file_metadata") {
        if let Ok(count) = stmt.query_row([], |row| row.get(0)) {
            freshness.files = count;
        } else {
            return freshness;
        }
    } else {
        return freshness;
    }

    // 2. Symbols count
    if let Ok(mut stmt) = conn.prepare("SELECT COUNT(*) FROM symbols") {
        if let Ok(count) = stmt.query_row([], |row| row.get(0)) {
            freshness.symbols = count;
        } else {
            return freshness;
        }
    } else {
        return freshness;
    }

    // 3. Languages count
    if let Ok(mut stmt) = conn.prepare("SELECT lang, COUNT(*) FROM symbols GROUP BY lang") {
        if let Ok(mut rows) = stmt.query([]) {
            while let Ok(Some(row)) = rows.next() {
                let lang: String = match row.get(0) {
                    Ok(l) => l,
                    Err(_) => return freshness,
                };
                let count: u64 = match row.get(1) {
                    Ok(c) => c,
                    Err(_) => return freshness,
                };
                freshness.languages.insert(lang, count);
            }
        } else {
            return freshness;
        }
    } else {
        return freshness;
    }

    // 4. last_indexed_at
    // Try metadata table first
    let mut last_indexed: Option<SystemTime> = None;
    if let Ok(mut stmt) = conn.prepare("SELECT value FROM metadata WHERE key = 'last_indexed_at'") {
        if let Ok(val) = stmt.query_row([], |row| row.get::<_, String>(0)) {
            if let Ok(dt) = DateTime::parse_from_rfc3339(&val) {
                last_indexed = Some(dt.into());
            }
        }
    }

    // Fallback to mtime of db and wal
    if last_indexed.is_none() {
        let db_mtime = std::fs::metadata(db_path).and_then(|m| m.modified()).ok();
        let wal_path = db_path.with_file_name(format!(
            "{}-wal",
            db_path.file_name().unwrap_or_default().to_string_lossy()
        ));
        let wal_mtime = std::fs::metadata(wal_path).and_then(|m| m.modified()).ok();

        last_indexed = match (db_mtime, wal_mtime) {
            (Some(d), Some(w)) => Some(std::cmp::max(d, w)),
            (Some(d), None) => Some(d),
            (None, Some(w)) => Some(w),
            (None, None) => None,
        };
    }

    if let Some(t) = last_indexed {
        let dt: DateTime<Utc> = t.into();
        freshness.last_indexed_at = Some(dt.to_rfc3339());

        if let Ok(elapsed) = t.elapsed() {
            freshness.stale = elapsed > stale_after;
        } else {
            // time is in the future
            freshness.stale = false;
        }
    } else {
        freshness.stale = true;
    }

    freshness
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_graph::db::CodeGraphDB;
    use code_graph::types::{Language, Symbol, SymbolKind};
    use std::fs;

    #[test]
    fn test_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("missing.db");
        let freshness = probe(&db_path, Duration::from_secs(10));
        assert!(!freshness.present);
        assert!(!freshness.stale);
        assert_eq!(freshness.files, 0);
        assert_eq!(freshness.symbols, 0);
        assert!(!db_path.exists());
    }

    #[test]
    fn test_temp_db_counts() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = CodeGraphDB::new(&db_path).unwrap();

        let sym1 = Symbol {
            id: None,
            stable_id: None,
            name: "f1".into(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "a.rs".into(),
            start_line: 1,
            end_line: 2,
            start_col: 0,
            end_col: 0,
            signature: None,
            parent: None,
            complexity: None,
        };
        let sym2 = Symbol {
            id: None,
            stable_id: None,
            name: "f2".into(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "b.rs".into(),
            start_line: 1,
            end_line: 2,
            start_col: 0,
            end_col: 0,
            signature: None,
            parent: None,
            complexity: None,
        };
        let sym3 = Symbol {
            id: None,
            stable_id: None,
            name: "f3".into(),
            kind: SymbolKind::Function,
            lang: Language::Python,
            file_path: "c.py".into(),
            start_line: 1,
            end_line: 2,
            start_col: 0,
            end_col: 0,
            signature: None,
            parent: None,
            complexity: None,
        };
        db.insert_symbol(&sym1).unwrap();
        db.insert_symbol(&sym2).unwrap();
        db.insert_symbol(&sym3).unwrap();

        let mut m = std::collections::HashMap::new();
        m.insert("a.rs".into(), 100);
        m.insert("b.rs".into(), 100);
        m.insert("c.py".into(), 100);
        db.batch_upsert_file_metadata(m).unwrap();
        db.checkpoint_wal().unwrap();
        // Disconnect by dropping db cache via internal scope or just probe it directly since it's sqlite
        drop(db);

        let freshness = probe(&db_path, Duration::from_secs(86400));
        assert!(freshness.present);
        assert_eq!(freshness.files, 3);
        assert_eq!(freshness.symbols, 3);
        // code-graph stores `lang` as the Debug name of `Language` (e.g. "Rust").
        assert_eq!(freshness.languages.get("Rust"), Some(&2));
        assert_eq!(freshness.languages.get("Python"), Some(&1));
        assert_eq!(freshness.languages.len(), 2);
    }

    #[test]
    fn test_stale_duration() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_stale.db");
        let db = CodeGraphDB::new(&db_path).unwrap();
        db.checkpoint_wal().unwrap();
        drop(db);

        // if stale_after = 0s -> stale should be true
        let freshness_0 = probe(&db_path, Duration::from_secs(0));
        assert!(freshness_0.present);
        assert!(freshness_0.stale);

        // if stale_after = 1 day -> stale should be false
        let freshness_1day = probe(&db_path, Duration::from_secs(86400));
        assert!(freshness_1day.present);
        assert!(!freshness_1day.stale);
    }

    #[test]
    fn test_non_sqlite_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.db");
        fs::write(&path, "not a sqlite db").unwrap();
        let freshness = probe(&path, Duration::from_secs(10));
        assert!(freshness.present);
        assert!(freshness.stale);
        assert_eq!(freshness.files, 0);
        assert_eq!(freshness.symbols, 0);
    }
}
