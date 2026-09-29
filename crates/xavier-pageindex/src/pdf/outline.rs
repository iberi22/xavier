//! PDF bookmarks to tree (pure Rust, `lopdf`).
//!
//! Reads the `/Outlines` chain, resolves direct, named and action
//! destinations, and extracts per-page text. Malformed input yields an
//! error, never a panic.

use std::collections::{BTreeMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::error::PageIndexError;
use crate::model::{DocumentTree, TreeNode};

/// Bound on outline depth and name-tree recursion (cycle/abuse guard).
const MAX_DEPTH: usize = 64;
/// Bound on decompressed content per page (decompression-bomb guard).
const MAX_PAGE_BYTES: usize = 64 * 1024 * 1024;

/// One bookmark: title, 1-based destination page and nested children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineNode {
    pub title: String,
    pub page: u32,
    pub children: Vec<OutlineNode>,
}

fn build_err(e: impl std::fmt::Display) -> PageIndexError {
    PageIndexError::Build(format!("pdf: {e}"))
}

fn load(bytes: &[u8]) -> Result<Document, PageIndexError> {
    let loaded = catch_unwind(AssertUnwindSafe(|| Document::load_mem(bytes)))
        .map_err(|_| build_err("parser panicked on malformed input"))?;
    // lopdf tries the empty user password on load and decrypts in place, so a
    // document that is still encrypted afterwards needs a real password.
    match loaded {
        Ok(doc) if doc.is_encrypted() => Err(PageIndexError::Encrypted),
        Ok(doc) => Ok(doc),
        Err(lopdf::Error::Decryption(_) | lopdf::Error::InvalidPassword) => {
            Err(PageIndexError::Encrypted)
        }
        Err(e) if is_password_required(&e) => Err(PageIndexError::Encrypted),
        Err(e) => Err(build_err(e)),
    }
}

/// lopdf reports "encrypted, password needed" as `Unimplemented` with a hint.
fn is_password_required(e: &lopdf::Error) -> bool {
    matches!(e, lopdf::Error::Unimplemented(m) if m.contains("requires a password"))
}

/// Extract the bookmark tree. `Ok(None)` when the PDF has no usable outline.
pub fn extract_outline(bytes: &[u8]) -> Result<Option<Vec<OutlineNode>>, PageIndexError> {
    let doc = load(bytes)?;
    catch_unwind(AssertUnwindSafe(|| read_outline(&doc)))
        .map_err(|_| build_err("outline reader panicked on malformed input"))
}

/// Extract text per page, in page order (index 0 is page 1).
pub fn extract_pages(bytes: &[u8]) -> Result<Vec<String>, PageIndexError> {
    let doc = load(bytes)?;
    catch_unwind(AssertUnwindSafe(|| {
        doc.get_pages()
            .keys()
            .map(|&n| {
                doc.extract_text_with_limit(&[n], MAX_PAGE_BYTES)
                    .unwrap_or_default()
            })
            .collect()
    }))
    .map_err(|_| build_err("text extraction panicked on malformed input"))
}

/// Convert bookmarks into a validated-shape tree over `page_count` pages.
///
/// Entries outside `1..=page_count` or before the previous sibling's start
/// are dropped. A bookmark only gives the page where a section starts, not
/// the position on it, so the previous section's tail may sit on that same
/// page: each section therefore ends on the next sibling's start page (they
/// always share the boundary page). One extra page is cheap to retrieve; a
/// lost tail is a retrieval miss.
pub fn outline_to_tree(doc_id: &str, nodes: &[OutlineNode], page_count: u32) -> DocumentTree {
    fn build(nodes: &[OutlineNode], lo: u32, hi: u32) -> Vec<TreeNode> {
        let mut kept: Vec<&OutlineNode> = Vec::new();
        let mut last = lo;
        for n in nodes {
            if n.page >= lo && n.page <= hi && n.page >= last {
                last = n.page;
                kept.push(n);
            }
        }
        (0..kept.len())
            .map(|i| {
                let n = kept[i];
                let end = kept.get(i + 1).map_or(hi, |next| next.page);
                TreeNode {
                    node_id: String::new(),
                    title: n.title.clone(),
                    start_page: n.page,
                    end_page: end,
                    summary: None,
                    token_estimate: 0,
                    children: build(&n.children, n.page, end),
                }
            })
            .collect()
    }
    let mut tree = DocumentTree {
        doc_id: doc_id.to_string(),
        roots: build(nodes, 1, page_count),
    };
    tree.assign_node_ids();
    tree
}

struct Ctx<'a> {
    doc: &'a Document,
    /// Page object id to 1-based page number.
    pages: BTreeMap<ObjectId, u32>,
}

