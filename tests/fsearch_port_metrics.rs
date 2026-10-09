// Inspired by fsearch (MIT, noahdunnagan/fsearch): typo-still-first metric from demo/vs_fff.py.
//
// Measurement harness: prints one `FSEARCH_PORT_METRICS {json}` line with ok/top1/top5 rates
// per query group (punct, typo, plain). Asserts only that plain and punct queries return Ok.

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();

use chrono::Utc;
use tempfile::tempdir;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::{HybridSearchMode, MemoryRecord, MemoryStore};

const WORKSPACE: &str = "fsearch-port";

// Deterministic synthetic corpus: (path, content).
const CORPUS: &[(&str, &str)] = &[
    (
        "src/cli/server.rs",
        "Entry point for the HTTP and MCP daemon that binds the xavier listeners.",
    ),
    (
        "src/cli/config.rs",
        "Loads CLI configuration from the environment and the xavier config file.",
    ),
    (
        "src/memory/tgd.rs",
        "TgdUtilityPruner decides whether a memory was ever needed using access counts.",
    ),
    (
        "src/consolidation/decay.rs",
        "Exponential decay applied during consolidation with a factor of 0.94 per cycle.",
    ),
    (
        "src/consolidation/minhash.rs",
        "MinHash signatures for near duplicate detection during consolidation.",
    ),
    (
        "src/memory/sqlite_vec_store/store_impl.rs",
        "Sqlite vector store using sqlite-vec with hybrid retrieval and BM25 ranking.",
    ),
    (
        "src/storage/pragma.rs",
        "SQLite pragma setup covering WAL journal mode and the busy timeout.",
    ),
    (
        "config/xavier.config.json",
        "Runtime configuration file with listener ports and feature flags.",
    ),
    (
        "docs/adr/041-session-ingestion.md",
        "ADR 041 session ingestion treats transcripts as untrusted prompt input.",
    ),
    (
        "docs/adr/042-mesh-signing.md",
        "ADR 042 mesh token signing binds public keys to node identifiers.",
    ),
    (
        "docs/SRS/REQUIREMENTS.md",
        "Software requirements specification in IEEE 830 format for memory retrieval.",
    ),
    (
        "scripts/backup/xavier-backup.sh",
        "Nightly backup script that uses VACUUM INTO to copy the vector store.",
    ),
    (
        "src/security/master_key.rs",
        "Handling of master key material and envelope encryption of private rows.",
    ),
    (
        "src/mcp/tools.rs",
        "MCP tool definitions exposed to agents for search and memory save.",
    ),
    (
        "src/ingest/session_gate.rs",
        "Credential screening gate applied on memory ingest before persistence.",
    ),
    (
        "release/CHANGELOG.md",
        "Release notes for version v0.4.1 including retrieval fixes.",
    ),
    (
        "release/legacy-notes.md",
        "Release notes for version v0.3.9, legacy rollout summary.",
    ),
    (
        "incidents/INC-4821.md",
        "Incident INC-4821 about an expired certificate in the staging cluster.",
    ),
    (
        "incidents/INC-4790.md",
        "Incident INC-4790 about a flaky integration test in the release job.",
    ),
    (
        "src/domain/cycle_breaks/w30_10.rs",
        "Query builder that turns free text into FTS5 match expressions for lexical retrieval.",
    ),
    (
        "src/memory/retrieval/rerank.rs",
        "Reranking of retrieval candidates by lexical and vector score fusion.",
    ),
    (
        "src/cli/handlers/memory.rs",
        "CLI handlers for memory commands such as search, get and save.",
    ),
    (
        "src/api/SessionController.rs",
        "SessionController handles payloads for the session ingestion endpoint.",
    ),
    (
        "src/api/tokenRefresh.rs",
        "tokenRefresh rotates bearer tokens for the admin HTTP surface.",
    ),
    (
        "src/graph/beliefEdge.rs",
        "beliefEdge stores weighted relations between entities extracted from memory.",
    ),
    (
        "docs/guides/embedding-setup.md",
        "Embedding providers and how to configure the local embedding model.",
    ),
    (
        "docs/guides/ingestion-pipeline.md",
        "Ingestion pipeline with cursor, batching and throttling of session sources.",
    ),
    (
        "docs/guides/backup-restore.md",
        "Backup and restore procedure for the vector store and key material.",
    ),
    (
        "src/memory/fallback_store.rs",
        "Fallback store used when the primary backend returns an error.",
    ),
    (
        "src/embedding/local.rs",
        "Local embedding backend using a deterministic hashing projection.",
    ),
    (
        "src/tasks/scheduler.rs",
        "Scheduler for periodic tasks such as consolidation and ingestion cycles.",
    ),
    (
        "panel-ui/src/components/SearchBar.tsx",
        "Search bar component in the panel with debounced queries.",
    ),
    (
        "tests/fixtures/notas-consolidacion.md",
        "Notas sobre la consolidacion de memorias y el decaimiento temporal.",
    ),
    (
        "docs/notas/arquitectura-retrieval.md",
        "Arquitectura de la recuperacion hibrida, lexica y vectorial combinadas.",
    ),
    (
        "docs/notas/despliegue-nodo.md",
        "Procedimiento de despliegue del nodo y reinicio del servicio.",
    ),
    (
        "src/governance/clearance.rs",
        "Clearance levels that gate which memories a caller may read.",
    ),
    (
        "src/net/http_limits.rs",
        "Rate limits and body size limits for the HTTP listener.",
    ),
    (
        "src/cli/handlers/skill.rs",
        "Skill dispatch handler that resolves skills by substring ranking.",
    ),
    (
        "docs/runbooks/disk-full.md",
        "Runbook for a full disk, with cleanup steps for temporary files.",
    ),
    (
        "src/observability/metrics.rs",
        "Metrics exporter reporting ingestion latency and retrieval counts.",
    ),
];

