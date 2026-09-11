//! CLI mirror-export / mirror-import commands.
//!
//! Neutral package format (JSON Lines UTF-8, one object per line) for mirroring
//! CONTENT + GRAPH between a Xavier node and a foreign store (e.g. Pocket
//! Cerebro, an Isar/Dart graph). Embeddings are intentionally NOT mirrored:
//! different models live in different vector spaces and copying them produces
//! garbage searches without a visible error — each side recomputes its own.
//!
//! Node line (memory):
//! ```jsonl
//! {"t":"node","id":"<sha256>","kind":"memory","content":"...","metadata":{...},"hash":"<sha256>","ts":"<ISO8601>"}
//! ```
//! Node line (entity): same shape with `kind:"entity"`, `content` = entity
//! name and `metadata.entity_type` set.
//!
//! Edge line:
//! ```jsonl
//! {"t":"edge","from":"<sha256>","to":"<sha256>","relation":"...","weight":0.8,"kind":"relations|memory_entities"}
//! ```
//!
//! `kind` selects the target table. A missing or unknown `kind` is treated as
//! a plain relation between the endpoints (never discarded). `source` is an
//! optional free-form provenance tag from the foreign package and never drives
//! the dispatch.
//!
//! `id`/`hash` are content hashes (dedupe key). `from`/`to` reference hashes,
//! never local row ids. The package carries no embeddings, no DEKs, no IVs.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;

use xavier::memory::sqlite_vec_store::at_rest;
use xavier::memory::store::MemoryRecord;

/// Neutral workspace assigned to imported rows (the package carries none).
const MIRROR_WORKSPACE: &str = "mirror";
/// Path prefix for imported memory records.
const MIRROR_PATH_PREFIX: &str = "mirror";
/// Node kinds.
const NODE_MEMORY: &str = "memory";
const NODE_ENTITY: &str = "entity";
/// Edge kinds: which target table the edge lands in. `source` stays a
/// free-form provenance attribute carried by the foreign package.
const EDGE_RELATIONS: &str = "relations";
const EDGE_MEMORY_ENTITIES: &str = "memory_entities";
/// Entity type reserved for internal memory-node placeholders (never mirrored).
const ENTITY_TYPE_MEMORY: &str = "memory";

/// SHA-256 hex digest of `content`: the package-wide dedupe key.
fn content_hash(content: &str) -> String {
    xavier::crypto::hex_encode(Sha256::digest(content.as_bytes()))
}

/// One JSONL node line.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MirrorNodeLine {
    #[serde(rename = "t")]
    t: String,
    id: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    metadata: Value,
    #[serde(default)]
    hash: String,
    #[serde(default)]
    ts: String,
}

/// One JSONL edge line.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MirrorEdgeLine {
    #[serde(rename = "t")]
    t: String,
    from: String,
    to: String,
    #[serde(default)]
    relation: String,
    #[serde(default)]
    weight: f64,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    source: String,
}

/// Resolve the local vec-store DB path (same order as the store config).
fn vec_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("XAVIER_MEMORY_VEC_PATH") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    let settings = xavier::settings::XavierSettings::current();
    if !settings.memory.vec_path.trim().is_empty() {
        PathBuf::from(&settings.memory.vec_path)
    } else {
        PathBuf::from(&settings.memory.data_dir)
            .join(xavier::memory::sqlite_vec_store::config::DB_FILENAME)
    }
}

/// Handle `mirror-export --out <file> [--limit N]`.
pub async fn handle_mirror_export(out: PathBuf, limit: Option<usize>) -> Result<()> {
    let db_path = vec_db_path();
    if !db_path.exists() {
        anyhow::bail!("database not found at {}", db_path.display());
    }

    let node_key = at_rest::resolve_record_key();
    let conn =
        Connection::open(&db_path).with_context(|| format!("cannot open {}", db_path.display()))?;
    let jsonl = export_jsonl(&conn, limit, node_key.as_ref())?;

    let mut f =
        std::fs::File::create(&out).with_context(|| format!("cannot create {}", out.display()))?;
    f.write_all(jsonl.as_bytes())
        .with_context(|| format!("cannot write {}", out.display()))?;
    let lines = jsonl.lines().count();
    println!("✅ Exported {lines} line(s) to {}", out.display());
    Ok(())
}

