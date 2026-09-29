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
    use xavier_pageindex::pdf::cascade::{build_pdf_tree, TocSource, FIXED_WINDOW_PAGES};
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
        fn summarize(&self, _: &str, text: &str) -> Result<String, PageIndexError> {
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
