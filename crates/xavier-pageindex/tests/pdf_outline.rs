#![cfg(feature = "pdf-outline")]

use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, ObjectId, Stream, StringFormat};
use xavier_pageindex::pdf::outline::{
    extract_outline, extract_pages, outline_to_tree, OutlineNode,
};
use xavier_pageindex::PageIndexError;

fn title(s: &str) -> Object {
    Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
}

/// Build an n-page PDF; returns the document, the catalog id and page ids.
fn base_pdf(n: usize) -> (Document, ObjectId, Vec<ObjectId>) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut page_ids = Vec::new();
    for i in 1..=n {
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![50.into(), 700.into()]),
                Operation::new(
                    "Tj",
                    vec![Object::string_literal(format!("Text of page {i}"))],
                ),
                Operation::new("ET", vec![]),
            ],
        };
        let cid = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        page_ids.push(doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => cid,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => resources,
        }));
    }
    let kids: Vec<Object> = page_ids.iter().map(|&i| i.into()).collect();
    doc.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n as i64 }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    (doc, catalog, page_ids)
}

/// Link `items` (object ids) as an outline chain under a new /Outlines root.
fn link_outline(doc: &mut Document, catalog: ObjectId, items: &[ObjectId]) {
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
            "Type" => "Outlines", "First" => items[0], "Last" => *items.last().unwrap(),
            "Count" => items.len() as i64,
        }
        .into(),
    );
    doc.get_dictionary_mut(catalog)
        .unwrap()
        .set("Outlines", root);
}

fn to_bytes(mut doc: Document) -> Vec<u8> {
    let mut buf = Vec::new();
    doc.save_to(&mut buf).unwrap();
    buf
}

#[test]
fn test_pdf_outline_bookmarks_to_tree() {
    let (mut doc, catalog, pages) = base_pdf(6);
    let dest = |p: usize| vec![Object::from(pages[p - 1]), "Fit".into()];
    let ch1 = doc.add_object(dictionary! { "Title" => title("Chapter 1"), "Dest" => dest(1) });
    let sec = doc.add_object(dictionary! { "Title" => title("Section 1.1"), "Dest" => dest(2) });
    // Second chapter uses a GoTo action instead of /Dest.
    let ch2 = doc.add_object(dictionary! {
        "Title" => title("Chapter 2"),
        "A" => dictionary! { "S" => "GoTo", "D" => dest(4) },
    });
    let d = doc.get_dictionary_mut(ch1).unwrap();
    d.set("First", sec);
    d.set("Last", sec);
    doc.get_dictionary_mut(sec).unwrap().set("Parent", ch1);
    link_outline(&mut doc, catalog, &[ch1, ch2]);

    let outline = extract_outline(&to_bytes(doc)).unwrap().unwrap();
    assert_eq!(outline.len(), 2);
    assert_eq!(
        (outline[0].title.as_str(), outline[0].page),
        ("Chapter 1", 1)
    );
    assert_eq!(outline[0].children.len(), 1);
    assert_eq!(
        (
            outline[0].children[0].title.as_str(),
            outline[0].children[0].page
        ),
        ("Section 1.1", 2)
    );
    assert_eq!(
        (outline[1].title.as_str(), outline[1].page),
        ("Chapter 2", 4)
    );

    let tree = outline_to_tree("d", &outline, 6);
    tree.validate(6).unwrap();
    assert_eq!((tree.roots[0].start_page, tree.roots[0].end_page), (1, 4));
    assert_eq!((tree.roots[1].start_page, tree.roots[1].end_page), (4, 6));
    assert_eq!(tree.roots[0].children[0].end_page, 4);
}

#[test]
fn test_pdf_outline_resolves_named_destinations() {
    let (mut doc, catalog, pages) = base_pdf(4);
    let dest = |p: usize| vec![Object::from(pages[p - 1]), "Fit".into()];
    let a =
        doc.add_object(dictionary! { "Title" => title("Via name tree"), "Dest" => title("intro") });
    let b = doc.add_object(dictionary! { "Title" => title("Via legacy dests"), "Dest" => Object::Name(b"legacy".to_vec()) });
    let names = dictionary! {
        "Dests" => dictionary! { "Kids" => vec![Object::from(dictionary! {
            "Names" => vec![title("intro"), dictionary! { "D" => dest(2) }.into()],
        })] },
    };
    let d = doc.get_dictionary_mut(catalog).unwrap();
    d.set("Names", names);
    d.set("Dests", dictionary! { "legacy" => dest(3) });
    link_outline(&mut doc, catalog, &[a, b]);

    let outline = extract_outline(&to_bytes(doc)).unwrap().unwrap();
    let got: Vec<(&str, u32)> = outline.iter().map(|n| (n.title.as_str(), n.page)).collect();
    assert_eq!(got, [("Via name tree", 2), ("Via legacy dests", 3)]);
}

#[test]
fn test_pdf_without_outline_returns_none() {
    let (doc, _, _) = base_pdf(2);
    assert_eq!(extract_outline(&to_bytes(doc)).unwrap(), None);
}

#[test]
fn test_pdf_page_text_extracted_per_page() {
    let (doc, _, _) = base_pdf(3);
    let pages = extract_pages(&to_bytes(doc)).unwrap();
    assert_eq!(pages.len(), 3);
    for (i, p) in pages.iter().enumerate() {
        assert!(
            p.contains(&format!("Text of page {}", i + 1)),
            "page {i}: {p:?}"
        );
    }
}

#[test]
fn test_pdf_encrypted_returns_typed_error() {
    let (mut doc, _, _) = base_pdf(1);
    let enc = doc.add_object(dictionary! {
        "Filter" => "Standard", "V" => 1, "R" => 2, "P" => -4,
        "O" => title("0123456789abcdef0123456789abcdef"),
        "U" => title("0123456789abcdef0123456789abcdef"),
    });
    doc.trailer.set("Encrypt", enc);
    let bytes = to_bytes(doc);
    assert!(matches!(
        extract_outline(&bytes),
        Err(PageIndexError::Encrypted)
    ));
    assert!(matches!(
        extract_pages(&bytes),
        Err(PageIndexError::Encrypted)
    ));
}

#[test]
fn test_pdf_malformed_never_panics() {
    for junk in [&b""[..], b"%PDF-1.5\ngarbage", b"not a pdf at all"] {
        assert!(extract_outline(junk).is_err());
        assert!(extract_pages(junk).is_err());
    }
}

#[test]
fn test_pdf_outline_keeps_same_page_bookmarks() {
    let node = |title: &str, page: u32, children| OutlineNode {
        title: title.into(),
        page,
        children,
    };
    let outline = vec![
        node("A", 2, vec![node("A1", 2, vec![]), node("A2", 2, vec![])]),
        node("B", 2, vec![]),
        node("C", 4, vec![]),
        // Before the previous sibling's start: dropped.
        node("Back", 3, vec![]),
        node("Out", 99, vec![]),
    ];
    let tree = outline_to_tree("d", &outline, 5);
    tree.validate(5).unwrap();
    let titles: Vec<_> = tree.roots.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(titles, ["A", "B", "C"]);
    assert_eq!((tree.roots[0].start_page, tree.roots[0].end_page), (2, 2));
    assert_eq!((tree.roots[1].start_page, tree.roots[1].end_page), (2, 4));
    assert_eq!((tree.roots[2].start_page, tree.roots[2].end_page), (4, 5));
    assert_eq!(tree.roots[0].children.len(), 2);
}