// (query, target path). Exact words only: control group.
const PLAIN: &[(&str, &str)] = &[
    ("TgdUtilityPruner", "src/memory/tgd.rs"),
    ("MinHash signatures", "src/consolidation/minhash.rs"),
    ("tokenRefresh", "src/api/tokenRefresh.rs"),
    ("beliefEdge", "src/graph/beliefEdge.rs"),
    ("Reranking", "src/memory/retrieval/rerank.rs"),
    ("Clearance", "src/governance/clearance.rs"),
    ("SessionController", "src/api/SessionController.rs"),
    ("Scheduler", "src/tasks/scheduler.rs"),
    ("Fallback", "src/memory/fallback_store.rs"),
    ("substring", "src/cli/handlers/skill.rs"),
];

// Queries with `/ . - :` punctuation.
const PUNCT: &[(&str, &str)] = &[
    ("src/cli/server.rs", "src/cli/server.rs"),
    ("config/xavier.config.json", "config/xavier.config.json"),
    ("sqlite-vec", "src/memory/sqlite_vec_store/store_impl.rs"),
    ("v0.4.1", "release/CHANGELOG.md"),
    ("INC-4821", "incidents/INC-4821.md"),
    (
        "docs/adr/041-session-ingestion.md",
        "docs/adr/041-session-ingestion.md",
    ),
    (
        "scripts/backup/xavier-backup.sh",
        "scripts/backup/xavier-backup.sh",
    ),
    (
        "src/domain/cycle_breaks/w30_10.rs",
        "src/domain/cycle_breaks/w30_10.rs",
    ),
    (
        "docs/guides/backup-restore.md",
        "docs/guides/backup-restore.md",
    ),
    (
        "src/memory/fallback_store.rs",
        "src/memory/fallback_store.rs",
    ),
];

