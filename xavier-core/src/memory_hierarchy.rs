use serde::{Deserialize, Serialize};
use crate::store::MemoryRecord;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum MemoryHierarchyNode {
    File(MemoryRecord),
    Directory {
        name: String,
        path: String,
        child_count: usize,
    },
}

pub struct MemoryTree;

impl MemoryTree {
    /// Build a list of hierarchical nodes representing the children of `parent_path`.
    pub fn build_ls(records: Vec<MemoryRecord>, parent_path: &str) -> Vec<MemoryHierarchyNode> {
        let parent_path = parent_path.trim_matches('/');
        let mut files = Vec::new();
        let mut dirs: HashMap<String, (String, usize)> = HashMap::new(); // name -> (full_path, count)

        for record in records {
            let record_path = record.path.trim_matches('/');

            // Skip if the record is the parent itself (ls shows children)
            if record_path == parent_path && !parent_path.is_empty() {
                continue;
            }

            if parent_path.is_empty() {
                // Root level
                if let Some(first_slash) = record_path.find('/') {
                    let dir_name = &record_path[..first_slash];
                    let entry = dirs.entry(dir_name.to_string()).or_insert((dir_name.to_string(), 0));
                    entry.1 += 1;
                } else if !record_path.is_empty() {
                    files.push(MemoryHierarchyNode::File(record));
                }
            } else if let Some(remainder) = record_path.strip_prefix(parent_path) {
                // Check if it's a direct child or in a sub-sub directory
                if let Some(remainder) = remainder.strip_prefix('/') {
                    if let Some(first_slash) = remainder.find('/') {
                        let dir_name = &remainder[..first_slash];
                        let full_dir_path = format!("{}/{}", parent_path, dir_name);
                        let entry = dirs.entry(dir_name.to_string()).or_insert((full_dir_path, 0));
                        entry.1 += 1;
                    } else if !remainder.is_empty() {
                        files.push(MemoryHierarchyNode::File(record));
                    }
                }
            }
        }

        let mut result = files;
        for (name, (path, child_count)) in dirs {
            result.push(MemoryHierarchyNode::Directory {
                name,
                path,
                child_count,
            });
        }

        // Sort for deterministic output: Directories first, then files, both alphabetically
        result.sort_by(|a, b| {
            match (a, b) {
                (MemoryHierarchyNode::Directory { name: na, .. }, MemoryHierarchyNode::Directory { name: nb, .. }) => na.cmp(nb),
                (MemoryHierarchyNode::Directory { .. }, MemoryHierarchyNode::File(_)) => std::cmp::Ordering::Less,
                (MemoryHierarchyNode::File(_), MemoryHierarchyNode::Directory { .. }) => std::cmp::Ordering::Greater,
                (MemoryHierarchyNode::File(ra), MemoryHierarchyNode::File(rb)) => ra.path.cmp(&rb.path),
            }
        });

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryRecord;
    use chrono::Utc;

    fn mock_record(path: &str) -> MemoryRecord {
        MemoryRecord {
            id: path.to_string(),
            workspace_id: "test".to_string(),
            path: path.to_string(),
            content: "test".to_string(),
            metadata: serde_json::json!({}),
            embedding: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            revision: 1,
            primary: true,
            parent_id: None,
            cluster_id: None,
            level: Default::default(),
            relation: None,
            clearance: Default::default(),
            revisions: vec![],
            content_iv: None,
            encrypted_dek: None,
            metadata_iv: None,
        }
    }

    #[test]
    fn test_build_ls_root() {
        let records = vec![
            mock_record("a.md"),
            mock_record("b/c.md"),
            mock_record("b/d/e.md"),
            mock_record("f/g.md"),
        ];

        let result = MemoryTree::build_ls(records, "");
        assert_eq!(result.len(), 3); // a.md (File), b (Dir), f (Dir)

        if let MemoryHierarchyNode::Directory { name, path, child_count } = &result[0] {
            assert_eq!(name, "b");
            assert_eq!(path, "b");
            assert_eq!(*child_count, 2);
        } else {
            panic!("Expected directory b");
        }

        if let MemoryHierarchyNode::Directory { name, path, child_count } = &result[1] {
            assert_eq!(name, "f");
            assert_eq!(path, "f");
            assert_eq!(*child_count, 1);
        } else {
            panic!("Expected directory f");
        }

        if let MemoryHierarchyNode::File(record) = &result[2] {
            assert_eq!(record.path, "a.md");
        } else {
            panic!("Expected file a.md");
        }
    }

    #[test]
    fn test_build_ls_subdir() {
        let records = vec![
            mock_record("a.md"),
            mock_record("b/c.md"),
            mock_record("b/d/e.md"),
            mock_record("b/f.md"),
        ];

        let result = MemoryTree::build_ls(records, "b");
        assert_eq!(result.len(), 3); // d (Dir), c.md (File), f.md (File)

        if let MemoryHierarchyNode::Directory { name, path, child_count } = &result[0] {
            assert_eq!(name, "d");
            assert_eq!(path, "b/d");
            assert_eq!(*child_count, 1);
        } else {
            panic!("Expected directory d");
        }

        if let MemoryHierarchyNode::File(record) = &result[1] {
            assert_eq!(record.path, "b/c.md");
        } else {
            panic!("Expected file b/c.md");
        }

        if let MemoryHierarchyNode::File(record) = &result[2] {
            assert_eq!(record.path, "b/f.md");
        } else {
            panic!("Expected file b/f.md");
        }
    }
}
