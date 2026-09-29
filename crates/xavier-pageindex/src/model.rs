//! Document and tree data model.

use serde::{Deserialize, Serialize};

use crate::error::PageIndexError;

/// Address unit: real PDF pages or fixed line windows for text sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageUnit {
    Page,
    VirtualPage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    Markdown,
    PlainText,
    Legal,
    Pdf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocStatus {
    Processing,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub doc_id: String,
    pub workspace: String,
    pub name: String,
    pub source_kind: SourceKind,
    /// sha256 hex of the raw bytes.
    pub content_hash: String,
    pub page_count: u32,
    pub page_unit: PageUnit,
    pub status: DocStatus,
    pub builder: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeNode {
    /// Zero-padded preorder id: "0000", "0001", ...
    pub node_id: String,
    pub title: String,
    /// 1-based inclusive.
    pub start_page: u32,
    /// 1-based inclusive.
    pub end_page: u32,
    pub summary: Option<String>,
    pub token_estimate: u32,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentTree {
    pub doc_id: String,
    pub roots: Vec<TreeNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub doc_id: String,
    pub page_no: u32,
    pub text: String,
}

/// Format the preorder counter as a node id.
pub fn format_node_id(n: usize) -> String {
    format!("{n:04}")
}

impl DocumentTree {
    /// Overwrite every `node_id` with its zero-padded preorder index.
    pub fn assign_node_ids(&mut self) {
        fn walk(nodes: &mut [TreeNode], counter: &mut usize) {
            for n in nodes {
                n.node_id = format_node_id(*counter);
                *counter += 1;
                walk(&mut n.children, counter);
            }
        }
        let mut counter = 0;
        walk(&mut self.roots, &mut counter);
    }

    /// Check ordering, containment, page bounds and preorder ids.
    pub fn validate(&self, page_count: u32) -> Result<(), PageIndexError> {
        fn walk(
            nodes: &[TreeNode],
            bounds: (u32, u32),
            counter: &mut usize,
        ) -> Result<(), PageIndexError> {
            let mut prev_end = bounds.0.saturating_sub(1);
            for n in nodes {
                let bad =
                    |m: &str| PageIndexError::InvalidRange(format!("node {}: {m}", n.node_id));
                if n.node_id != format_node_id(*counter) {
                    return Err(bad("node_id is not the expected preorder id"));
                }
                *counter += 1;
                if n.start_page < 1 || n.start_page > n.end_page {
                    return Err(bad("start_page/end_page out of order"));
                }
                if n.start_page < bounds.0 || n.end_page > bounds.1 {
                    return Err(bad("range outside parent range or page_count"));
                }
                if n.start_page <= prev_end {
                    return Err(bad("overlaps or precedes previous sibling"));
                }
                prev_end = n.end_page;
                walk(&n.children, (n.start_page, n.end_page), counter)?;
            }
            Ok(())
        }
        walk(&self.roots, (1, page_count), &mut 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(title: &str, s: u32, e: u32, children: Vec<TreeNode>) -> TreeNode {
        TreeNode {
            node_id: String::new(),
            title: title.into(),
            start_page: s,
            end_page: e,
            summary: None,
            token_estimate: 0,
            children,
        }
    }

    fn tree(roots: Vec<TreeNode>) -> DocumentTree {
        let mut t = DocumentTree {
            doc_id: "d".into(),
            roots,
        };
        t.assign_node_ids();
        t
    }

    #[test]
    fn test_tree_node_serde_roundtrip_stable_json() {
        let t = tree(vec![node("A", 1, 2, vec![node("A1", 1, 1, vec![])])]);
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(
            json,
            r#"{"doc_id":"d","roots":[{"node_id":"0000","title":"A","start_page":1,"end_page":2,"summary":null,"token_estimate":0,"children":[{"node_id":"0001","title":"A1","start_page":1,"end_page":1,"summary":null,"token_estimate":0,"children":[]}]}]}"#
        );
        let back: DocumentTree = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn test_node_ids_are_zero_padded_preorder() {
        let t = tree(vec![
            node(
                "A",
                1,
                3,
                vec![node("A1", 1, 1, vec![]), node("A2", 2, 3, vec![])],
            ),
            node("B", 4, 5, vec![]),
        ]);
        let ids = [
            &t.roots[0].node_id,
            &t.roots[0].children[0].node_id,
            &t.roots[0].children[1].node_id,
            &t.roots[1].node_id,
        ];
        assert_eq!(ids, ["0000", "0001", "0002", "0003"]);
        assert!(t.validate(5).is_ok());
    }

    #[test]
    fn test_tree_validate_rejects_overlapping_siblings() {
        let t = tree(vec![node("A", 1, 3, vec![]), node("B", 3, 5, vec![])]);
        assert!(matches!(
            t.validate(5),
            Err(PageIndexError::InvalidRange(_))
        ));
    }

    #[test]
    fn test_tree_validate_rejects_child_outside_parent_range() {
        let t = tree(vec![node("A", 2, 3, vec![node("A1", 1, 3, vec![])])]);
        assert!(matches!(
            t.validate(5),
            Err(PageIndexError::InvalidRange(_))
        ));
        let t = tree(vec![node("A", 1, 9, vec![])]);
        assert!(t.validate(5).is_err());
    }
}
