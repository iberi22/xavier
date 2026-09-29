#![cfg(all(feature = "sqlite", feature = "markdown"))]

use xavier_pageindex::store::SqliteStore;
use xavier_pageindex::{
    BrowseQuery, IngestOptions, PageIndex, PageIndexError, Source, StructureOpts,
};

const WS: &str = "ws";

fn index() -> PageIndex<SqliteStore> {
    PageIndex::new(SqliteStore::open_in_memory().unwrap())
}

/// Markdown with `sections` sections of `body_lines` lines each.
fn big_md(sections: usize, body_lines: usize) -> String {
    let mut s = String::new();
    for i in 1..=sections {
        s.push_str(&format!("# Section {i}\n\n"));
        for l in 0..body_lines {
            s.push_str(&format!("secret body text {i}-{l}\n"));
        }
        s.push_str(&format!("## Sub {i}\n\nsub body {i}\n\n"));
    }
    s
}

fn opts(lpp: u32) -> IngestOptions {
    IngestOptions {
        lines_per_page: lpp,
        ..Default::default()
    }
}

#[test]
fn test_browse_lists_docs_with_status_and_page_count() {
    let pi = index();
    pi.ingest(WS, "a.md", Source::Markdown("# A\n\ntext\n"), opts(10))
        .unwrap();
    pi.ingest(WS, "b.txt", Source::PlainText("hello\nworld\n"), opts(10))
        .unwrap();
    let page = pi.browse(WS, BrowseQuery::default()).unwrap();
    assert_eq!(page.total, 2);
    assert!(!page.has_more);
    assert!(page.documents.iter().all(|d| d.status == "completed"));
    assert!(page.documents.iter().all(|d| d.page_count >= 1));
    // Paging and filter.
    let p1 = pi
        .browse(
            WS,
            BrowseQuery {
                limit: 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(p1.has_more);
    assert_eq!(p1.next_offset, Some(1));
    let hit = pi
        .browse(
            WS,
            BrowseQuery {
                query: Some("b.txt".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(hit.documents.len(), 1);
    let info = pi.get_document(WS, "a.md").unwrap();
    assert_eq!(info.status, "completed");
    assert!(!info.structure_first);
    // Idempotent re-ingest.
    let again = pi
        .ingest(WS, "a.md", Source::Markdown("# A\n\ntext\n"), opts(10))
        .unwrap();
    assert_eq!(again.doc_id, info.doc_id);
    let bad_pdf = pi.ingest(WS, "x.pdf", Source::Pdf(b"%PDF"), opts(10));
    #[cfg(not(feature = "pdf-outline"))]
    assert!(matches!(
        bad_pdf,
        Err(PageIndexError::FeatureDisabled("pdf"))
    ));
    #[cfg(feature = "pdf-outline")]
    assert!(matches!(bad_pdf, Err(PageIndexError::Build(_))));
}

#[test]
fn test_get_document_structure_strips_text() {
    let pi = index();
    pi.ingest(WS, "d.md", Source::Markdown(&big_md(3, 5)), opts(5))
        .unwrap();
    let full = pi
        .get_structure(WS, "d.md", StructureOpts::default())
        .unwrap();
    let json = serde_json::to_string(&full).unwrap();
    assert!(!json.contains("secret body"));
    assert!(!json.contains("\"text\""));
    assert!(!full.truncated);
    assert_eq!(full.nodes.len(), 3);
    assert_eq!(full.nodes[0].children.len(), 1);

    let shallow = pi
        .get_structure(
            WS,
            "d.md",
            StructureOpts {
                max_depth: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(shallow.nodes.iter().all(|n| n.children.is_empty()));
    assert_eq!(shallow.nodes[0].hidden_children, 1);

    let sub = pi
        .get_structure(
            WS,
            "d.md",
            StructureOpts {
                node_id: Some(full.nodes[1].node_id.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(sub.nodes.len(), 1);
    assert_eq!(sub.nodes[0].title, "Section 2");
    assert!(pi
        .get_structure(
            WS,
            "d.md",
            StructureOpts {
                node_id: Some("9999".into()),
                ..Default::default()
            }
        )
        .is_err());
}

#[test]
fn test_structure_truncates_to_char_budget_with_marker() {
    let pi = index();
    pi.ingest(WS, "d.md", Source::Markdown(&big_md(30, 2)), opts(5))
        .unwrap();
    let v = pi
        .get_structure(
            WS,
            "d.md",
            StructureOpts {
                char_budget: Some(1500),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(v.truncated);
    assert!(v.omitted_nodes > 0);
    assert!(serde_json::to_string(&v.nodes).unwrap().len() < 2500);
    // Breadth first: top-level sections survive before their children.
    assert!(!v.nodes.is_empty());
    assert!(v.next_steps.iter().any(|s| s.contains("truncated")));
}

#[test]
fn test_get_page_content_parses_ranges() {
    let pi = index();
    let doc = pi
        .ingest(WS, "d.md", Source::Markdown(&big_md(6, 8)), opts(4))
        .unwrap();
    assert!(doc.page_count >= 12);
    let c = pi.get_pages(WS, "d.md", "2-3, 5,3", 0).unwrap();
    let nos: Vec<u32> = c.pages.iter().map(|p| p.page_no).collect();
    assert_eq!(nos, vec![2, 3, 5]);
    assert!(!c.truncated);
    let n = pi.get_node_pages(WS, "d.md", "0000", 0).unwrap();
    assert!(!n.pages.is_empty());
    for bad in ["", "abc", "0", "5-3", "1-2,,4"] {
        assert!(
            matches!(
                pi.get_pages(WS, "d.md", bad, 0),
                Err(PageIndexError::InvalidRange(_))
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn test_get_page_content_rejects_out_of_range() {
    let pi = index();
    let doc = pi
        .ingest(WS, "d.md", Source::Markdown(&big_md(2, 4)), opts(4))
        .unwrap();
    let over = format!("1-{}", doc.page_count + 1);
    assert!(matches!(
        pi.get_pages(WS, "d.md", &over, 0),
        Err(PageIndexError::InvalidRange(_))
    ));
    let tall = ingest_long(&pi);
    let err = pi.get_pages(WS, &tall, "1-21", 0).unwrap_err();
    assert!(err.to_string().contains("20"));
    let info = pi.get_document(WS, &tall).unwrap();
    assert!(info.structure_first);
}

fn ingest_long(pi: &PageIndex<SqliteStore>) -> String {
    let text: String = (0..60).map(|i| format!("line {i}\n")).collect();
    pi.ingest(WS, "long.txt", Source::PlainText(&text), opts(2))
        .unwrap();
    "long.txt".into()
}

#[test]
fn test_get_page_content_respects_response_char_cap() {
    let pi = index();
    let text: String = (0..40)
        .map(|i| format!("{}\n", "x".repeat(50 + i)))
        .collect();
    pi.ingest(WS, "d.txt", Source::PlainText(&text), opts(4))
        .unwrap();
    let c = pi.get_pages(WS, "d.txt", "1-5", 500).unwrap();
    let total: usize = c.pages.iter().map(|p| p.text.chars().count()).sum();
    assert!(total <= 500);
    assert!(c.truncated);
    assert!(c.remaining_pages.is_some());
    // A single oversized page is cut, not dropped.
    let one = pi.get_pages(WS, "d.txt", "1", 20).unwrap();
    assert_eq!(one.pages.len(), 1);
    assert!(one.pages[0].truncated);
    assert_eq!(one.pages[0].text.chars().count(), 20);
}

#[test]
fn test_unknown_doc_returns_similar_names() {
    let pi = index();
    for n in ["Quarterly Report.md", "Quarterly Review.md", "zzz.md"] {
        pi.ingest(WS, n, Source::Markdown("# T\n\nx\n"), opts(10))
            .unwrap();
    }
    let err = pi.get_document(WS, "Quarterly Repor.md").unwrap_err();
    let PageIndexError::NotFound(msg) = err else {
        panic!("expected NotFound");
    };
    assert!(msg.contains("Quarterly Report.md"), "{msg}");
    assert!(msg.contains("Quarterly Review.md"), "{msg}");
    assert!(!msg.contains("zzz.md"), "{msg}");
    let names = pi.similar_document_names(WS, "Quarterly Repor.md").unwrap();
    assert!(names.len() <= 3);
    assert!(pi.delete(WS, "Quarterly Report.md").is_ok());
    assert!(pi.delete(WS, "Quarterly Report.md").is_err());
}

fn search_doc() -> PageIndex<SqliteStore> {
    let pi = index();
    let mut md = String::from("# Manual\n\nIntro filler.\n\n## Battery\n\n");
    for i in 0..6 {
        md.push_str(&format!("battery filler line {i} about nothing\n"));
    }
    md.push_str("\n## Cleaning\n\n");
    for i in 0..6 {
        md.push_str(&format!("wipe the lens filler {i}\n"));
    }
    md.push_str("the rare quokka term appears exactly here in the cleaning chapter\n");
    for i in 0..30 {
        md.push_str(&format!("tail filler {i} {}\n", "x".repeat(20)));
    }
    pi.ingest(WS, "manual.md", Source::Markdown(&md), opts(5))
        .unwrap();
    pi
}

#[test]
fn test_search_ranks_page_with_terms_first() {
    let pi = search_doc();
    let r = pi.search(WS, "manual.md", "quokka term", 5).unwrap();
    assert!(!r.hits.is_empty());
    let page = pi
        .get_pages(WS, "manual.md", &r.hits[0].page_no.to_string(), 0)
        .unwrap();
    assert!(page.pages[0].text.contains("quokka"), "{r:?}");
    assert!(r.hits[0].score >= r.hits.last().unwrap().score);
    assert!(r.next_steps[0].contains("get_page_content"));
}

#[test]
fn test_search_returns_breadcrumb_and_snippet_not_full_text() {
    let pi = search_doc();
    let r = pi.search(WS, "manual.md", "quokka", 3).unwrap();
    let h = &r.hits[0];
    assert!(h.snippet.contains("quokka"), "{h:?}");
    let full = pi
        .get_pages(WS, "manual.md", &h.page_no.to_string(), 0)
        .unwrap()
        .pages[0]
        .text
        .chars()
        .count();
    assert!(h.snippet.chars().count() < full.max(300), "{h:?}");
    assert!(h.snippet.chars().count() <= 224, "{h:?}");
    assert_eq!(h.breadcrumb.first().map(String::as_str), Some("Manual"));
    assert!(h.breadcrumb.iter().any(|t| t == "Cleaning"), "{h:?}");
    assert!(h.node_id.is_some());
    // Limit is honoured and a term-less query is a typed error.
    assert!(pi.search(WS, "manual.md", "filler", 2).unwrap().hits.len() <= 2);
    assert!(matches!(
        pi.search(WS, "manual.md", "! ?", 2),
        Err(PageIndexError::InvalidRange(_))
    ));
}

#[test]
fn test_search_unknown_doc_suggests_names() {
    let pi = search_doc();
    match pi.search(WS, "manuel.md", "quokka", 3) {
        Err(PageIndexError::NotFound(m)) => assert!(m.contains("manual.md"), "{m}"),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn test_structure_hints_sample_hidden_titles_and_flag_large_nodes() {
    let pi = index();
    let mut md = String::from("# Big\n\n## Alpha part\n\n");
    for i in 0..60 {
        md.push_str(&format!("line {i}\n"));
    }
    md.push_str("\n## Beta part\n\nmore\n");
    pi.ingest(WS, "big.md", Source::Markdown(&md), opts(2))
        .unwrap();
    let v = pi
        .get_structure(
            WS,
            "big.md",
            StructureOpts {
                max_depth: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
    let root = &v.nodes[0];
    assert_eq!(root.hidden_children, 2);
    assert_eq!(
        root.descendant_titles_sample,
        ["Alpha part", "Beta part"],
        "{root:?}"
    );
    assert!(root.too_large_for_one_call);
    assert!(v
        .next_steps
        .iter()
        .any(|s| s.contains("too_large_for_one_call")));
    let json = serde_json::to_value(&v).unwrap();
    assert!(json["nodes"][0].get("too_large_for_one_call").is_some());
}
