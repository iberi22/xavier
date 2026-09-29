#![cfg(feature = "pdf-layout")]

use std::path::{Path, PathBuf};
use xavier_pageindex::error::PageIndexError;
use xavier_pageindex::pdf::layout::{
    classify_headings, detect_headings, try_detect_headings, LayoutParams, LineRecord,
};
use xavier_pageindex::pdf::pdfium_loader::{bind_first, candidate_paths};

fn line(page: u32, top: f32, text: &str, size: f32, bold: bool) -> LineRecord {
    LineRecord {
        page,
        top,
        page_height: 800.0,
        text: text.into(),
        font_size: size,
        bold,
        chars: text.chars().filter(|c| !c.is_whitespace()).count() as u32,
    }
}

fn body(page: u32, top: f32) -> LineRecord {
    line(
        page,
        top,
        "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do",
        11.0,
        false,
    )
}

#[test]
fn test_layout_font_size_clusters_map_to_levels() {
    let lines = vec![
        line(1, 100.0, "Big Title", 24.0, true),
        body(1, 130.0),
        body(1, 145.0),
        body(1, 160.0),
        line(1, 200.0, "Section One", 16.0, true),
        body(1, 230.0),
        body(1, 245.0),
        line(2, 100.0, "Another Section", 16.2, true),
        body(2, 130.0),
        line(2, 300.0, "Subsection", 13.0, false),
        body(2, 330.0),
        body(2, 345.0),
    ];
    let h = classify_headings(&lines, &LayoutParams::default());
    let got: Vec<(&str, u32, u32)> = h
        .iter()
        .map(|c| (c.title.as_str(), c.page, c.level))
        .collect();
    assert_eq!(
        got,
        vec![
            ("Big Title", 1, 1),
            ("Section One", 1, 2),
            ("Another Section", 2, 2),
            ("Subsection", 2, 3),
        ]
    );
}

#[test]
fn test_layout_headers_footers_filtered() {
    let mut lines = Vec::new();
    for p in 1..=4 {
        // Running header in a larger size and a page-number footer.
        lines.push(line(p, 20.0, "ACME Annual Report", 14.0, true));
        lines.push(body(p, 200.0));
        lines.push(body(p, 215.0));
        lines.push(line(p, 780.0, &format!("Page {p}"), 9.0, false));
    }
    lines.push(line(2, 100.0, "Real Heading", 18.0, true));
    let h = classify_headings(&lines, &LayoutParams::default());
    let titles: Vec<&str> = h.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["Real Heading"]);
}

#[test]
fn test_layout_bold_short_line_is_heading_candidate() {
    let lines = vec![
        line(1, 100.0, "Overview", 11.0, true),
        body(1, 120.0),
        body(1, 135.0),
        body(1, 150.0),
        // Long bold sentence is emphasis, not a heading.
        line(
            1,
            170.0,
            "This entire sentence is bold but it is far too long to be a section heading in any document ever written",
            11.0,
            true,
        ),
        body(1, 190.0),
    ];
    let h = classify_headings(&lines, &LayoutParams::default());
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].title, "Overview");
    assert_eq!(h[0].level, 1);
}

#[test]
fn test_layout_missing_pdfium_returns_typed_error_not_panic() {
    let missing = vec![PathBuf::from("/nonexistent/xavier/libpdfium.so")];
    let Err(err) = bind_first(&missing, false) else {
        panic!("must fail")
    };
    assert!(
        matches!(err, PageIndexError::PdfiumUnavailable(_)),
        "{err:?}"
    );

    // A file that exists but is not a shared library also yields the typed error.
    let junk = std::env::temp_dir().join("xavier_pageindex_not_a_lib.so");
    std::fs::write(&junk, b"not a library").unwrap();
    let res = bind_first(std::slice::from_ref(&junk), false);
    let _ = std::fs::remove_file(&junk);
    let Err(err) = res else { panic!("must fail") };
    assert!(
        matches!(err, PageIndexError::PdfiumUnavailable(_)),
        "{err:?}"
    );
}

#[test]
fn test_layout_pdfium_path_resolution_env_order() {
    let dir = std::env::temp_dir();
    let exe = Path::new("/opt/app");
    let c = candidate_paths(
        Some("/a/libpdfium.so"),
        Some(dir.to_str().unwrap()),
        Some(exe),
    );
    assert_eq!(c.len(), 3);
    assert_eq!(c[0], Path::new("/a/libpdfium.so"));
    assert!(
        c[1].starts_with(&dir)
            && c[1]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("pdfium")
    );
    assert!(c[2].starts_with(exe));

    // Empty values are skipped; the executable dir stays last.
    let c = candidate_paths(Some("  "), None, Some(exe));
    assert_eq!(c.len(), 1);
    assert!(c[0].starts_with(exe));
}

/// Minimal 2-page PDF: Helvetica body, Helvetica-Bold headings of two sizes.
fn sample_pdf() -> Vec<u8> {
    fn page_stream(items: &[(&str, f32, f32, &str)]) -> String {
        let mut s = String::new();
        for (font, size, y, text) in items {
            s.push_str(&format!("BT /{font} {size} Tf 72 {y} Td ({text}) Tj ET\n"));
        }
        s
    }
    let p1 = page_stream(&[
        ("F2", 24.0, 720.0, "Main Title"),
        (
            "F1",
            11.0,
            690.0,
            "Body text line one of the introduction paragraph.",
        ),
        (
            "F1",
            11.0,
            675.0,
            "Body text line two of the introduction paragraph.",
        ),
        (
            "F1",
            11.0,
            660.0,
            "Body text line three of the introduction paragraph.",
        ),
        ("F2", 16.0, 600.0, "First Section"),
        (
            "F1",
            11.0,
            570.0,
            "Section body text goes here and continues on.",
        ),
        (
            "F1",
            11.0,
            555.0,
            "More section body text goes here and continues.",
        ),
    ]);
    let p2 = page_stream(&[
        ("F2", 16.0, 720.0, "Second Section"),
        (
            "F1",
            11.0,
            690.0,
            "Second page body text goes here and continues.",
        ),
        (
            "F1",
            11.0,
            675.0,
            "More second page body text goes here as well.",
        ),
        (
            "F1",
            11.0,
            660.0,
            "Yet more second page body text for statistics.",
        ),
    ]);
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R /Resources << /Font << /F1 7 0 R /F2 8 0 R >> >> >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 6 0 R /Resources << /Font << /F1 7 0 R /F2 8 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}endstream", p1.len(), p1),
        format!("<< /Length {} >>\nstream\n{}endstream", p2.len(), p2),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{}\nendobj\n", i + 1, o).into_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).into_bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").into_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .into_bytes(),
    );
    out
}

#[test]
#[ignore = "needs libpdfium (XAVIER_PAGEINDEX_PDFIUM_LIB / PDFIUM_DYNAMIC_LIB_PATH); run with --include-ignored"]
fn test_layout_pdfium_detects_headings_from_generated_pdf() {
    let pdf = sample_pdf();
    let h = match try_detect_headings(&pdf) {
        Ok(h) => h,
        Err(PageIndexError::PdfiumUnavailable(m)) => {
            eprintln!("skipped: {m}");
            return;
        }
        Err(e) => panic!("unexpected error: {e}"),
    };
    let got: Vec<(&str, u32, u32)> = h
        .iter()
        .map(|c| (c.title.as_str(), c.page, c.level))
        .collect();
    assert_eq!(
        got,
        vec![
            ("Main Title", 1, 1),
            ("First Section", 1, 2),
            ("Second Section", 2, 2)
        ]
    );
    assert!(detect_headings(&pdf).is_some());
}
