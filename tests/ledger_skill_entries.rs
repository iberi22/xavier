//! Ledger self-check for the skill-injection wave (feat-skill-ledger-docs).
//!
//! Runs with CWD = package root. Validates that every `feat-skill-*` entry
//! in `.gitcore/features.json` carries its REQ-060..065 trace, declares
//! tests, and points at existing implementation paths.

use std::collections::HashMap;
use std::path::Path;

const EXPECTED: &[(&str, &str)] = &[
    ("feat-skill-scan-paths", "REQ-060"),
    ("feat-skill-semantic-rank", "REQ-061"),
    ("feat-skill-loader-fix", "REQ-062"),
    ("feat-skill-context-fusion", "REQ-063"),
    ("feat-skill-mcp-tool", "REQ-064"),
    ("feat-skill-ledger-docs", "REQ-065"),
];

fn ledger_features() -> HashMap<String, serde_json::Value> {
    let text = std::fs::read_to_string(".gitcore/features.json").expect("ledger must be readable");
    let ledger: serde_json::Value = serde_json::from_str(&text).expect("ledger must be valid JSON");
    match ledger.get("features").expect("ledger.features") {
        serde_json::Value::Array(list) => list
            .iter()
            .map(|f| {
                (
                    f.get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    f.clone(),
                )
            })
            .collect(),
        serde_json::Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        other => panic!("ledger.features must be list or dict, got {other}"),
    }
}

#[test]
fn test_ledger_skill_entries_valid() {
    let features = ledger_features();
    for (id, req) in EXPECTED {
        let entry = features
            .get(*id)
            .unwrap_or_else(|| panic!("ledger entry missing: {id}"));
        let req_ids: Vec<&str> = entry
            .get("req_ids")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        assert!(
            req_ids.contains(req),
            "{id} must reference {req}, has {req_ids:?}"
        );
        let tests: Vec<&str> = match entry.get("tests") {
            Some(serde_json::Value::Array(a)) => a.iter().filter_map(|v| v.as_str()).collect(),
            Some(serde_json::Value::String(s)) => vec![s.as_str()],
            _ => vec![],
        };
        assert!(!tests.is_empty(), "{id} must declare tests");
        let paths: Vec<&str> = match entry.get("implemented_in") {
            Some(serde_json::Value::Array(a)) => a.iter().filter_map(|v| v.as_str()).collect(),
            Some(serde_json::Value::String(s)) => vec![s.as_str()],
            _ => vec![],
        };
        assert!(!paths.is_empty(), "{id} must declare implemented_in");
        for p in paths {
            // Comma-separated path lists are allowed by the pipeline.
            for one in p.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                assert!(
                    Path::new(one).exists(),
                    "{id} implemented_in path missing: {one}"
                );
            }
        }
    }
}
