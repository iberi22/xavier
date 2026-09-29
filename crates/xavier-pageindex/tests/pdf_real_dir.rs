//! Manual smoke over real PDFs: `XAVIER_PAGEINDEX_PDF_DIR=<dir> cargo test --test pdf_real_dir -- --ignored --nocapture`.
//! The PDFs are never committed.

#[cfg(feature = "pdf-outline")]
#[derive(Default)]
struct Quality {
    nodes: usize,
    depth: usize,
    sentence: usize,
    bare_numbers: usize,
    dup_titles: usize,
    max_span: u32,
}

/// Tree quality counters: sentence-like titles, bare numbers, repeated titles.
#[cfg(feature = "pdf-outline")]
fn quality(roots: &[xavier_pageindex::TreeNode]) -> Quality {
    fn walk(
        ns: &[xavier_pageindex::TreeNode],
        d: usize,
        q: &mut Quality,
        seen: &mut std::collections::HashMap<String, usize>,
    ) {
        for n in ns {
            q.nodes += 1;
            q.depth = q.depth.max(d);
            let t = n.title.trim();
            let words = t.split_whitespace().count();
            if t.ends_with('.') || words > 12 || t.chars().next().is_some_and(char::is_lowercase) {
                q.sentence += 1;
            }
            if t.chars().all(|c| c.is_ascii_digit() || c.is_whitespace()) {
                q.bare_numbers += 1;
            }
            *seen.entry(t.to_lowercase()).or_default() += 1;
            if n.children.is_empty() {
                q.max_span = q.max_span.max(n.end_page - n.start_page + 1);
            }
            walk(&n.children, d + 1, q, seen);
        }
    }
    let mut q = Quality::default();
    let mut seen = std::collections::HashMap::new();
    walk(roots, 1, &mut q, &mut seen);
    q.dup_titles = seen.values().filter(|&&c| c > 2).map(|c| c - 1).sum();
    q
}

#[cfg(feature = "pdf-outline")]
#[test]
#[ignore = "reads PDFs from XAVIER_PAGEINDEX_PDF_DIR"]
fn real_pdfs_produce_trees() {
    use xavier_pageindex::pdf::cascade::build_pdf_tree;

    let Ok(dir) = std::env::var("XAVIER_PAGEINDEX_PDF_DIR") else {
        eprintln!("XAVIER_PAGEINDEX_PDF_DIR not set; skipping");
        return;
    };
    let only: Vec<String> = std::env::var("XAVIER_PAGEINDEX_PDF_ONLY")
        .map(|v| v.split(',').map(str::to_string).collect())
        .unwrap_or_default();
    let mut failed = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "pdf"))
        .collect();
    names.sort();
    for path in names {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !only.is_empty() && !only.contains(&name) {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        match build_pdf_tree("d", &bytes, None) {
            Ok(b) => {
                let n = b.pages.len() as u32;
                b.tree.validate(n).unwrap();
                let q = quality(&b.tree.roots);
                let empty_pages = b
                    .pages
                    .iter()
                    .filter(|p| p.text.split_whitespace().count() == 0)
                    .count();
                if std::env::var("XAVIER_PAGEINDEX_DUMP").is_ok() {
                    fn dump(ns: &[xavier_pageindex::TreeNode], d: usize) {
                        for n in ns {
                            println!(
                                "  {}{} [{}-{}]",
                                "  ".repeat(d),
                                n.title,
                                n.start_page,
                                n.end_page
                            );
                            dump(&n.children, d + 1);
                        }
                    }
                    dump(&b.tree.roots, 0);
                }
                println!(
                    "QR {name}: pages={n} builder={} roots={} nodes={} depth={} sentence%={:.0} bare_num={} dup_titles={} max_leaf_span={} empty_pages={empty_pages}",
                    b.builder(),
                    b.tree.roots.len(),
                    q.nodes,
                    q.depth,
                    if q.nodes == 0 { 0.0 } else { 100.0 * q.sentence as f64 / q.nodes as f64 },
                    q.bare_numbers,
                    q.dup_titles,
                    q.max_span,
                );
            }
            Err(e) => {
                println!("{name}: ERROR {e}");
                failed.push(name);
            }
        }
    }
    assert!(failed.is_empty(), "failed: {failed:?}");
}
