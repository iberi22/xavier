use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const PACK_MAGIC: &[u8] = b"SWALPACK1\n";
pub const ALLOWED_ENTRIES: [&str; 3] = ["memories.jsonl", "vectors.sqlite", "code_graph.jsonl"];

pub struct PackEntry {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[rustfmt::skip]
#[derive(Debug, Error)]
pub enum PackError {
    #[error("unknown entry: {0}")] UnknownEntry(String),
    #[error("duplicate entry: {0}")] DuplicateEntry(String),
    #[error("pack is empty")] Empty,
    #[error("pack is corrupt: {0}")] Corrupt(String),
    #[error("pack is truncated")] Truncated,
    #[error("invalid namespace: {0}")] InvalidNamespace(String),
    #[error("missing entry: {0}")] MissingEntry(String),
}

#[rustfmt::skip]
#[derive(Serialize, Deserialize)]
struct ManifestEntry { name: String, sha256: String, len: usize }

#[rustfmt::skip]
#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    entries: Vec<ManifestEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,
}

pub fn write_pack(entries: &[PackEntry]) -> Result<Vec<u8>, PackError> {
    write_pack_with_namespace(entries, None)
}

pub fn write_pack_with_namespace(
    entries: &[PackEntry],
    namespace: Option<&str>,
) -> Result<Vec<u8>, PackError> {
    if entries.is_empty() {
        return Err(PackError::Empty);
    }
    let mut manifest_entries = Vec::new();
    let mut names = std::collections::HashSet::new();
    for e in entries {
        if !ALLOWED_ENTRIES.contains(&e.name.as_str()) {
            return Err(PackError::UnknownEntry(e.name.clone()));
        }
        if !names.insert(&e.name) {
            return Err(PackError::DuplicateEntry(e.name.clone()));
        }
        let mut h = Sha256::new();
        h.update(&e.bytes);
        manifest_entries.push(ManifestEntry {
            name: e.name.clone(),
            sha256: hex::encode(h.finalize()),
            len: e.bytes.len(),
        });
    }
    let m = Manifest {
        version: 1,
        entries: manifest_entries,
        namespace: namespace.map(|s| s.to_string()),
    };
    let m_bytes = serde_json::to_vec(&m).map_err(|e| PackError::Corrupt(e.to_string()))?;

    let mut out = Vec::new();
    out.extend_from_slice(PACK_MAGIC);
    out.extend_from_slice(&(m_bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(&m_bytes);
    for e in entries {
        out.extend_from_slice(&e.bytes);
    }
    Ok(out)
}

pub fn read_pack(bytes: &[u8]) -> Result<Vec<PackEntry>, PackError> {
    read_pack_info(bytes).map(|(entries, _)| entries)
}

pub fn read_pack_info(bytes: &[u8]) -> Result<(Vec<PackEntry>, Option<String>), PackError> {
    if bytes.len() < PACK_MAGIC.len() + 8 {
        return Err(PackError::Truncated);
    }
    if &bytes[..PACK_MAGIC.len()] != PACK_MAGIC {
        return Err(PackError::Corrupt("bad magic".into()));
    }
    let mut off = PACK_MAGIC.len();
    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&bytes[off..off + 8]);
    let m_len = u64::from_be_bytes(len_bytes) as usize;
    off += 8;
    if off.checked_add(m_len).map_or(true, |end| bytes.len() < end) {
        return Err(PackError::Truncated);
    }
    let m: Manifest = serde_json::from_slice(&bytes[off..off + m_len])
        .map_err(|e| PackError::Corrupt(e.to_string()))?;
    off += m_len;

    let mut result = Vec::new();
    let mut names = std::collections::HashSet::new();
    for me in m.entries {
        if off
            .checked_add(me.len)
            .map_or(true, |end| bytes.len() < end)
        {
            return Err(PackError::Truncated);
        }
        let e_bytes = &bytes[off..off + me.len];
        let mut h = Sha256::new();
        h.update(e_bytes);
        if hex::encode(h.finalize()) != me.sha256 {
            return Err(PackError::Corrupt("hash".into()));
        }
        if !ALLOWED_ENTRIES.contains(&me.name.as_str()) {
            return Err(PackError::UnknownEntry(me.name.clone()));
        }
        if !names.insert(me.name.clone()) {
            return Err(PackError::DuplicateEntry(me.name.clone()));
        }
        result.push(PackEntry {
            name: me.name,
            bytes: e_bytes.to_vec(),
        });
        off += me.len;
    }
    if off != bytes.len() {
        return Err(PackError::Corrupt("trailing".into()));
    }
    Ok((result, m.namespace))
}

pub fn pack_hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// A structured container for namespace package contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespacePack {
    pub namespace: String,
    pub memories_jsonl: Vec<u8>,
    pub vectors_sqlite: Vec<u8>,
    pub code_graph_jsonl: Option<Vec<u8>>,
}

/// Export a namespace package into `.swalpack` format with deterministic content hashing.
pub fn export_namespace_pack(pack: &NamespacePack) -> Result<Vec<u8>, PackError> {
    let ns = pack.namespace.trim();
    if ns.is_empty() {
        return Err(PackError::InvalidNamespace(
            "namespace must not be empty".into(),
        ));
    }
    let mut entries = Vec::new();
    entries.push(PackEntry {
        name: "memories.jsonl".into(),
        bytes: pack.memories_jsonl.clone(),
    });
    entries.push(PackEntry {
        name: "vectors.sqlite".into(),
        bytes: pack.vectors_sqlite.clone(),
    });
    if let Some(ref cg) = pack.code_graph_jsonl {
        entries.push(PackEntry {
            name: "code_graph.jsonl".into(),
            bytes: cg.clone(),
        });
    }
    write_pack_with_namespace(&entries, Some(ns))
}