/// Handle `mirror-import --in <file>`.
pub async fn handle_mirror_import(input: PathBuf) -> Result<()> {
    let db_path = vec_db_path();
    if !db_path.exists() {
        anyhow::bail!("database not found at {}", db_path.display());
    }

    let jsonl = std::fs::read_to_string(&input)
        .with_context(|| format!("cannot read {}", input.display()))?;
    let conn =
        Connection::open(&db_path).with_context(|| format!("cannot open {}", db_path.display()))?;
    let stats = import_jsonl(&conn, &jsonl)?;

    println!(
        "✅ Imported: {} memor(y|ies), {} entit(y|ies), {} relation(s), {} memory-entity link(s) \
         ({} node(s) skipped, {} edge(s) skipped)",
        stats.memories,
        stats.entities,
        stats.relations,
        stats.memory_entities,
        stats.skipped_nodes,
        stats.skipped_edges,
    );
    Ok(())
}

/// Build the JSONL package from a live connection. Records are decrypted on
/// the way out: exported content is readable plaintext, never hex ciphertext.
fn export_jsonl(
    conn: &Connection,
    limit: Option<usize>,
    node_key: Option<&[u8; 32]>,
) -> Result<String> {
    let mut lines: Vec<String> = Vec::new();

    // 1. Memory nodes (decrypted content).
    let mut mem_hashes: HashMap<String, String> = HashMap::new();
    {
        let sql = match limit {
            Some(n) => format!(
                "SELECT id, content, metadata, encrypted_dek, content_iv, metadata_iv, updated_at \
                 FROM memory_records ORDER BY created_at LIMIT {n}"
            ),
            None => {
                "SELECT id, content, metadata, encrypted_dek, content_iv, metadata_iv, updated_at \
                     FROM memory_records ORDER BY created_at"
                    .to_string()
            }
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,          // id
                row.get::<_, String>(1)?,          // content
                row.get::<_, String>(2)?,          // metadata
                row.get::<_, Option<Vec<u8>>>(3)?, // encrypted_dek
                row.get::<_, Option<Vec<u8>>>(4)?, // content_iv
                row.get::<_, Option<Vec<u8>>>(5)?, // metadata_iv
                row.get::<_, String>(6)?,          // updated_at
            ))
        })?;
        for row in rows {
            let (id, content, metadata, dek, civ, miv, updated_at) = row?;
            let mut rec = MemoryRecord {
                id: id.clone(),
                content,
                metadata: serde_json::from_str(&metadata).unwrap_or_default(),
                encrypted_dek: dek,
                content_iv: civ,
                metadata_iv: miv,
                ..Default::default()
            };
            at_rest::decrypt_with_resolved_key(&mut rec, node_key)
                .with_context(|| format!("cannot decrypt record {id} (set XAVIER_RECORD_KEY?)"))?;
            let hash = content_hash(&rec.content);
            mem_hashes.insert(id, hash.clone());
            lines.push(serde_json::to_string(&json!({
                "t": "node",
                "id": hash,
                "kind": NODE_MEMORY,
                "content": rec.content,
                "metadata": rec.metadata,
                "hash": hash,
                "ts": updated_at,
            }))?);
        }
    }

    // 2. Entity nodes (name as content). Internal memory-node placeholders
    //    (entity_type == "memory") are skipped: they are derived state.
    let mut entity_hashes: HashMap<String, String> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, name, entity_type, properties FROM entities WHERE entity_type != ?1",
        )?;
        let rows = stmt.query_map(params![ENTITY_TYPE_MEMORY], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        for row in rows {
            let (id, name, entity_type, properties) = row?;
            let hash = content_hash(&name);
            entity_hashes.insert(id, hash.clone());
            let mut metadata = json!({ "entity_type": entity_type });
            if let Some(props) = properties {
                if let Ok(v) = serde_json::from_str::<Value>(&props) {
                    if !v.is_null() {
                        metadata["properties"] = v;
                    }
                }
            }
            lines.push(serde_json::to_string(&json!({
                "t": "node",
                "id": hash,
                "kind": NODE_ENTITY,
                "content": name,
                "metadata": metadata,
                "hash": hash,
                "ts": chrono::Utc::now().to_rfc3339(),
            }))?);
        }
    }

    // 2b. Placeholder entities (entity_type == "memory") are derived state and
    //     are not exported as nodes, but they are the endpoints of every row in
    //     `relations`: map each placeholder to its memory so those edges travel
    //     as memory<->memory instead of being silently dropped. The link lives
    //     in `properties.memory_id`, and the id itself is "mem:<ws>:<memory_id>":
    //     both are read so a placeholder resolves whatever convention wrote it.
    let mut placeholder_memories: HashMap<String, String> = HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT id, properties FROM entities WHERE entity_type = ?1")?;
        let rows = stmt.query_map(params![ENTITY_TYPE_MEMORY], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })?;
        for row in rows {
            let (entity_id, properties) = row?;
            let from_props = properties
                .as_deref()
                .and_then(|p| serde_json::from_str::<Value>(p).ok())
                .and_then(|v| v.get("memory_id").and_then(Value::as_str).map(str::to_owned));
            let resolved = from_props.or_else(|| {
                entity_id
                    .strip_prefix("mem:")
                    .and_then(|rest| rest.split_once(':'))
                    .map(|(_, memory_id)| memory_id.to_string())
            });
            if let Some(memory_id) = resolved {
                placeholder_memories.insert(entity_id, memory_id);
            }
        }
    }

    // 3. memory_entities edges (memory -> entity).
    {
        let mut stmt =
            conn.prepare("SELECT memory_id, entity_id, relation_type FROM memory_entities")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        for row in rows {
            let (memory_id, entity_id, relation_type) = row?;
            let (Some(from_hash), Some(to_hash)) =
                (mem_hashes.get(&memory_id), entity_hashes.get(&entity_id))
            else {
                continue;
            };
            lines.push(serde_json::to_string(&json!({
                "t": "edge",
                "from": from_hash,
                "to": to_hash,
                "relation": relation_type.unwrap_or_default(),
                "weight": 1.0,
                "kind": EDGE_MEMORY_ENTITIES,
            }))?);
        }
    }

    // 4. relations edges (entity -> entity).
    {
        let mut stmt =
            conn.prepare("SELECT source_id, target_id, relation_type, weight FROM relations")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?;
        for row in rows {
            let (source_id, target_id, relation_type, weight) = row?;
            // An endpoint is either a real entity (exported as a node) or a
            // memory placeholder (resolved to its memory). Anything else has no
            // node in the package, so the edge would be an orphan on the other
            // side: drop it here rather than ship a dangling reference.
            let resolve = |id: &str| -> Option<&String> {
                entity_hashes
                    .get(id)
                    .or_else(|| placeholder_memories.get(id).and_then(|m| mem_hashes.get(m)))
            };
            let (Some(from_hash), Some(to_hash)) = (resolve(&source_id), resolve(&target_id)) else {
                continue;
            };
            lines.push(serde_json::to_string(&json!({
                "t": "edge",
                "from": from_hash,
                "to": to_hash,
                "relation": relation_type,
                "weight": weight,
                "kind": EDGE_RELATIONS,
            }))?);
        }
    }

    if lines.is_empty() {
        Ok(String::new())
    } else {
        Ok(lines.join("\n") + "\n")
    }
}

