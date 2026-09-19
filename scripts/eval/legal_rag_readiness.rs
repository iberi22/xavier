// Included by legal_rag_readiness.py in an isolated offline Cargo package.
use documents::{image_extractor::ImageExtractor, GenericDocumentExtractor};
use serde_json::{json, Value};
use std::{fs, io::Cursor, path::Path};

fn stream(body: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendstream");
    out
}

// Real PDF objects, page tree, byte offsets and xref, not just a PDF-like string.
fn pdf(objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in &offsets[1..] {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    out
}

fn text_pdf(contents: &[&[u8]], two_pages: bool, reversed: bool) -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        if two_pages { b"<< /Type /Pages /Kids [3 0 R 7 0 R] /Count 2 >>".to_vec() }
        else { b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec() },
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents {} >>",
            if two_pages && reversed {"6 0 R"} else if contents.len() == 2 && !two_pages {"[5 0 R 6 0 R]"} else {"5 0 R"}).into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
    ];
    objects.extend(contents.iter().map(|text| stream(text)));
    if two_pages {
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents {} 0 R >>", if reversed {5} else {6}).into_bytes());
    }
    pdf(objects)
}

fn check(checks: &mut Vec<Value>, name: &str, passed: bool, observed: Value) {
    checks.push(json!({"name": name, "passed": passed, "observed": observed}));
}