/// Re-import a namespace package from `.swalpack` bytes.
pub fn import_namespace_pack(bytes: &[u8]) -> Result<NamespacePack, PackError> {
    let (entries, namespace_opt) = read_pack_info(bytes)?;
    let namespace = namespace_opt.unwrap_or_else(|| "default".into());
    let mut memories_jsonl = None;
    let mut vectors_sqlite = None;
    let mut code_graph_jsonl = None;

    for e in entries {
        match e.name.as_str() {
            "memories.jsonl" => memories_jsonl = Some(e.bytes),
            "vectors.sqlite" => vectors_sqlite = Some(e.bytes),
            "code_graph.jsonl" => code_graph_jsonl = Some(e.bytes),
            _ => return Err(PackError::UnknownEntry(e.name)),
        }
    }

    let memories_jsonl =
        memories_jsonl.ok_or_else(|| PackError::MissingEntry("memories.jsonl".into()))?;
    let vectors_sqlite =
        vectors_sqlite.ok_or_else(|| PackError::MissingEntry("vectors.sqlite".into()))?;

    Ok(NamespacePack {
        namespace,
        memories_jsonl,
        vectors_sqlite,
        code_graph_jsonl,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rt() {
        let e = vec![PackEntry {
            name: "memories.jsonl".into(),
            bytes: b"1".to_vec(),
        }];
        assert_eq!(read_pack(&write_pack(&e).unwrap()).unwrap()[0].bytes, b"1");
    }

    #[test]
    fn h() {
        let e = vec![PackEntry {
            name: "memories.jsonl".into(),
            bytes: b"h".to_vec(),
        }];
        let p1 = write_pack(&e).unwrap();
        let p2 = write_pack(&read_pack(&p1).unwrap()).unwrap();
        assert_eq!(p1, p2);
        assert_eq!(pack_hash(&p1), pack_hash(&p2));
    }

    #[test]
    fn tamp() {
        let mut p = write_pack(&[PackEntry {
            name: "memories.jsonl".into(),
            bytes: b"1".to_vec(),
        }])
        .unwrap();
        let l = p.len() - 1;
        p[l] ^= 0xff;
        assert!(matches!(read_pack(&p), Err(PackError::Corrupt(_))));
    }

    #[test]
    fn unk() {
        assert!(matches!(
            write_pack(&[PackEntry {
                name: "x".into(),
                bytes: vec![]
            }]),
            Err(PackError::UnknownEntry(_))
        ));
    }

    #[test]
    fn dup() {
        let pe = || PackEntry {
            name: "memories.jsonl".into(),
            bytes: vec![],
        };
        assert!(matches!(
            write_pack(&[pe(), pe()]),
            Err(PackError::DuplicateEntry(_))
        ));
    }

    #[test]
    fn emp() {
        assert!(matches!(write_pack(&[]), Err(PackError::Empty)));
    }

    #[test]
    fn trl() {
        let mut p = write_pack(&[PackEntry {
            name: "memories.jsonl".into(),
            bytes: b"1".to_vec(),
        }])
        .unwrap();
        p.push(0);
        assert!(matches!(read_pack(&p), Err(PackError::Corrupt(_))));
    }

    #[test]
    fn test_namespace_export_reimport_identical_hash() {
        let original = NamespacePack {
            namespace: "swal/agent-1/inst-42".into(),
            memories_jsonl: b"{\"id\":\"m1\",\"content\":\"important knowledge\"}\n".to_vec(),
            vectors_sqlite: b"SQLite format 3\0test-vector-bytes".to_vec(),
            code_graph_jsonl: Some(b"{\"symbol\":\"main\"}\n".to_vec()),
        };

        // Export to swalpack bytes
        let bytes1 = export_namespace_pack(&original).expect("export succeeds");
        let hash1 = pack_hash(&bytes1);

        // Re-import from bytes
        let imported = import_namespace_pack(&bytes1).expect("import succeeds");
        assert_eq!(imported.namespace, original.namespace);
        assert_eq!(imported.memories_jsonl, original.memories_jsonl);
        assert_eq!(imported.vectors_sqlite, original.vectors_sqlite);
        assert_eq!(imported.code_graph_jsonl, original.code_graph_jsonl);

        // Re-export again
        let bytes2 = export_namespace_pack(&imported).expect("re-export succeeds");
        let hash2 = pack_hash(&bytes2);

        // Identical hash rule
        assert_eq!(bytes1, bytes2);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_namespace_export_empty_namespace_rejected() {
        let bad = NamespacePack {
            namespace: "   ".into(),
            memories_jsonl: b"{}".to_vec(),
            vectors_sqlite: b"{}".to_vec(),
            code_graph_jsonl: None,
        };
        assert!(matches!(
            export_namespace_pack(&bad),
            Err(PackError::InvalidNamespace(_))
        ));
    }

    #[test]
    fn test_import_missing_required_entry() {
        let e = vec![PackEntry {
            name: "memories.jsonl".into(),
            bytes: b"{}".to_vec(),
        }];
        let bytes = write_pack(&e).unwrap();
        assert!(matches!(
            import_namespace_pack(&bytes),
            Err(PackError::MissingEntry(_))
        ));
    }
}