fn read_outline(doc: &Document) -> Option<Vec<OutlineNode>> {
    let ctx = Ctx {
        doc,
        pages: doc.get_pages().into_iter().map(|(n, id)| (id, n)).collect(),
    };
    let catalog = doc.catalog().ok()?;
    let outlines = ctx.dict(catalog.get(b"Outlines").ok()?)?;
    let first = outlines.get(b"First").ok()?;
    let mut seen = HashSet::new();
    let nodes = ctx.siblings(first, catalog, 0, &mut seen);
    (!nodes.is_empty()).then_some(nodes)
}

impl<'a> Ctx<'a> {
    fn resolve(&self, o: &'a Object) -> Option<&'a Object> {
        self.doc.dereference(o).ok().map(|(_, o)| o)
    }

    fn dict(&self, o: &'a Object) -> Option<&'a Dictionary> {
        match self.resolve(o)? {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    /// Walk a `/First`..`/Next` chain; unresolved entries are skipped and
    /// their children promoted.
    fn siblings(
        &self,
        first: &'a Object,
        catalog: &'a Dictionary,
        depth: usize,
        seen: &mut HashSet<ObjectId>,
    ) -> Vec<OutlineNode> {
        let mut out = Vec::new();
        if depth >= MAX_DEPTH {
            return out;
        }
        let mut cur = Some(first);
        while let Some(obj) = cur {
            if let Ok(id) = obj.as_reference() {
                if !seen.insert(id) {
                    break;
                }
            }
            let Some(item) = self.dict(obj) else { break };
            let children = item
                .get(b"First")
                .ok()
                .map(|f| self.siblings(f, catalog, depth + 1, seen))
                .unwrap_or_default();
            let title = item
                .get(b"Title")
                .ok()
                .and_then(|t| self.resolve(t))
                .and_then(|t| lopdf::decode_text_string(t).ok())
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());
            match (title, self.item_page(item, catalog)) {
                (Some(title), Some(page)) => out.push(OutlineNode {
                    title,
                    page,
                    children,
                }),
                _ => out.extend(children),
            }
            cur = item.get(b"Next").ok();
        }
        out
    }

    fn item_page(&self, item: &'a Dictionary, catalog: &'a Dictionary) -> Option<u32> {
        if let Ok(d) = item.get(b"Dest") {
            return self.dest_page(d, catalog, 0);
        }
        let action = self.dict(item.get(b"A").ok()?)?;
        let is_goto = action
            .get(b"S")
            .ok()
            .and_then(|s| self.resolve(s))
            .and_then(|s| s.as_name().ok())
            .is_some_and(|n| n == b"GoTo");
        if !is_goto {
            return None;
        }
        self.dest_page(action.get(b"D").ok()?, catalog, 0)
    }

    fn dest_page(&self, d: &'a Object, catalog: &'a Dictionary, depth: usize) -> Option<u32> {
        if depth > 4 {
            return None;
        }
        match self.resolve(d)? {
            Object::Array(a) => self.array_page(a),
            Object::Dictionary(dd) => self.dest_page(dd.get(b"D").ok()?, catalog, depth + 1),
            Object::Name(n) => self.named(n, catalog, depth),
            Object::String(s, _) => self.named(s, catalog, depth),
            _ => None,
        }
    }

    fn array_page(&self, a: &[Object]) -> Option<u32> {
        match a.first()? {
            Object::Reference(id) => self.pages.get(id).copied(),
            // Remote-style zero-based page index.
            Object::Integer(i) => u32::try_from(*i).ok()?.checked_add(1),
            _ => None,
        }
    }

    fn named(&self, name: &[u8], catalog: &'a Dictionary, depth: usize) -> Option<u32> {
        // Legacy PDF 1.1 `/Dests` dictionary on the catalog.
        if let Some(dests) = catalog.get(b"Dests").ok().and_then(|o| self.dict(o)) {
            if let Ok(v) = dests.get(name) {
                return self.dest_page(v, catalog, depth + 1);
            }
        }
        // `/Names` -> `/Dests` name tree.
        let root = catalog
            .get(b"Names")
            .ok()
            .and_then(|n| self.dict(n))?
            .get(b"Dests")
            .ok()?;
        let v = self.name_tree(root, name, 0)?;
        self.dest_page(v, catalog, depth + 1)
    }

    fn name_tree(&self, node: &'a Object, key: &[u8], depth: usize) -> Option<&'a Object> {
        if depth >= MAX_DEPTH {
            return None;
        }
        let d = self.dict(node)?;
        if let Some(Object::Array(names)) = d.get(b"Names").ok().and_then(|n| self.resolve(n)) {
            for pair in names.chunks(2) {
                if let [k, v] = pair {
                    if let Some(Object::String(s, _)) = self.resolve(k) {
                        if s == key {
                            return Some(v);
                        }
                    }
                }
            }
        }
        if let Some(Object::Array(kids)) = d.get(b"Kids").ok().and_then(|n| self.resolve(n)) {
            for kid in kids {
                if let Some(v) = self.name_tree(kid, key, depth + 1) {
                    return Some(v);
                }
            }
        }
        None
    }
}
