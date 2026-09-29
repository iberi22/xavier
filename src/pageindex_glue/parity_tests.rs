//! The pageindex legal builder must agree with the RAG legal chunker on the
//! shared fixtures (heading titles and breadcrumb nesting).

use std::collections::BTreeSet;

use xavier_pageindex::builders::legal;
use xavier_pageindex::TreeNode;

use crate::documents::legal_chunker::LegalHierarchicalChunker;

const SEP: &str = " > ";

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/crates/xavier-pageindex/tests/fixtures/legal/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

fn tree_paths(nodes: &[TreeNode], prefix: &[String], out: &mut BTreeSet<String>) {
    for n in nodes {
        let mut path = prefix.to_vec();
        path.push(n.title.clone());
        out.insert(path.join(SEP));
        tree_paths(&n.children, &path, out);
    }
}

/// Every breadcrumb prefix of every chunk path (a chapter without body text
/// has no chunk of its own but still appears as a prefix).
fn chunker_paths(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for c in LegalHierarchicalChunker::default().chunk_document("doc", text) {
        let parts: Vec<&str> = c.full_path.split(SEP).collect();
        for i in 1..=parts.len() {
            out.insert(parts[..i].join(SEP));
        }
    }
    out
}

fn assert_parity(file: &str) {
    let text = fixture(file);
    let built = legal::build("doc", &text, 60).expect("legal build");
    let mut from_tree = BTreeSet::new();
    tree_paths(&built.tree.roots, &[], &mut from_tree);
    let from_chunker = chunker_paths(&text);
    assert!(!from_tree.is_empty() && !from_chunker.is_empty());
    assert_eq!(
        from_tree,
        from_chunker,
        "{file}: only in tree {:?}; only in chunker {:?}",
        from_tree.difference(&from_chunker).collect::<Vec<_>>(),
        from_chunker.difference(&from_tree).collect::<Vec<_>>(),
    );
}

#[test]
fn test_legal_parity_with_legal_chunker_fixture() {
    assert_parity("contract_es.txt");
}

#[test]
fn test_pageindex_legal_parity_with_legal_chunker() {
    assert_parity("contract_es.txt");
    assert_parity("contract_en.txt");
}