fn main() -> anyhow::Result<()> {
    let out = std::env::args().nth(1).expect("output directory");
    let out = Path::new(&out);
    let fixtures = out.join("fixtures");
    fs::create_dir_all(&fixtures)?;
    let extractor = GenericDocumentExtractor::new();
    let mut checks = vec![];
    let clause = b"BT /F1 12 Tf 50 740 Td (CLAUSULA 1. Pago de 7500000 COP en 30 dias.) Tj ET";
    let exception = b"BT /F1 12 Tf 50 700 Td (EXCEPCION: sin penalidad por fuerza mayor.) Tj ET";
    let simple = text_pdf(&[clause], false, false);
    fs::write(fixtures.join("contract.pdf"), &simple)?;
    let doc = extractor.extract(Path::new("contract.pdf"), &simple)?;
    check(
        &mut checks,
        "pdf_native_amount",
        doc.content.contains("7500000"),
        json!(doc.content),
    );
    let split = text_pdf(&[clause, exception], false, false);
    fs::write(fixtures.join("one_page_two_streams.pdf"), &split)?;
    let doc =
        documents::extract_pdf_document(Path::new("one_page_two_streams.pdf"), &split).unwrap();
    check(
        &mut checks,
        "pdf_physical_page_citation",
        doc.total_pages == 1 && doc.chunks.iter().all(|c| c.page == Some(1)),
        json!({"expected_pages": 1, "reported_pages": doc.total_pages, "chunk_pages": doc.chunks.iter().map(|c| c.page).collect::<Vec<_>>()}),
    );
    let reordered = text_pdf(&[exception, clause], true, true);
    fs::write(fixtures.join("reordered_objects.pdf"), &reordered)?;
    let doc = extractor.extract(Path::new("reordered_objects.pdf"), &reordered)?;
    check(
        &mut checks,
        "pdf_page_tree_order",
        doc.chunks
            .first()
            .is_some_and(|c| c.content.contains("7500000")),
        json!(doc.chunks),
    );
    let accented = text_pdf(
        &[b"BT /F1 12 Tf 50 740 Td (CL\\301USULA: indemnizaci\\363n y terminaci\\363n.) Tj ET"],
        false,
        false,
    );
    fs::write(fixtures.join("winansi_contract.pdf"), &accented)?;
    let doc = extractor.extract(Path::new("winansi_contract.pdf"), &accented)?;
    check(
        &mut checks,
        "pdf_spanish_octal_encoding",
        doc.content.contains("CLÁUSULA") && doc.content.contains("indemnización"),
        json!(doc.content),
    );
    check(
        &mut checks,
        "reject_malformed_pdf",
        extractor
            .extract(Path::new("broken.pdf"), b"%PDF-1.4\nnot a PDF")
            .is_err(),
        json!("Header-only malformed PDF must be rejected"),
    );

    // A clear, high contrast raster amount; the fixture name does not reveal it.
    let glyphs = [
        [31u8, 17, 17, 17, 17, 17, 31], // 0
        [31u8, 1, 2, 4, 8, 8, 8],       // 7
        [31u8, 16, 16, 31, 1, 1, 31],   // 5
    ];
    let mut raster = image::GrayImage::from_pixel(720, 140, image::Luma([255]));
    for (i, glyph) in [1usize, 2, 0, 0, 0, 0, 0].iter().enumerate() {
        for (y, row) in glyphs[*glyph].iter().enumerate() {
            for x in 0..5 {
                if row & (1 << (4 - x)) != 0 {
                    for dy in 0..12 {
                        for dx in 0..12 {
                            raster.put_pixel(
                                25 + i as u32 * 92 + x * 12 + dx,
                                25 + y as u32 * 12 + dy,
                                image::Luma([0]),
                            );
                        }
                    }
                }
            }
        }
    }
    let dynamic = image::DynamicImage::ImageLuma8(raster.clone());
    for (name, format) in [
        ("attachment.png", image::ImageFormat::Png),
        ("attachment.jpg", image::ImageFormat::Jpeg),
    ] {
        let mut bytes = Cursor::new(vec![]);
        dynamic.write_to(&mut bytes, format)?;
        let bytes = bytes.into_inner();
        fs::write(fixtures.join(name), &bytes)?;
        let doc = extractor.extract(Path::new(name), &bytes)?;
        let payload = ImageExtractor::extract_payload(&bytes, name)?;
        check(
            &mut checks,
            &format!("ocr_{name}"),
            doc.content.contains("7500000")
                || payload
                    .ocr_text
                    .as_ref()
                    .is_some_and(|s| s.contains("7500000")),
            json!({"ocr_text": payload.ocr_text, "caption": payload.caption, "generic_content": doc.content}),
        );
    }
    let pixels = raster.into_raw();
    let mut image_object = format!("<< /Type /XObject /Subtype /Image /Width 720 /Height 140 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n", pixels.len()).into_bytes();
    image_object.extend_from_slice(&pixels);
    image_object.extend_from_slice(b"\nendstream");
    let scan = pdf(vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 792 612] /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream(b"q 720 0 0 140 30 400 cm /Im0 Do Q"), image_object,
    ]);
    fs::write(fixtures.join("scanned.pdf"), &scan)?;
    let doc = extractor.extract(Path::new("scanned.pdf"), &scan)?;
    check(
        &mut checks,
        "scanned_pdf_ocr",
        doc.content.contains("7500000"),
        json!(doc.content),
    );

    let contract = "CAPÍTULO I. OBLIGACIONES\nCLÁUSULA PRIMERA. PAGO\nPago de 7500000 COP dentro de 30 días.\nCLÁUSULA SEGUNDA. EXCEPCIONES\nNo hay penalidad por fuerza mayor.";
    fs::write(fixtures.join("contract.txt"), contract)?;
    let chunks = documents::legal_chunker::LegalHierarchicalChunker::default()
        .chunk_document("synthetic-contract", contract);
    check(
        &mut checks,
        "legal_chunker_explicit",
        chunks
            .iter()
            .any(|c| c.clause_number == Some(1) && c.text.contains("7500000"))
            && chunks
                .iter()
                .any(|c| c.clause_number == Some(2) && c.text.contains("fuerza mayor")),
        json!(chunks),
    );
    let doc = extractor.extract(Path::new("contract.txt"), contract.as_bytes())?;
    check(
        &mut checks,
        "generic_legal_clause_anchors",
        doc.chunks
            .iter()
            .any(|c| c.metadata.get("clause_number").is_some()),
        json!(doc.chunks),
    );
    let report = json!({"scope": "Original Rust document modules only; no HTTP, embeddings or LLM generation", "checks": checks});
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{} checks written to {}",
        checks.len(),
        out.join("report.json").display()
    );
    Ok(())
}
