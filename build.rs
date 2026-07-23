use std::path::Path;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(has_classifier_module)");
    if Path::new("src/memory/qmd/search/classifier.rs").exists() {
        println!("cargo:rustc-cfg=has_classifier_module");
    }
}
