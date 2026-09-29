//! Integration tests for the SQLite tree store.
#![cfg(feature = "sqlite")]

use xavier_pageindex::model::{
    DocStatus, Document, DocumentTree, Page, PageUnit, SourceKind, TreeNode,
};
use xavier_pageindex::store::{SqliteStore, Store};

fn node(title: &str, s: u32, e: u32, children: Vec<TreeNode>) -> TreeNode {
    TreeNode {
        node_id: String::new(),
        title: title.into(),
        start_page: s,
        end_page: e,
        summary: Some(format!("about {title}")),
        token_estimate: 7,
        children,
    }
}

fn fixture(ws: &str, name: &str, id: &str, hash: &str) -> (Document, DocumentTree, Vec<Page>) {
    let doc = Document {
        doc_id: id.into(),
        workspace: ws.into(),
        name: name.into(),
        source_kind: SourceKind::Markdown,
        content_hash: hash.into(),
        page_count: 5,
        page_unit: PageUnit::VirtualPage,
        status: DocStatus::Completed,
        builder: "test".into(),
        created_at: 1,
    };
    let mut tree = DocumentTree {
        doc_id: id.into(),
        roots: vec![
            node(
                "Intro",
                1,
                3,
                vec![node("Scope", 1, 1, vec![]), node("Goals", 2, 3, vec![])],
            ),
            node("Body", 4, 5, vec![]),
        ],
    };
    tree.assign_node_ids();
    let pages = (1..=5)
        .map(|n| Page {
            doc_id: id.into(),
            page_no: n,
            text: format!("page {n}"),
        })
        .collect();
    (doc, tree, pages)
}

/// Minimal temp dir helper (no extra dev-dependency).
mod tmp {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    pub struct Dir(PathBuf);

    impl Dir {
        pub fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "xavier-pi-store-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn temp_store() -> (tmp::Dir, SqliteStore) {
    let dir = tmp::Dir::new();
    let store = SqliteStore::open(dir.path().join("pi.sqlite3")).unwrap();
    (dir, store)
}

#[test]
fn test_store_roundtrip_document_tree() {
    let (_d, store) = temp_store();
    let (doc, tree, pages) = fixture("ws", "a.md", "d1", "h1");
    let out = store.put_document(&doc, &tree, &pages).unwrap();
    assert!(!out.reused);
    assert_eq!(store.get_document("ws", "d1").unwrap().unwrap(), doc);
    assert_eq!(store.find_by_name("ws", "a.md").unwrap().unwrap(), doc);
    assert_eq!(store.get_tree("ws", "d1").unwrap().unwrap(), tree);
    assert_eq!(store.list_documents("ws").unwrap(), vec![doc.clone()]);
    assert_eq!(
        store.search_by_title("ws", "goal").unwrap(),
        vec![doc.clone()]
    );
    assert_eq!(store.search_by_title("ws", "a.MD").unwrap(), vec![doc]);
    assert!(store.search_by_title("ws", "nothing").unwrap().is_empty());
}

#[test]
fn test_store_get_pages_by_range() {
    let (_d, store) = temp_store();
    let (doc, tree, pages) = fixture("ws", "a.md", "d1", "h1");
    store.put_document(&doc, &tree, &pages).unwrap();
    let got = store.get_pages("ws", "d1", 2, 4).unwrap();
    let nos: Vec<u32> = got.iter().map(|p| p.page_no).collect();
    assert_eq!(nos, [2, 3, 4]);
    assert_eq!(got[0].text, "page 2");
    assert!(store.get_pages("ws", "d1", 4, 2).is_err());
    assert!(store.get_pages("ws", "d1", 9, 12).unwrap().is_empty());
}

#[test]
fn test_store_reingest_same_hash_is_idempotent() {
    let (_d, store) = temp_store();
    let (doc, tree, pages) = fixture("ws", "a.md", "d1", "h1");
    store.put_document(&doc, &tree, &pages).unwrap();
    let (doc2, tree2, pages2) = fixture("ws", "a.md", "d2", "h1");
    let out = store.put_document(&doc2, &tree2, &pages2).unwrap();
    assert!(out.reused);
    assert_eq!(out.doc_id, "d1");
    assert_eq!(store.list_documents("ws").unwrap().len(), 1);
    // A changed hash replaces the old document.
    let (doc3, tree3, pages3) = fixture("ws", "a.md", "d3", "h2");
    let out = store.put_document(&doc3, &tree3, &pages3).unwrap();
    assert!(!out.reused);
    assert!(store.get_document("ws", "d1").unwrap().is_none());
    assert!(store.get_document("ws", "d3").unwrap().is_some());
}

#[test]
fn test_store_delete_cascades_nodes_and_pages() {
    let (d, store) = temp_store();
    let (doc, tree, pages) = fixture("ws", "a.md", "d1", "h1");
    store.put_document(&doc, &tree, &pages).unwrap();
    assert!(store.delete_document("ws", "d1").unwrap());
    assert!(!store.delete_document("ws", "d1").unwrap());
    let raw = rusqlite::Connection::open(d.path().join("pi.sqlite3")).unwrap();
    for t in ["pi_documents", "pi_nodes", "pi_pages"] {
        let n: i64 = raw
            .query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "{t} not empty");
    }
}

#[test]
fn test_store_wal_and_foreign_keys_enabled() {
    let (_d, store) = temp_store();
    assert_eq!(store.pragma("journal_mode").unwrap(), "wal");
    assert_eq!(store.pragma("foreign_keys").unwrap(), "1");
    assert_eq!(store.schema_version().unwrap(), 1);
}

#[test]
fn test_store_workspace_isolation() {
    let (_d, store) = temp_store();
    let (doc, tree, pages) = fixture("ws1", "a.md", "d1", "h1");
    store.put_document(&doc, &tree, &pages).unwrap();
    // Same name in another workspace is allowed and independent.
    let (doc2, tree2, pages2) = fixture("ws2", "a.md", "d2", "h1");
    assert!(!store.put_document(&doc2, &tree2, &pages2).unwrap().reused);
    assert!(store.get_document("ws2", "d1").unwrap().is_none());
    assert!(store.get_tree("ws2", "d1").unwrap().is_none());
    assert!(store.get_pages("ws2", "d1", 1, 5).unwrap().is_empty());
    assert!(!store.delete_document("ws2", "d1").unwrap());
    assert_eq!(store.list_documents("ws1").unwrap().len(), 1);
    assert!(store.get_document("ws1", "d1").unwrap().is_some());
}

#[test]
fn test_store_roundtrip_more_than_10000_nodes_keeps_preorder() {
    let (_d, store) = temp_store();
    let (doc, _, pages) = fixture("ws", "big.md", "dbig", "hb");
    let roots: Vec<TreeNode> = (0..10_050)
        .map(|i| node(&format!("n{i}"), 1, 1, vec![node("leaf", 1, 1, vec![])]))
        .collect();
    let mut tree = DocumentTree {
        doc_id: "dbig".into(),
        roots,
    };
    tree.assign_node_ids();
    store.put_document(&doc, &tree, &pages).unwrap();
    assert_eq!(store.get_tree("ws", "dbig").unwrap().unwrap(), tree);
}
