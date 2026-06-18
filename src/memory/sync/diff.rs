//! Diff two memory store manifests → list of operations to reconcile.
//!
//! The diff algorithm compares local and remote manifests entry-by-entry
//! and produces two lists:
//!
//! - `to_push`: entries the remote is missing or has stale versions of.
//! - `to_pull`: entries we are missing or have stale versions of.

use anyhow::Result;

use super::{ChunkDiff, DiffAction, Manifest, ManifestEntry};

/// Compare two manifests and determine what to push and pull.
///
/// Returns `(to_push, to_pull)` where each is a `Vec<ManifestEntry>` describing
/// chunks that are missing or outdated on the respective side.
pub fn diff_manifests(
    local: &Manifest,
    remote: &Manifest,
) -> Result<(Vec<ManifestEntry>, Vec<ManifestEntry>)> {
    let mut local_by_hash: std::collections::HashMap<&str, &ManifestEntry> =
        std::collections::HashMap::new();
    for entry in local {
        local_by_hash.insert(&entry.chunk_hash, entry);
    }

    let mut remote_by_hash: std::collections::HashMap<&str, &ManifestEntry> =
        std::collections::HashMap::new();
    for entry in remote {
        remote_by_hash.insert(&entry.chunk_hash, entry);
    }

    let mut to_push = Vec::new();
    let mut to_pull = Vec::new();

    // Entries in local but not remote → push
    for entry in local {
        match remote_by_hash.get(entry.chunk_hash.as_str()) {
            None => {
                to_push.push(entry.clone());
            }
            Some(remote_entry) => {
                // Same hash, but remote is older → push
                if remote_entry.revision < entry.revision {
                    to_push.push(entry.clone());
                }
            }
        }
    }

    // Entries in remote but not local → pull
    for entry in remote {
        match local_by_hash.get(entry.chunk_hash.as_str()) {
            None => {
                to_pull.push(entry.clone());
            }
            Some(local_entry) => {
                // Same hash, but local is older → pull
                if local_entry.revision < entry.revision {
                    to_pull.push(entry.clone());
                }
            }
        }
    }

    Ok((to_push, to_pull))
}

/// Convert a list of manifest entries into a list of `ChunkDiff::Delete`
/// actions (for removing chunks that exist on one side but not the other).
pub fn deletions_from_diff(
    local: &Manifest,
    remote: &Manifest,
) -> Result<Vec<ChunkDiff>> {
    let remote_hashes: std::collections::HashSet<&str> =
        remote.iter().map(|e| e.chunk_hash.as_str()).collect();

    let mut deletes = Vec::new();
    for entry in local {
        if !remote_hashes.contains(entry.chunk_hash.as_str()) {
            deletes.push(ChunkDiff {
                chunk_hash: entry.chunk_hash.clone(),
                namespace: entry.namespace.clone(),
                action: DiffAction::Delete,
                data: None,
                timestamp: std::time::SystemTime::now(),
            });
        }
    }
    Ok(deletes)
}

/// Build `ChunkDiff::Add` actions from manifest entries (local entry, need content).
pub fn as_push_diffs(entries: &[ManifestEntry]) -> Vec<ManifestEntry> {
    entries.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_entry(hash: &str, rev: u64, namespace: &str) -> ManifestEntry {
        ManifestEntry {
            chunk_hash: hash.to_string(),
            namespace: namespace.to_string(),
            revision: rev,
            updated_at: Utc::now(),
            size_bytes: 100,
        }
    }

    #[test]
    fn test_empty_manifests() {
        let local: Manifest = vec![];
        let remote: Manifest = vec![];
        let (push, pull) = diff_manifests(&local, &remote).unwrap();
        assert!(push.is_empty());
        assert!(pull.is_empty());
    }

    #[test]
    fn test_local_has_newer_entry() {
        let local = vec![make_entry("abc", 5, "episodic")];
        let remote = vec![make_entry("abc", 3, "episodic")];
        let (push, pull) = diff_manifests(&local, &remote).unwrap();
        assert_eq!(push.len(), 1, "should push newer revision");
        assert!(pull.is_empty(), "nothing to pull");
    }

    #[test]
    fn test_remote_has_newer_entry() {
        let local = vec![make_entry("abc", 2, "semantic")];
        let remote = vec![make_entry("abc", 7, "semantic")];
        let (push, pull) = diff_manifests(&local, &remote).unwrap();
        assert!(push.is_empty(), "nothing to push");
        assert_eq!(pull.len(), 1, "should pull newer revision");
    }

    #[test]
    fn test_local_only_entry() {
        let local = vec![make_entry("local-only", 1, "working")];
        let remote: Manifest = vec![];
        let (push, pull) = diff_manifests(&local, &remote).unwrap();
        assert_eq!(push.len(), 1);
        assert!(pull.is_empty());
    }

    #[test]
    fn test_remote_only_entry() {
        let local: Manifest = vec![];
        let remote = vec![make_entry("remote-only", 1, "episodic")];
        let (push, pull) = diff_manifests(&local, &remote).unwrap();
        assert!(push.is_empty());
        assert_eq!(pull.len(), 1);
    }

    #[test]
    fn test_same_hash_same_revision_no_op() {
        let local = vec![make_entry("xyz", 3, "episodic")];
        let remote = vec![make_entry("xyz", 3, "episodic")];
        let (push, pull) = diff_manifests(&local, &remote).unwrap();
        assert!(push.is_empty(), "same revision → no push");
        assert!(pull.is_empty(), "same revision → no pull");
    }
}