/// Import statistics (inserted rows, plus skipped duplicates).
#[derive(Debug, Default)]
struct ImportStats {
    memories: usize,
    entities: usize,
    relations: usize,
    memory_entities: usize,
    skipped_nodes: usize,
    skipped_edges: usize,
}

/// Insert an already-encrypted memory record.
fn insert_memory(conn: &Connection, rec: &MemoryRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO memory_records (id, workspace_id, path, content, metadata, encrypted_dek, \
         content_iv, metadata_iv, created_at, updated_at, revision, primary_flag, level, relation, \
         revisions, embedding_status, embedding_attempts) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            rec.id,
            rec.workspace_id,
            rec.path,
            rec.content,
            serde_json::to_string(&rec.metadata).unwrap_or_default(),
            rec.encrypted_dek,
            rec.content_iv,
            rec.metadata_iv,
            rec.created_at.to_rfc3339(),
            rec.updated_at.to_rfc3339(),
            rec.revision,
            rec.primary as i32,
            rec.level.as_str(),
            serde_json::to_string(&rec.relation).unwrap_or_default(),
            serde_json::to_string(&rec.revisions).unwrap_or_default(),
            rec.embedding_status,
            rec.embedding_attempts,
        ],
    )?;
    Ok(())
}

/// Resolve an edge endpoint hash to an entity id. A memory node has no row in
/// `entities`, so it gets (or reuses) a deterministic placeholder entity of
/// type `ENTITY_TYPE_MEMORY`; anything else resolves through the id map to the
/// entity row imported for that hash.
fn resolve_edge_entity(
    conn: &Connection,
    id_map: &HashMap<String, String>,
    memory_hashes: &HashSet<String>,
    hash: &str,
) -> Option<String> {
    if memory_hashes.contains(hash) {
        let placeholder_id = xavier::memory::store::stable_key("mirror_ph", &[hash]);
        conn.execute(
            "INSERT OR IGNORE INTO entities (id, name, entity_type, workspace_id) \
             VALUES (?1, ?2, ?3, ?4)",
            params![placeholder_id, hash, ENTITY_TYPE_MEMORY, MIRROR_WORKSPACE],
        )
        .ok()?;
        Some(placeholder_id)
    } else {
        id_map.get(hash).cloned()
    }
}