// Queries with exactly one edit in a word of 5+ letters.
const TYPO: &[(&str, &str)] = &[
    ("Exponental decay", "src/consolidation/decay.rs"),
    ("signaturs minhash", "src/consolidation/minhash.rs"),
    ("Reranikng", "src/memory/retrieval/rerank.rs"),
    ("clearence levels", "src/governance/clearance.rs"),
    ("throtling sessions", "docs/guides/ingestion-pipeline.md"),
    ("SessionControler", "src/api/SessionController.rs"),
    ("beliefEdgee", "src/graph/beliefEdge.rs"),
    ("substrng ranking", "src/cli/handlers/skill.rs"),
    ("Schedluer", "src/tasks/scheduler.rs"),
    ("Fallbak", "src/memory/fallback_store.rs"),
];

struct GroupStats {
    n: usize,
    ok: usize,
    top1: usize,
    top5: usize,
}

impl GroupStats {
    fn rate(part: usize, whole: usize) -> f64 {
        if whole == 0 {
            0.0
        } else {
            part as f64 / whole as f64
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "n": self.n,
            "ok_rate": Self::rate(self.ok, self.n),
            "top1_rate": Self::rate(self.top1, self.ok),
            "top5_rate": Self::rate(self.top5, self.ok),
        })
    }
}

async fn measure(
    store: &VecSqliteMemoryStore,
    queries: &[(&str, &str)],
    mode: HybridSearchMode,
) -> GroupStats {
    let mut stats = GroupStats {
        n: queries.len(),
        ok: 0,
        top1: 0,
        top5: 0,
    };
    for (query, target) in queries {
        let Ok(hits) = store.hybrid_search(WORKSPACE, query, mode, None, 5).await else {
            continue;
        };
        stats.ok += 1;
        if hits.first().is_some_and(|hit| hit.record.path == *target) {
            stats.top1 += 1;
        }
        if hits.iter().any(|hit| hit.record.path == *target) {
            stats.top5 += 1;
        }
    }
    stats
}

#[tokio::test]
async fn fsearch_port_metrics_report() {
    // Keep key material out of the real data dir, as in tests/unified_storage_validation.rs.
    let tmp = tempdir().unwrap();
    let key_blocker = tmp.path().join("no-key-blocker");
    std::fs::write(&key_blocker, b"not a directory").unwrap();
    std::env::set_var("XAVIER_DATA_DIR", key_blocker.join("data"));

    let store = VecSqliteMemoryStore::new(VecSqliteStoreConfig {
        path: tmp.path().join("fsearch-port.db"),
        embedding_dimensions: 3,
    })
    .await
    .unwrap();

    let now = Utc::now();
    for (i, (path, content)) in CORPUS.iter().enumerate() {
        let k = i as f32;
        store
            .put(MemoryRecord {
                id: format!("fsearch-doc-{i}"),
                workspace_id: WORKSPACE.to_string(),
                path: path.to_string(),
                content: content.to_string(),
                embedding: vec![1.0 + (k % 7.0), 1.0, (k % 5.0) / 5.0],
                created_at: now,
                updated_at: now,
                metadata: serde_json::json!({ "clearance": "public" }),
                ..Default::default()
            })
            .await
            .unwrap();
    }

    // Both: default hybrid (vector + lexical). Text: lexical leg only, where the FTS query builder lives.
    let mut report = serde_json::Map::new();
    report.insert("corpus_docs".into(), CORPUS.len().into());
    for (name, mode) in [
        ("both", HybridSearchMode::Both),
        ("text", HybridSearchMode::Text),
    ] {
        let plain = measure(&store, PLAIN, mode).await;
        let punct = measure(&store, PUNCT, mode).await;
        let typo = measure(&store, TYPO, mode).await;
        report.insert(
            name.into(),
            serde_json::json!({
                "punct": punct.to_json(),
                "typo": typo.to_json(),
                "plain": plain.to_json(),
            }),
        );
        if name == "both" {
            assert_eq!(
                plain.ok, plain.n,
                "plain control group must return Ok for every query"
            );
        }
        // Regression guard for literal FTS terms: punctuation must never fail the search.
        assert_eq!(
            punct.ok, punct.n,
            "[{name}] punctuated queries must return Ok"
        );
    }
    let report = serde_json::Value::Object(report);
    println!("FSEARCH_PORT_METRICS {report}");
}
