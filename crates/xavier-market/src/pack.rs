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
}

#[rustfmt::skip]
#[derive(Serialize, Deserialize)]
struct ManifestEntry { name: String, sha256: String, len: usize }

#[rustfmt::skip]
#[derive(Serialize, Deserialize)]
struct Manifest { version: u32, entries: Vec<ManifestEntry> }

pub fn write_pack(entries: &[PackEntry]) -> Result<Vec<u8>, PackError> {
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
    Ok(result)
}

pub fn pack_hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
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
}