/// Parse and apply a JSONL package. Idempotent: importing the same package
/// twice inserts nothing the second time.
fn import_jsonl(conn: &Connection, jsonl: &str) -> Result<ImportStats> {
    let mut stats = ImportStats::default();

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mirror_id_map(origin_hash TEXT PRIMARY KEY, local_id TEXT);",
    )?;

    // Load origin-hash -> local-id mappings persisted by previous imports.
    let mut id_map: HashMap<String, String> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT origin_hash, local_id FROM mirror_id_map")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (hash, local_id) = row?;
            id_map.insert(hash, local_id);
        }
    }

    // Content-hash index over plaintext (legacy, unencrypted) rows so a
    // package does not re-insert content that already exists locally.
    let mut existing_plaintext: HashMap<String, String> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT id, content, encrypted_dek FROM memory_records")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<Vec<u8>>>(2)?,
            ))
        })?;
        for row in rows {
            let (id, content, dek) = row?;
            if dek.as_deref().is_none_or(|b| b.is_empty()) {
                existing_plaintext.insert(content_hash(&content), id);
            }
        }
    }

    // Parse lines into nodes + edges (order preserved for edge resolution).
    let mut nodes: Vec<MirrorNodeLine> = Vec::new();
    let mut edges: Vec<MirrorEdgeLine> = Vec::new();
    for (idx, line) in jsonl.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .with_context(|| format!("invalid JSON at line {}", idx + 1))?;
        match value.get("t").and_then(|v| v.as_str()).unwrap_or_default() {
            "node" => nodes.push(
                serde_json::from_value(value)
                    .with_context(|| format!("bad node object at line {}", idx + 1))?,
            ),
            "edge" => edges.push(
                serde_json::from_value(value)
                    .with_context(|| format!("bad edge object at line {}", idx + 1))?,
            ),
            _ => {}
        }
    }

    // Origin hashes that resolve to memory nodes (vs entities). Rebuilt on
    // every import — including from skipped duplicate node lines — so edge
    // endpoints stay classifiable across idempotent re-imports.
    let mut memory_hashes: HashSet<String> = HashSet::new();

    // Insert nodes.
    for node in &nodes {
        match node.kind.as_str() {
            // Un nodo SIN `kind` es una memoria: el campo lo anadio el lado
            // Xavier por su cuenta y el paquete de Pocket no lo trae, asi que
            // exigirlo descartaba todos los nodos del vecino como entidades.
            NODE_MEMORY | "" => {
                let hash = if node.hash.is_empty() {
                    content_hash(&node.content)
                } else {
                    node.hash.clone()
                };
                memory_hashes.insert(hash.clone());
                if id_map.contains_key(&hash) {
                    stats.skipped_nodes += 1;
                    continue;
                }
                if let Some(local_id) = existing_plaintext.get(&hash) {
                    id_map.insert(hash.clone(), local_id.clone());
                    stats.skipped_nodes += 1;
                    continue;
                }
                let local_id = ulid::Ulid::new().to_string();
                let mut rec = MemoryRecord {
                    id: local_id.clone(),
                    workspace_id: MIRROR_WORKSPACE.to_string(),
                    path: format!("{}/{}", MIRROR_PATH_PREFIX, hash),
                    content: node.content.clone(),
                    metadata: node.metadata.clone(),
                    ..Default::default()
                };
                let _encrypted = at_rest::encrypt_columns_for_write(&mut rec)?;
                insert_memory(conn, &rec)?;
                id_map.insert(hash.clone(), local_id.clone());
                conn.execute(
                    "INSERT OR REPLACE INTO mirror_id_map (origin_hash, local_id) VALUES (?1, ?2)",
                    params![hash, local_id],
                )?;
                stats.memories += 1;
            }
            NODE_ENTITY => {
                let hash = if node.hash.is_empty() {
                    content_hash(&node.content)
                } else {
                    node.hash.clone()
                };
                if id_map.contains_key(&hash) {
                    stats.skipped_nodes += 1;
                    continue;
                }
                let existing_id: Option<String> = conn
                    .query_row(
                        "SELECT id FROM entities WHERE name = ?1 LIMIT 1",
                        params![node.content],
                        |row| row.get(0),
                    )
                    .optional()?;
                if let Some(eid) = existing_id {
                    id_map.insert(hash.clone(), eid);
                    stats.skipped_nodes += 1;
                    continue;
                }
                let entity_type = node
                    .metadata
                    .get("entity_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("manual")
                    .to_string();
                let local_id = ulid::Ulid::new().to_string();
                conn.execute(
                    "INSERT OR IGNORE INTO entities (id, name, entity_type, workspace_id) \
                     VALUES (?1, ?2, ?3, ?4)",
                    params![local_id, node.content, entity_type, MIRROR_WORKSPACE],
                )?;
                id_map.insert(hash.clone(), local_id.clone());
                conn.execute(
                    "INSERT OR REPLACE INTO mirror_id_map (origin_hash, local_id) VALUES (?1, ?2)",
                    params![hash, local_id],
                )?;
                stats.entities += 1;
            }
            _ => {
                stats.skipped_nodes += 1;
            }
        }
    }

    // Rebuild edges resolving hashes to local ids.
    for edge in &edges {
        let (Some(from_local), Some(to_local)) = (id_map.get(&edge.from), id_map.get(&edge.to))
        else {
            stats.skipped_edges += 1;
            continue;
        };
        match edge.kind.as_str() {
            EDGE_MEMORY_ENTITIES => {
                let edge_id = xavier::memory::store::stable_key(
                    "mirror_me",
                    &[&edge.from, &edge.to, &edge.relation],
                );
                let inserted = conn.execute(
                    "INSERT OR IGNORE INTO memory_entities (id, workspace_id, memory_id, entity_id, relation_type) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![edge_id, MIRROR_WORKSPACE, from_local, to_local, edge.relation],
                )?;
                stats.memory_entities += inserted;
            }
            // `relations` kind, missing kind, or any unknown value: a plain
            // relation between the endpoints. NEVER discarded just because the
            // kind is absent (Pocket ships no `kind`). Memory endpoints get
            // placeholder entities; entity endpoints resolve directly.
            _ => {
                let Some(from_entity) =
                    resolve_edge_entity(conn, &id_map, &memory_hashes, &edge.from)
                else {
                    stats.skipped_edges += 1;
                    continue;
                };
                let Some(to_entity) =
                    resolve_edge_entity(conn, &id_map, &memory_hashes, &edge.to)
                else {
                    stats.skipped_edges += 1;
                    continue;
                };
                let edge_id = xavier::memory::store::stable_key(
                    "mirror_rel",
                    &[&edge.from, &edge.to, &edge.relation],
                );
                let inserted = conn.execute(
                    "INSERT OR IGNORE INTO relations (id, source_id, target_id, relation_type, weight, workspace_id) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![edge_id, from_entity, to_entity, edge.relation, edge.weight, MIRROR_WORKSPACE],
                )?;
                stats.relations += inserted;
            }
        }
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CANARY: &str = "CANARIO-SECRETO-987654321";

    fn test_key(fill: u8) -> [u8; 32] {
        [fill; 32]
    }

    // Import encrypts via `encrypt_columns_for_write`, which resolves the key
    // from the env/file. Tests pin the env var (single-threaded: safe) so the
    // imported rows are decryptable with the same test key.
    fn set_record_key(key: &[u8; 32]) -> Option<String> {
        let saved = std::env::var(at_rest::RECORD_KEY_ENV).ok();
        std::env::set_var(at_rest::RECORD_KEY_ENV, xavier::crypto::hex_encode(key));
        saved
    }

    fn restore_record_key(saved: Option<String>) {
        match saved {
            Some(v) => std::env::set_var(at_rest::RECORD_KEY_ENV, v),
            None => std::env::remove_var(at_rest::RECORD_KEY_ENV),
        }
    }

    fn schema() -> &'static str {
        r#"
        CREATE TABLE IF NOT EXISTS memory_records (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL,
            path TEXT NOT NULL,
            content TEXT NOT NULL,
            metadata TEXT NOT NULL DEFAULT '{}',
            embedding BLOB,
            encrypted_dek BLOB,
            content_iv BLOB,
            metadata_iv BLOB,
            created_at DATETIME NOT NULL,
            updated_at DATETIME NOT NULL,
            revision INTEGER NOT NULL DEFAULT 1,
            primary_flag INTEGER DEFAULT 1,
            parent_id TEXT,
            cluster_id TEXT,
            level TEXT DEFAULT 'atom',
            relation TEXT,
            revisions TEXT,
            embedding_status TEXT DEFAULT 'pending',
            embedding_attempts INTEGER DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            entity_type TEXT NOT NULL,
            properties TEXT,
            language_family TEXT,
            workspace_id TEXT
        );
        CREATE TABLE IF NOT EXISTS relations (
            id TEXT PRIMARY KEY,
            source_id TEXT NOT NULL,
            target_id TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            properties TEXT,
            weight REAL DEFAULT 1.0,
            confidence_score REAL DEFAULT 1.0,
            provenance_id TEXT,
            contradicts_edge_id TEXT,
            is_inferred INTEGER DEFAULT 0,
            source_language TEXT,
            target_language TEXT,
            workspace_id TEXT,
            created_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now')),
            updated_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))
        );
        CREATE TABLE IF NOT EXISTS memory_entities (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL,
            memory_id TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            relation_type TEXT
        );
        "#
    }

    fn open_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(schema()).unwrap();
        conn
    }

    fn seed_memory(
        conn: &Connection,
        key: &[u8; 32],
        id: &str,
        content: &str,
        meta: Value,
    ) -> MemoryRecord {
        let mut rec = MemoryRecord {
            id: id.to_string(),
            workspace_id: "ws1".to_string(),
            path: format!("notes/{id}.md"),
            content: content.to_string(),
            metadata: meta,
            ..Default::default()
        };
        at_rest::encrypt_columns_for_write_with_key(&mut rec, key).unwrap();
        insert_memory(conn, &rec).unwrap();
        rec
    }

    #[test]
    fn mirror_export_decrypts_content() {
        let key = test_key(0x5A);
        let conn = open_db();
        seed_memory(&conn, &key, "m1", CANARY, json!({"topic": "canary"}));

        let jsonl = export_jsonl(&conn, None, Some(&key)).unwrap();
        assert!(
            jsonl.contains(CANARY),
            "exported content must be readable plaintext, got:\n{jsonl}"
        );
        assert!(
            !jsonl.contains("\"embedding\""),
            "package must not carry embeddings"
        );
        assert!(
            !jsonl.contains("encrypted_dek"),
            "package must not leak DEKs"
        );
        assert!(!jsonl.contains("content_iv"), "package must not leak IVs");
        // Stored column is hex ciphertext; the export must never echo it.
        let stored: String = conn
            .query_row(
                "SELECT content FROM memory_records WHERE id = 'm1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!stored.contains(CANARY));
    }

    #[test]
    fn mirror_export_emits_edges_through_placeholders() {
        let key = test_key(0x7C);
        let conn = open_db();
        seed_memory(&conn, &key, "m1", "memoria uno", json!({}));
        seed_memory(&conn, &key, "m2", "memoria dos", json!({}));

        // La base real cuelga las aristas memoria<->memoria de los placeholders
        // (entity_type == "memory"), que NO se exportan como nodos. Su id es
        // "mem:<workspace>:<memory_id>" y su properties lleva memory_id.
        for (ph, memory_id) in [("mem:ws1:m1", "m1"), ("mem:ws1:m2", "m2")] {
            conn.execute(
                "INSERT INTO entities (id, name, entity_type, workspace_id, properties) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    ph,
                    format!("placeholder/{memory_id}"),
                    ENTITY_TYPE_MEMORY,
                    "ws1",
                    json!({ "memory_id": memory_id }).to_string()
                ],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO memory_entities (id, workspace_id, memory_id, entity_id, relation_type) \
             VALUES ('me1', 'ws1', 'm1', 'mem:ws1:m1', 'about')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO memory_entities (id, workspace_id, memory_id, entity_id, relation_type) \
             VALUES ('me2', 'ws1', 'm2', 'mem:ws1:m2', 'about')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO relations (id, source_id, target_id, relation_type, weight, workspace_id) \
             VALUES ('r1', 'mem:ws1:m1', 'mem:ws1:m2', 'supports', 0.8, 'ws1')",
            [],
        )
        .unwrap();

        let jsonl = export_jsonl(&conn, None, Some(&key)).unwrap();
        let aristas: Vec<Value> = jsonl
            .lines()
            .filter(|l| l.contains("\"t\":\"edge\""))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        assert_eq!(aristas.len(), 1, "la relacion debe viajar como arista:\n{jsonl}");
        let edge = &aristas[0];
        assert_eq!(edge["relation"], "supports");
        assert_eq!(edge["kind"], EDGE_RELATIONS);
        // Los extremos son las MEMORIAS, no los placeholders no exportados.
        assert_eq!(edge["from"], content_hash("memoria uno"));
        assert_eq!(edge["to"], content_hash("memoria dos"));
        for extremo in [&edge["from"], &edge["to"]] {
            assert!(
                jsonl.contains(&format!("\"id\":{extremo}")),
                "extremo sin nodo en el paquete: {extremo}"
            );
        }
    }

    #[test]
    fn mirror_roundtrip_counts_and_content() {
        let key = test_key(0x6B);
        let src = open_db();
        seed_memory(&src, &key, "m1", "memoria uno", json!({"n": 1}));
        seed_memory(&src, &key, "m2", "memoria dos", json!({"n": 2}));
        let entity_id = "entity_1";
        src.execute(
            "INSERT INTO entities (id, name, entity_type, workspace_id) VALUES (?1, ?2, ?3, ?4)",
            params![entity_id, "rust", "language", "ws1"],
        )
        .unwrap();
        src.execute(
            "INSERT INTO memory_entities (id, workspace_id, memory_id, entity_id, relation_type) \
             VALUES ('me1', 'ws1', 'm1', 'entity_1', 'mentions')",
            [],
        )
        .unwrap();

        let jsonl = export_jsonl(&src, None, Some(&key)).unwrap();
        // 2 memory nodes + 1 entity node + 1 edge.
        assert_eq!(jsonl.lines().count(), 4);

        let dst = open_db();
        let saved_key = set_record_key(&key);
        let stats = import_jsonl(&dst, &jsonl).unwrap();
        restore_record_key(saved_key);
        assert_eq!(stats.memories, 2);
        assert_eq!(stats.entities, 1);
        assert_eq!(stats.memory_entities, 1);

        let memories: i64 = dst
            .query_row("SELECT COUNT(*) FROM memory_records", [], |r| r.get(0))
            .unwrap();
        let entities: i64 = dst
            .query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))
            .unwrap();
        let links: i64 = dst
            .query_row("SELECT COUNT(*) FROM memory_entities", [], |r| r.get(0))
            .unwrap();
        assert_eq!(memories, 2);
        assert_eq!(entities, 1);
        assert_eq!(links, 1);

        // Imported rows are encrypted at rest, but decrypt back to the originals.
        let mut contents: Vec<String> = Vec::new();
        {
            let mut stmt = dst
                .prepare("SELECT id, content, metadata, encrypted_dek, content_iv, metadata_iv FROM memory_records")
                .unwrap();
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<Vec<u8>>>(3)?,
                        row.get::<_, Option<Vec<u8>>>(4)?,
                        row.get::<_, Option<Vec<u8>>>(5)?,
                    ))
                })
                .unwrap();
            for row in rows {
                let (id, content, metadata, dek, civ, miv) = row.unwrap();
                let mut rec = MemoryRecord {
                    id,
                    content,
                    metadata: serde_json::from_str(&metadata).unwrap_or_default(),
                    encrypted_dek: dek,
                    content_iv: civ,
                    metadata_iv: miv,
                    ..Default::default()
                };
                at_rest::decrypt_record_with_key(&mut rec, &key).unwrap();
                contents.push(rec.content);
            }
        }
        contents.sort();
        assert_eq!(
            contents,
            vec!["memoria dos".to_string(), "memoria uno".to_string()]
        );
    }

    #[test]
    fn mirror_import_idempotent() {
        let key = test_key(0x7C);
        let src = open_db();
        seed_memory(&src, &key, "m1", "único contenido", json!({}));
        let jsonl = export_jsonl(&src, None, Some(&key)).unwrap();

        let dst = open_db();
        let saved_key = set_record_key(&key);
        let first = import_jsonl(&dst, &jsonl).unwrap();
        assert_eq!(first.memories, 1);

        let second = import_jsonl(&dst, &jsonl).unwrap();
        restore_record_key(saved_key);
        assert_eq!(
            second.memories, 0,
            "second import must not duplicate memories"
        );
        assert_eq!(second.skipped_nodes, 1);

        let total: i64 = dst
            .query_row("SELECT COUNT(*) FROM memory_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 1);
        let map: i64 = dst
            .query_row("SELECT COUNT(*) FROM mirror_id_map", [], |r| r.get(0))
            .unwrap();
        assert_eq!(map, 1);
    }

    #[test]
    fn mirror_export_limit() {
        let key = test_key(0x8D);
        let conn = open_db();
        seed_memory(&conn, &key, "m1", "uno", json!({}));
        seed_memory(&conn, &key, "m2", "dos", json!({}));
        let jsonl = export_jsonl(&conn, Some(1), Some(&key)).unwrap();
        assert_eq!(jsonl.lines().count(), 1);
        assert!(jsonl.contains("uno") || jsonl.contains("dos"));
    }

    // Pocket ships edges with `source: "seed"` (provenance) and NO `kind`.
    // Those edges link two memory nodes, so the import must create one
    // placeholder entity per endpoint plus one `relations` row — never skip.
    #[test]
    fn mirror_import_memory_to_memory_edge_without_kind() {
        let key = test_key(0x9E);
        let dst = open_db();
        let saved_key = set_record_key(&key);

        let hash_a = content_hash("memoria A");
        let hash_b = content_hash("memoria B");
        let package = format!(
            "{}\n{}\n{}\n",
            json!({ "t": "node", "id": hash_a, "content": "memoria A" }),
            json!({ "t": "node", "id": hash_b, "content": "memoria B" }),
            json!({
                "t": "edge",
                "from": hash_a,
                "to": hash_b,
                "relation": "supports",
                "weight": 0.8,
                "source": "seed"
            }),
        );

        let first = import_jsonl(&dst, &package).unwrap();
        assert_eq!(first.memories, 2);
        assert_eq!(
            first.relations, 1,
            "memory<->memory edge must land in relations"
        );
        assert_eq!(first.skipped_edges, 0, "edge must not be skipped");

        let second = import_jsonl(&dst, &package).unwrap();
        restore_record_key(saved_key);
        assert_eq!(
            second.relations, 0,
            "second import must not duplicate the relation"
        );

        let placeholders: i64 = dst
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE entity_type = 'memory'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(placeholders, 2, "one placeholder entity per endpoint");

        let rels: i64 = dst
            .query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rels, 1);
    }
}
