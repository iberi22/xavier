#![cfg(feature = "markdown")]

use xavier_pageindex::builders::markdown::build;
use xavier_pageindex::model::{DocumentTree, TreeNode};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/md/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

fn titles(nodes: &[TreeNode]) -> Vec<&str> {
    nodes.iter().map(|n| n.title.as_str()).collect()
}

fn all_titles<'a>(nodes: &'a [TreeNode], out: &mut Vec<&'a str>) {
    for n in nodes {
        out.push(&n.title);
        all_titles(&n.children, out);
    }
}

fn assert_covers(tree: &DocumentTree, page_count: u32) {
    tree.validate(page_count).unwrap();
    assert_eq!(tree.roots.first().unwrap().start_page, 1);
    assert_eq!(tree.roots.last().unwrap().end_page, page_count);
    for w in tree.roots.windows(2) {
        assert_eq!(w[0].end_page + 1, w[1].start_page);
    }
}

#[test]
fn test_md_headings_build_nested_tree() {
    let (tree, pages) = build(&fixture("nested.md"), 1);
    assert_eq!(pages.len(), 10);
    assert_eq!(titles(&tree.roots), ["Alpha", "Beta"]);
    assert_eq!(titles(&tree.roots[0].children), ["Alpha One", "Alpha Two"]);
    // Setext heading is a level-2 child of Beta.
    assert_eq!(titles(&tree.roots[1].children), ["Setext Gamma"]);
    assert_eq!(tree.roots[0].node_id, "0000");
    tree.validate(pages.len() as u32).unwrap();
}

#[test]
fn test_md_ignores_hashes_inside_code_fences() {
    let (tree, pages) = build(&fixture("fenced.md"), 1);
    let mut all = Vec::new();
    all_titles(&tree.roots, &mut all);
    assert_eq!(all, ["Real", "Also Real"]);
    tree.validate(pages.len() as u32).unwrap();
}

#[test]
fn test_md_skipped_heading_levels_attach_to_nearest_ancestor() {
    let (tree, pages) = build(&fixture("skipped.md"), 1);
    assert_eq!(titles(&tree.roots), ["Top"]);
    let top = &tree.roots[0];
    // "Deep" (h3) skips h2 and attaches to Top; "Mid" (h2) is a sibling of it.
    assert_eq!(titles(&top.children), ["Deep", "Mid"]);
    assert_eq!(titles(&top.children[1].children), ["Deeper"]);
    tree.validate(pages.len() as u32).unwrap();
}

#[test]
fn test_md_node_ranges_cover_all_pages() {
    for per_page in [1, 2, 3, 100] {
        for name in ["nested.md", "fenced.md", "skipped.md", "preamble.md"] {
            let (tree, pages) = build(&fixture(name), per_page);
            assert_covers(&tree, pages.len() as u32);
        }
    }
}

#[test]
fn test_md_preamble_becomes_root_node() {
    let (tree, pages) = build(&fixture("preamble.md"), 2);
    assert_eq!(titles(&tree.roots), ["Preamble", "First", "Second"]);
    assert_eq!(tree.roots[0].start_page, 1);
    assert_covers(&tree, pages.len() as u32);
}
