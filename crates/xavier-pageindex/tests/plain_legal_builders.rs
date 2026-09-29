use serde_json::{json, Value};
use xavier_pageindex::builders::{legal, plain};
use xavier_pageindex::model::TreeNode;

fn simplify(nodes: &[TreeNode]) -> Value {
    Value::Array(
        nodes
            .iter()
            .map(|n| json!({ "title": n.title, "children": simplify(&n.children) }))
            .collect(),
    )
}

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/legal/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn assert_golden(text_file: &str, golden_file: &str) {
    let built = legal::build("doc", &fixture(text_file), 60).unwrap();
    let golden: Value = serde_json::from_str(&fixture(golden_file)).unwrap();
    assert_eq!(simplify(&built.tree.roots), golden);
    built.tree.validate(built.pages.len() as u32).unwrap();
}

#[test]
fn test_plain_numbered_headings_detected() {
    let text = "Intro line\n\n1. Introduction\nBody one.\n1.1 Scope\nMore text.\n1.2 Goals\nText.\n2. Methods\nDetails.\n\nCHAPTER IV\nEnd.\n";
    let built = plain::build("d", text, 60).unwrap();
    let titles: Vec<_> = built.tree.roots.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Preamble", "1. Introduction", "2. Methods", "CHAPTER IV"]
    );
    let intro = &built.tree.roots[1];
    let sub: Vec<_> = intro.children.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(sub, ["1.1 Scope", "1.2 Goals"]);
    assert_eq!(built.builder, "plain-headings");
    built.tree.validate(built.pages.len() as u32).unwrap();
}

#[test]
fn test_plain_caps_line_and_prose_not_confused() {
    let text =
        "OVERVIEW\nThis is a sentence. It has 3 items.\n2024 was a good year.\n1. Buy milk.\n";
    let built = plain::build("d", text, 60).unwrap();
    let titles: Vec<_> = built.tree.roots.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(titles, ["OVERVIEW"]);
}

#[test]
fn test_plain_no_headings_falls_back_to_fixed_windows() {
    let text: String = (1..=25)
        .map(|i| format!("line {i} of prose text\n"))
        .collect();
    let built = plain::build("d", &text, 10).unwrap();
    assert_eq!(built.builder, "plain-windows");
    assert_eq!(built.pages.len(), 3);
    let titles: Vec<_> = built.tree.roots.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(titles, ["Lines 1-10", "Lines 11-20", "Lines 21-25"]);
    assert_eq!(built.tree.roots[2].start_page, 3);
    built.tree.validate(3).unwrap();
}

#[test]
fn test_plain_empty_text_is_an_error() {
    assert!(plain::build("d", "  \n\n", 60).is_err());
}

#[test]
fn test_legal_articles_nest_under_chapters() {
    let built = legal::build("d", &fixture("contract_es.txt"), 60).unwrap();
    let chapter = &built.tree.roots[1];
    assert!(chapter.title.starts_with("CAPÍTULO I"));
    assert_eq!(chapter.children.len(), 2);
    assert!(chapter.children[1].children[0]
        .title
        .starts_with("PARÁGRAFO"));
    assert_eq!(built.builder, "legal");
    // Page ranges nest; siblings may share boundary pages.
    built.tree.validate(built.pages.len() as u32).unwrap();
    let a1 = &chapter.children[0];
    assert!(a1.start_page >= chapter.start_page && a1.end_page <= chapter.end_page);
}

#[test]
fn test_legal_spanish_and_english_headings() {
    assert_golden("contract_es.txt", "contract_es.golden.json");
    assert_golden("contract_en.txt", "contract_en.golden.json");
}

#[test]
fn test_legal_ocr_spaced_headings_and_no_headings_fallback() {
    let built = legal::build("d", "C A P Í T U L O 1\nA R T I C L E 2\ntext\n", 60).unwrap();
    assert_eq!(built.tree.roots[0].title, "CAPÍTULO 1");
    assert_eq!(built.tree.roots[0].children[0].title, "ARTICLE 2");
    let flat = legal::build("d", "just some prose\nwithout structure\n", 60).unwrap();
    assert_eq!(flat.builder, "plain-windows");
}

#[test]
fn test_plain_and_legal_headings_share_virtual_pages() {
    let plain_text = "1. One\nbody\n2. Two\nbody\n3. Three\nbody\n";
    let built = plain::build("d", plain_text, 60).unwrap();
    assert_eq!(built.pages.len(), 1);
    assert_eq!(built.tree.roots.len(), 3);
    assert!(built.tree.roots.iter().all(|n| n.start_page == 1));
    built.tree.validate(1).unwrap();

    let legal_text = "ARTICLE 1\nx\nARTICLE 2\ny\nARTICLE 3\nz\n";
    let built = legal::build("d", legal_text, 60).unwrap();
    assert_eq!(built.pages.len(), 1);
    assert_eq!(built.tree.roots.len(), 3);
    assert!(built.tree.roots.iter().all(|n| n.end_page == 1));
    built.tree.validate(1).unwrap();
}

#[test]
fn test_legal_keyword_prefixes_are_not_headings() {
    let text = "Title to the goods passes on delivery.\nArticles 3 and 4 apply.\n\
                Clauses 5-7 are void.\nChaptered sections follow.\n";
    let built = legal::build("d", text, 60).unwrap();
    assert_eq!(built.builder, "plain-windows");
    let ok = "Title II\nArt.3 x\nCLÁUSULA PRIMERA: OBJETO\nPARÁGRAFO ÚNICO\n";
    let built = legal::build("d", ok, 60).unwrap();
    assert_eq!(built.builder, "legal");
}
