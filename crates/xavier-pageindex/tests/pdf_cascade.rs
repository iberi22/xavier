//! PDF cascade: bookmarks -> layout -> LLM ToC -> fixed windows.

#[cfg(all(feature = "sqlite", not(feature = "pdf-outline")))]
#[test]
fn test_pdf_ingest_default_features_returns_feature_disabled_error() {
    use xavier_pageindex::store::SqliteStore;
    use xavier_pageindex::{IngestOptions, PageIndex, PageIndexError, Source};

    let idx = PageIndex::new(SqliteStore::open_in_memory().unwrap());
    let err = idx
        .ingest(
            "ws",
            "a.pdf",
            Source::Pdf(b"%PDF-1.4"),
            IngestOptions::default(),
        )
        .unwrap_err();
    assert!(
        matches!(err, PageIndexError::FeatureDisabled("pdf")),
        "{err}"
    );
}

#[cfg(feature = "pdf-outline")]
mod with_pdf {
    use std::sync::Mutex;

    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream, StringFormat};
    use xavier_pageindex::pdf::cascade::{
        build_pdf_tree, TocSource, FIXED_WINDOW_PAGES, MAX_PAGES_PER_NODE,
    };
    use xavier_pageindex::summarize::Summarizer;
    use xavier_pageindex::PageIndexError;

    /// One text line: font size, bold, text.
    type Line = (i64, bool, String);

    fn body(s: &str) -> Line {
        (11, false, s.to_string())
    }

    fn heading(s: &str) -> Line {
        (24, true, s.to_string())
    }

    /// PDF with one entry of `pages` per page; `bookmarks` are `(title, page)`.
    fn make_pdf(pages: &[Vec<Line>], bookmarks: &[(&str, usize)]) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let f1 = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let f2 = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold",
        });
        let res = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => f1, "F2" => f2 } });
        let mut ids = Vec::new();
        for lines in pages {
            let mut ops = Vec::new();
            let mut y = 740;
            for (size, bold, text) in lines {
                ops.push(Operation::new("BT", vec![]));
                let font = if *bold { "F2" } else { "F1" };
                ops.push(Operation::new("Tf", vec![font.into(), (*size).into()]));
                ops.push(Operation::new("Td", vec![50.into(), y.into()]));
                ops.push(Operation::new(
                    "Tj",
                    vec![Object::string_literal(text.as_str())],
                ));
                ops.push(Operation::new("ET", vec![]));
                y -= size * 2 + 6;
            }
            let cid = doc.add_object(Stream::new(
                dictionary! {},
                Content { operations: ops }.encode().unwrap(),
            ));
            ids.push(doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => cid,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Resources" => res,
            }));
        }
        let kids: Vec<Object> = ids.iter().map(|&i| i.into()).collect();
        doc.objects.insert(
            pages_id,
            dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => pages.len() as i64 }.into(),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        if !bookmarks.is_empty() {
            let items: Vec<_> = bookmarks
                .iter()
                .map(|(t, p)| {
                    doc.add_object(dictionary! {
                        "Title" => Object::String(t.as_bytes().to_vec(), StringFormat::Literal),
                        "Dest" => vec![Object::from(ids[p - 1]), "Fit".into()],
                    })
                })
                .collect();
            let root = doc.new_object_id();
            for (i, &id) in items.iter().enumerate() {
                let d = doc.get_dictionary_mut(id).unwrap();
                d.set("Parent", root);
                if i > 0 {
                    d.set("Prev", items[i - 1]);
                }
                if i + 1 < items.len() {
                    d.set("Next", items[i + 1]);
                }
            }
            doc.objects.insert(
                root,
                dictionary! {
                    "Type" => "Outlines", "First" => items[0],
                    "Last" => *items.last().unwrap(), "Count" => items.len() as i64,
                }
                .into(),
            );
            doc.get_dictionary_mut(catalog)
                .unwrap()
                .set("Outlines", root);
        }
        let mut buf = Vec::new();
        doc.save_to(&mut buf).unwrap();
        buf
    }

    fn filler() -> Vec<Line> {
        (0..6)
            .map(|i| {
                body(&format!(
                    "Lorem ipsum dolor sit amet consectetur adipiscing {i}"
                ))
            })
            .collect()
    }

    /// Chapters One/Two/Three on pages 1, 3 and 5 of a 6-page document.
    fn chaptered() -> Vec<Vec<Line>> {
        let mut pages = Vec::new();
        for (i, name) in [(1, "Chapter One"), (3, "Chapter Two"), (5, "Chapter Three")] {
            let mut p = vec![heading(name)];
            p.extend(filler());
            pages.push(p);
            let _ = i;
            pages.push(filler());
        }
        pages
    }

    struct Fake(Mutex<Vec<String>>, Result<String, ()>);

    impl Fake {
        fn ok(reply: &str) -> Self {
            Self(Mutex::new(Vec::new()), Ok(reply.into()))
        }
    }

    impl Summarizer for Fake {
        fn model(&self) -> &str {
            "fake"
        }
        fn summarize(&self, _: &str, _: &str) -> Result<String, PageIndexError> {
            Ok("This section describes the document.".into())
        }
        fn complete(&self, text: &str) -> Result<String, PageIndexError> {
            self.0.lock().unwrap().push(text.to_string());
            self.1
                .clone()
                .map_err(|_| PageIndexError::Build("llm down".into()))
        }
    }

    fn titles(t: &xavier_pageindex::DocumentTree) -> Vec<(String, u32)> {
        t.roots
            .iter()
            .map(|n| (n.title.clone(), n.start_page))
            .collect()
    }

    #[test]
    fn test_pdf_cascade_prefers_bookmarks() {
        let bytes = make_pdf(&chaptered(), &[("Intro", 1), ("Middle", 3), ("End", 5)]);
        let llm = Fake::ok("1|1|Chapter One\n1|3|Chapter Two");
        let b = build_pdf_tree("d", &bytes, Some(&llm)).unwrap();
        assert_eq!(b.source, TocSource::Bookmarks);
        assert_eq!(b.builder(), "pdf-bookmarks");
        assert_eq!(
            titles(&b.tree),
            [("Intro".into(), 1), ("Middle".into(), 3), ("End".into(), 5)]
        );
        assert_eq!(b.pages.len(), 6);
        assert!(b.pages[0].text.contains("Chapter One"));
        assert!(llm.0.lock().unwrap().is_empty(), "LLM must not be called");
        b.tree.validate(6).unwrap();
    }

    /// Needs libpdfium (`XAVIER_PAGEINDEX_PDFIUM_LIB`); run with `--include-ignored`.
    #[cfg(feature = "pdf-layout")]
    #[test]
    #[ignore = "requires libpdfium"]
    fn test_pdf_cascade_falls_back_to_layout() {
        let bytes = make_pdf(&chaptered(), &[]);
        let b = build_pdf_tree("d", &bytes, None).unwrap();
        assert_eq!(b.source, TocSource::Layout, "{:?}", b.tree);
        assert_eq!(
            titles(&b.tree),
            [
                ("Chapter One".into(), 1),
                ("Chapter Two".into(), 3),
                ("Chapter Three".into(), 5)
            ]
        );
        assert!(b.pages[2].text.contains("Chapter Two"));
        b.tree.validate(6).unwrap();
    }

    /// Uniform-font pages: no headings for layout, so the LLM ToC is used.
    fn plain_pages() -> Vec<Vec<Line>> {
        let mut pages = Vec::new();
        for name in ["Alpha Basics", "Beta Methods", "Gamma Results"] {
            let mut p = vec![body(name)];
            p.extend(filler());
            pages.push(p);
        }
        pages
    }

    #[test]
    fn test_pdf_cascade_falls_back_to_llm_toc_when_no_layout() {
        let bytes = make_pdf(&plain_pages(), &[]);
        // The bogus entry (page 3 does not contain "Delta") must be dropped.
        let llm = Fake::ok(
            "1|1|Alpha Basics\n2|2|Beta Methods\n1|3|Gamma Results\n1|3|Delta Nonsense\nnoise",
        );
        let b = build_pdf_tree("d", &bytes, Some(&llm)).unwrap();
        assert_eq!(b.source, TocSource::Llm);
        assert_eq!(b.builder(), "pdf-llm-toc");
        // "Beta Methods" is at level 2 under "Alpha Basics" (page 2 of 3).
        assert_eq!(
            titles(&b.tree),
            [("Alpha Basics".into(), 1), ("Gamma Results".into(), 3)]
        );
        assert_eq!(b.tree.roots[0].children[0].title, "Beta Methods");
        let sent = llm.0.lock().unwrap();
        assert!(sent[0].contains("=== PAGE 2 ===") && sent[0].contains("Beta Methods"));
        b.tree.validate(3).unwrap();
    }

    /// Prose from `summarize`, a real ToC only from `complete`.
    struct Split;

    impl Summarizer for Split {
        fn model(&self) -> &str {
            "split"
        }
        fn summarize(&self, _: &str, _: &str) -> Result<String, PageIndexError> {
            Ok("This document covers alpha, beta and gamma topics.".into())
        }
        fn complete(&self, prompt: &str) -> Result<String, PageIndexError> {
            assert!(prompt.starts_with("TASK: list the table of contents"));
            Ok("1|1|Alpha Basics\n1|2|Beta Methods\n1|3|Gamma Results".into())
        }
    }

    #[test]
    fn test_pdf_cascade_llm_toc_uses_raw_complete_not_summarize() {
        let bytes = make_pdf(&plain_pages(), &[]);
        let b = build_pdf_tree("d", &bytes, Some(&Split)).unwrap();
        assert_eq!(b.source, TocSource::Llm);
        assert_eq!(b.builder(), "pdf-llm-toc");
        assert_eq!(b.tree.roots.len(), 3);
    }

    #[test]
    fn test_pdf_cascade_llm_failure_or_garbage_degrades_to_fixed_windows() {
        let bytes = make_pdf(&plain_pages(), &[]);
        let down = Fake(Mutex::new(Vec::new()), Err(()));
        let b = build_pdf_tree("d", &bytes, Some(&down)).unwrap();
        assert_eq!(b.source, TocSource::FixedWindows);
        let prose = Fake::ok("This document covers alpha, beta and gamma topics.");
        let b = build_pdf_tree("d", &bytes, Some(&prose)).unwrap();
        assert_eq!(b.source, TocSource::FixedWindows);
    }

    #[test]
    fn test_pdf_cascade_final_fallback_fixed_windows() {
        let pages: Vec<Vec<Line>> = (0..12).map(|_| filler()).collect();
        let bytes = make_pdf(&pages, &[]);
        let b = build_pdf_tree("d", &bytes, None).unwrap();
        assert_eq!(b.source, TocSource::FixedWindows);
        assert_eq!(b.builder(), "pdf-fixed-windows");
        let w = FIXED_WINDOW_PAGES;
        assert_eq!(b.tree.roots.len(), 3);
        assert_eq!(
            (b.tree.roots[0].start_page, b.tree.roots[0].end_page),
            (1, w)
        );
        assert_eq!(b.tree.roots[2].end_page, 12);
        b.tree.validate(12).unwrap();
    }

    const NAMES: [&str; 12] = [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
        "juliet", "kilo", "lima",
    ];

    /// Pages whose first line is unique but that carry a repeated running footer.
    fn distinct_pages(n: usize) -> Vec<Vec<Line>> {
        (0..n)
            .map(|i| {
                let mut p = vec![
                    body(&format!(
                        "Warranty terms for {} family",
                        NAMES[i % NAMES.len()]
                    )),
                    body("ACME Corp Confidential Footer Line"),
                ];
                p.extend(filler());
                p
            })
            .collect()
    }

    #[test]
    fn test_pdf_fixed_windows_are_titled_by_first_meaningful_line() {
        let bytes = make_pdf(&distinct_pages(12), &[]);
        let b = build_pdf_tree("d", &bytes, None).unwrap();
        assert_eq!(b.source, TocSource::FixedWindows);
        let t: Vec<&str> = b.tree.roots.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(
            t,
            [
                "Warranty terms for alpha family",
                "Warranty terms for foxtrot family",
                "Warranty terms for kilo family"
            ]
        );
        // Page ranges are unchanged.
        assert_eq!(b.tree.roots[1].start_page, 6);
        assert_eq!(b.tree.roots[1].end_page, 10);
        // Windows without any usable line keep the generic label.
        let blank = make_pdf(&filler_pages(12), &[]);
        let b = build_pdf_tree("d", &blank, None).unwrap();
        assert_eq!(b.tree.roots[0].title, "Pages 1-5");
    }

    #[test]
    fn test_pdf_split_windows_are_titled_by_first_meaningful_line() {
        let bytes = make_pdf(&distinct_pages(30), &[("A", 1), ("B", 25)]);
        let b = build_pdf_tree("d", &bytes, None).unwrap();
        let a = &b.tree.roots[0];
        assert_eq!(a.children[0].title, "Warranty terms for alpha family");
        assert_eq!(a.children[1].title, "Warranty terms for foxtrot family");
        assert!(a.children.iter().all(|c| !c.title.contains("(pages")));
        assert_eq!(a.children[1].start_page, 6);
        assert_eq!(a.children[1].end_page, 10);
    }

    /// Bookmarked PDF whose words are separate text objects on one baseline.
    fn word_by_word_pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let f1 = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let res = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => f1 } });
        let mut ids = Vec::new();
        for _ in 0..2 {
            let mut ops = Vec::new();
            for (i, w) in "Our comprehensive solutions extend more broadly"
                .split(' ')
                .enumerate()
            {
                ops.push(Operation::new("BT", vec![]));
                ops.push(Operation::new("Tf", vec!["F1".into(), 11.into()]));
                ops.push(Operation::new(
                    "Td",
                    vec![(50 + i as i64 * 80).into(), 700.into()],
                ));
                ops.push(Operation::new("Tj", vec![Object::string_literal(w)]));
                ops.push(Operation::new("ET", vec![]));
            }
            let cid = doc.add_object(Stream::new(
                dictionary! {},
                Content { operations: ops }.encode().unwrap(),
            ));
            ids.push(doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => cid,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Resources" => res,
            }));
        }
        let kids: Vec<Object> = ids.iter().map(|&i| i.into()).collect();
        doc.objects.insert(
            pages_id,
            dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 2 }.into(),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut buf = Vec::new();
        doc.save_to(&mut buf).unwrap();
        buf
    }

    /// Needs libpdfium; page text is rebuilt into lines from glyph positions.
    #[cfg(feature = "pdf-layout")]
    #[test]
    #[ignore = "requires libpdfium"]
    fn test_pdf_page_text_joins_words_on_one_baseline_into_lines() {
        let b = build_pdf_tree("d", &word_by_word_pdf(), None).unwrap();
        assert_eq!(b.pages.len(), 2);
        for p in &b.pages {
            assert!(
                p.text
                    .contains("Our comprehensive solutions extend more broadly"),
                "{:?}",
                p.text
            );
        }
    }

    fn filler_pages(n: usize) -> Vec<Vec<Line>> {
        (0..n).map(|_| filler()).collect()
    }

    fn count(nodes: &[xavier_pageindex::TreeNode]) -> usize {
        nodes.iter().map(|n| 1 + count(&n.children)).sum()
    }

    #[test]
    fn test_pdf_cascade_single_bookmark_falls_through() {
        let bytes = make_pdf(&filler_pages(12), &[("Only", 1)]);
        let b = build_pdf_tree("d", &bytes, None).unwrap();
        assert_ne!(b.source, TocSource::Bookmarks);
        assert!(count(&b.tree.roots) >= 2);
        b.tree.validate(12).unwrap();
        // A short document may keep its single bookmark.
        let short = make_pdf(&filler_pages(3), &[("Only", 1)]);
        let b = build_pdf_tree("d", &short, None).unwrap();
        assert_eq!(b.source, TocSource::Bookmarks);
    }

    #[test]
    fn test_pdf_oversized_leaf_is_split_into_windows() {
        let bytes = make_pdf(&filler_pages(30), &[("A", 1), ("B", 25)]);
        let b = build_pdf_tree("d", &bytes, None).unwrap();
        assert_eq!(b.source, TocSource::Bookmarks);
        b.tree.validate(30).unwrap();
        let a = &b.tree.roots[0];
        assert_eq!((a.start_page, a.end_page), (1, 25));
        assert!(a.children.len() >= 2);
        assert_eq!(a.children[0].start_page, 1);
        assert_eq!(a.children.last().unwrap().end_page, 25);
        fn leaves_ok(nodes: &[xavier_pageindex::TreeNode]) -> bool {
            nodes.iter().all(|n| {
                if n.children.is_empty() {
                    n.end_page - n.start_page < MAX_PAGES_PER_NODE
                } else {
                    leaves_ok(&n.children)
                }
            })
        }
        // "B" (25..30) is within the limit and stays a leaf.
        assert!(leaves_ok(&b.tree.roots));
        assert!(b.tree.roots[1].children.is_empty());
    }

    #[test]
    fn test_pdf_nodes_have_token_estimates() {
        let bytes = make_pdf(&filler_pages(30), &[("A", 1), ("B", 25)]);
        for built in [
            build_pdf_tree("d", &bytes, None).unwrap(),
            build_pdf_tree("d", &make_pdf(&filler_pages(12), &[]), None).unwrap(),
        ] {
            fn all_positive(nodes: &[xavier_pageindex::TreeNode]) -> bool {
                nodes
                    .iter()
                    .all(|n| n.token_estimate > 0 && all_positive(&n.children))
            }
            assert!(all_positive(&built.tree.roots));
        }
    }

    #[test]
    fn test_pdf_cascade_malformed_is_an_error_not_a_panic() {
        assert!(build_pdf_tree("d", b"not a pdf", None).is_err());
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn test_pdf_facade_ingest_records_toc_source_in_get_document() {
        use xavier_pageindex::store::SqliteStore;
        use xavier_pageindex::{IngestOptions, PageIndex, PageUnit, Source, SourceKind};

        let idx = PageIndex::new(SqliteStore::open_in_memory().unwrap());
        let bytes = make_pdf(&chaptered(), &[("Intro", 1), ("Middle", 3)]);
        let doc = idx
            .ingest("ws", "a.pdf", Source::Pdf(&bytes), IngestOptions::default())
            .unwrap();
        assert_eq!(doc.source_kind, SourceKind::Pdf);
        assert_eq!(doc.page_unit, PageUnit::Page);
        assert_eq!(doc.page_count, 6);
        let info = idx.get_document("ws", "a.pdf").unwrap();
        assert_eq!(info.builder, "pdf-bookmarks");
        let p = idx.get_pages("ws", "a.pdf", "3", 0).unwrap();
        assert!(format!("{p:?}").contains("Chapter Two"));
    }
}
