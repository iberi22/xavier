use crate::memory::sqlite_vec_store::types::ExtractedEntity;
use crate::memory::sqlite_vec_store::utils;
use crate::memory::store::{stable_key, GraphHopPath, MemoryRecord};
use anyhow::Result;
use rusqlite::{params, Connection};
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

pub fn extract_entities(content: &str) -> Vec<ExtractedEntity> {
    static MENTION_RE: OnceLock<Regex> = OnceLock::new();
    static TOPIC_RE: OnceLock<Regex> = OnceLock::new();
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    static DATE_RE: OnceLock<Regex> = OnceLock::new();

    let mention_re =
        MENTION_RE.get_or_init(|| Regex::new(r"@[\w.-]{2,}").expect("valid mention regex"));
    let topic_re = TOPIC_RE.get_or_init(|| Regex::new(r"#[\w-]{2,}").expect("valid topic regex"));
    let url_re =
        URL_RE.get_or_init(|| Regex::new(r#"https?://[^\s)>"]+"#).expect("valid url entity regex"));
    let date_re = DATE_RE.get_or_init(|| {
        Regex::new(
            r"\b(\d{4}-\d{2}-\d{2}|\d{4}/\d{2}/\d{2}|(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\s+\d{1,2},\s+\d{4})\b",
        )
        .expect("valid date entity regex")
    });

    let mut entities = Vec::new();
    let mut seen = HashSet::new();
    for (regex, entity_type, relation_type) in [
        (mention_re, "mention", "mentions"),
        (topic_re, "topic", "tags"),
        (url_re, "url", "references"),
        (date_re, "date", "dated_on"),
    ] {
        for matched in regex.find_iter(content) {
            let value = matched.as_str().trim().to_string();
            let key = format!("{entity_type}:{}", value.to_ascii_lowercase());
            if seen.insert(key) {
                entities.push(ExtractedEntity {
                    value,
                    entity_type,
                    relation_type,
                });
            }
        }
    }

    entities
}

pub fn memory_node_id(workspace_id: &str, memory_id: &str) -> String {
    format!("mem:{}:{}", workspace_id, memory_id)
}

pub fn entity_node_id(workspace_id: &str, entity_type: &str, value: &str) -> String {
    stable_key(
        "entity",
        &[
            workspace_id,
            entity_type,
            &value.trim().to_ascii_lowercase(),
        ],
    )
}

pub fn sync_memory_entities(
    conn: &Connection,
    workspace_id: &str,
    record: &MemoryRecord,
) -> Result<()> {
    if !utils::entity_extraction_enabled() {
        return Ok(());
    }

    let memory_node_id = memory_node_id(workspace_id, &record.id);
    conn.execute(
        "INSERT OR REPLACE INTO entities (id, name, entity_type, properties) VALUES (?, ?, ?, ?)",
        params![
            memory_node_id,
            record.path,
            "memory",
            serde_json::json!({
                "memory_id": record.id,
                "path": record.path,
                "workspace_id": workspace_id,
            })
            .to_string()
        ],
    )?;

    conn.execute(
        "DELETE FROM memory_entities WHERE workspace_id = ? AND memory_id = ?",
        params![workspace_id, record.id],
    )?;
    conn.execute(
        "DELETE FROM relations WHERE source_id = ?",
        params![memory_node_id],
    )?;

    for entity in extract_entities(&record.content) {
        let entity_id = entity_node_id(workspace_id, entity.entity_type, &entity.value);
        conn.execute(
            "INSERT OR REPLACE INTO entities (id, name, entity_type, properties) VALUES (?, ?, ?, ?)",
            params![
                entity_id,
                entity.value,
                entity.entity_type,
                serde_json::json!({
                    "workspace_id": workspace_id,
                    "normalized": entity.value.to_ascii_lowercase(),
                })
                .to_string()
            ],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO memory_entities (id, workspace_id, memory_id, entity_id, relation_type) VALUES (?, ?, ?, ?, ?)",
            params![
                stable_key("memory_entity_link", &[workspace_id, &record.id, &entity_id]),
                workspace_id,
                record.id,
                entity_id,
                entity.relation_type,
            ],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO relations (id, source_id, target_id, relation_type, properties, confidence_score, provenance_id) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                stable_key("memory_relation", &[workspace_id, &memory_node_id, &entity_id, entity.relation_type]),
                memory_node_id,
                entity_id,
                entity.relation_type,
                serde_json::json!({
                    "memory_id": record.id,
                    "path": record.path,
                    "entity_type": entity.entity_type,
                })
                .to_string(),
                1.0,
                record.id
            ],
        )?;
    }

    Ok(())
}

pub fn resolve_graph_seed_entities(
    conn: &Connection,
    workspace_id: &str,
    source: &MemoryRecord,
    query: &str,
) -> Result<HashSet<String>> {
    let mut seeds = HashSet::new();
    seeds.insert(memory_node_id(workspace_id, &source.id));

    // Also seed from entities mentioned in the query
    let terms = utils::search_tokens(query);
    let mut entity_stmt = conn
        .prepare("SELECT id FROM entities WHERE name LIKE ?")?;
    for term in terms {
        let mut entity_rows = entity_stmt.query(params![format!("%{term}%")])?;
        while let Some(row) = entity_rows.next()? {
            seeds.insert(row.get(0)?);
        }
    }

    Ok(seeds)
}

pub fn traverse_recursive(
    _conn: &Connection,
    _workspace_id: &str,
    _seeds: HashSet<String>,
    _hops: usize,
    _query: &str,
) -> Result<Vec<GraphHopPath>> {
    // Unused: Recursive traversal is currently handled via CTE in backend_impl.rs
    Ok(Vec::new())
}
