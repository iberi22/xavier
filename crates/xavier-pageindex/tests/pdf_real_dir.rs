//! Manual smoke over real PDFs: `XAVIER_PAGEINDEX_PDF_DIR=<dir> cargo test --test pdf_real_dir -- --ignored --nocapture`.
//! The PDFs are never committed.

#[cfg(feature = "pdf-outline")]
#[test]
#[ignore = "reads PDFs from XAVIER_PAGEINDEX_PDF_DIR"]
fn real_pdfs_produce_trees() {
    use xavier_pageindex::pdf::cascade::build_pdf_tree;

    let Ok(dir) = std::env::var("XAVIER_PAGEINDEX_PDF_DIR") else {
        panic!("set XAVIER_PAGEINDEX_PDF_DIR");
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
                let mut total = 0;
                let mut zero_tok = 0;
                let mut max_span = 0;
                fn walk(
                    ns: &[xavier_pageindex::TreeNode],
                    t: &mut usize,
                    z: &mut usize,
                    m: &mut u32,
                ) {
                    for n in ns {
                        *t += 1;
                        if n.token_estimate == 0 {
                            *z += 1;
                        }
                        if n.children.is_empty() {
                            *m = (*m).max(n.end_page - n.start_page + 1);
                        }
                        walk(&n.children, t, z, m);
                    }
                }
                walk(&b.tree.roots, &mut total, &mut zero_tok, &mut max_span);
                b.tree.validate(n).unwrap();
                println!(
                    "{name}: pages={n} builder={} roots={} nodes={total} zero_tokens={zero_tok} max_leaf_span={max_span}",
                    b.builder(),
                    b.tree.roots.len()
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
