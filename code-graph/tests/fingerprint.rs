//! Integration tests for `code-graph fingerprint` (ksm-sfp/1).

use code_graph::fingerprint::{
    fingerprint_source, kgram_key, parse_ranges, resolve_language, to_json_sorted,
};
use code_graph::types::Language;
use serde_json::Value;

fn fixture(name: &str) -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    std::fs::read_to_string(format!("{dir}/tests/fixtures/fingerprint/{name}")).unwrap()
}

fn is_hex8(s: &str) -> bool {
    s.len() == 8
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn var_lines(fp: &Value, func: &str, var: &str) -> Vec<u64> {
    fp["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == func)
        .unwrap()["vars"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == var)
        .unwrap()["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_u64().unwrap())
        .collect()
}

#[test]
fn c_function_shape() {
    let src = fixture("sample.c");
    let fp = fingerprint_source(&src, Language::C, "sample.c", &[]);
    let json = serde_json::to_value(&fp).unwrap();
    assert_eq!(fp.schema, "ksm-sfp/1");
    assert_eq!(fp.coverage, "OK");
    assert_eq!(fp.language, "c");
    assert_eq!(fp.functions.len(), 1);
    let f = &fp.functions[0];
    assert_eq!(f.name, "copy_checked");
    assert_eq!((f.start_line, f.end_line), (3, 7));
    assert!(f.node_count > 10);

    // calls: callee, args, line.
    assert_eq!(f.calls.len(), 1);
    assert_eq!(f.calls[0].callee, "memcpy");
    assert_eq!(f.calls[0].line, 5);
    assert_eq!(f.calls[0].args, vec!["dst", "src", "len"]);

    // guards: kind, vars, line.
    let j = &json["functions"][0];
    assert_eq!(j["guards"].as_array().unwrap().len(), 1);
    assert_eq!(j["guards"][0]["kind"], "if");
    assert_eq!(j["guards"][0]["line"], 4);
    let vars: Vec<&str> = j["guards"][0]["vars"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(vars, vec!["len", "max"]);

    // vars: lines per occurrence; callee names excluded.
    assert_eq!(var_lines(&json, "copy_checked", "dst"), vec![3, 5]);
    assert_eq!(var_lines(&json, "copy_checked", "len"), vec![3, 4, 5]);
    assert!(j["vars"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["name"] != "memcpy"));
}

#[test]
fn rust_if_let_guard() {
    let src = fixture("sample.rs");
    let fp = fingerprint_source(&src, Language::Rust, "sample.rs", &[]);
    assert_eq!(fp.coverage, "OK");
    assert_eq!(fp.functions.len(), 1);
    let f = &fp.functions[0];
    assert_eq!(f.name, "load");
    assert_eq!((f.start_line, f.end_line), (1, 7));
    assert_eq!(f.guards.len(), 1);
    assert_eq!(f.guards[0].kind, "if");
    assert_eq!(f.guards[0].line, 2);
    assert_eq!(f.guards[0].vars, vec!["Some", "data", "path", "read_file"]);
    assert!(f
        .calls
        .iter()
        .any(|c| c.callee == "read_file" && c.line == 2 && c.args == vec!["path"]));
}

#[test]
fn python_def_with_ternary() {
    let src = fixture("sample.py");
    let fp = fingerprint_source(&src, Language::Python, "sample.py", &[]);
    assert_eq!(fp.coverage, "OK");
    assert_eq!(fp.functions.len(), 1);
    let f = &fp.functions[0];
    assert_eq!(f.name, "fetch");
    assert_eq!((f.start_line, f.end_line), (1, 5));
    assert_eq!(f.calls.len(), 1);
    assert_eq!(f.calls[0].callee, "get");
    assert_eq!(f.calls[0].args, vec!["url"]);
    let kinds: Vec<&str> = f.guards.iter().map(|g| g.kind.as_str()).collect();
    assert_eq!(kinds, vec!["if", "ternary"]);
    assert_eq!(f.guards[0].vars, vec!["timeout"]);
    assert_eq!(f.guards[1].line, 5);
    assert_eq!(f.guards[1].vars, vec!["data"]);
}

#[test]
fn kgram_keys_are_hex8_and_minhash_is_64() {
    for (name, lang) in [
        ("sample.c", Language::C),
        ("sample.rs", Language::Rust),
        ("sample.py", Language::Python),
    ] {
        let src = fixture(name);
        let fp = fingerprint_source(&src, lang, name, &[]);
        for f in &fp.functions {
            assert!(!f.kgrams.is_empty(), "{name}: expected kgrams");
            for key in f.kgrams.keys() {
                assert!(is_hex8(key), "{name}: bad kgram key {key:?}");
            }
            assert_eq!(f.minhash.len(), 64, "{name}: minhash length");
        }
        // Same via the JSON value (u32 range).
        let json = serde_json::to_value(&fp).unwrap();
        for m in json["functions"].as_array().unwrap() {
            assert_eq!(m["minhash"].as_array().unwrap().len(), 64);
            for v in m["minhash"].as_array().unwrap() {
                assert!(v.as_u64().unwrap() <= 4_294_967_295);
            }
        }
    }
}

#[test]
fn output_is_byte_stable() {
    let src = fixture("sample.c");
    let a = to_json_sorted(&fingerprint_source(&src, Language::C, "sample.c", &[]));
    let b = to_json_sorted(&fingerprint_source(&src, Language::C, "sample.c", &[]));
    assert_eq!(a, b);
    // Top-level keys sorted.
    let order = [
        "coverage",
        "file",
        "functions",
        "language",
        "schema",
        "tool",
    ];
    let mut pos = 0;
    for key in order {
        let needle = format!("\"{key}\"");
        let found = a[pos..].find(&needle).unwrap();
        pos += found + needle.len();
    }
}

#[test]
fn ranges_filter_functions() {
    let src = fixture("sample.c");
    let all = fingerprint_source(&src, Language::C, "sample.c", &[]);
    assert_eq!(all.functions.len(), 1);
    let outside = fingerprint_source(&src, Language::C, "sample.c", &[(1, 2)]);
    assert!(outside.functions.is_empty());
    assert_eq!(outside.coverage, "OK");
    let overlap = fingerprint_source(&src, Language::C, "sample.c", &[(4, 4)]);
    assert_eq!(overlap.functions.len(), 1);
    assert_eq!(parse_ranges("10-50,80-120"), Ok(vec![(10, 50), (80, 120)]));
    assert_eq!(resolve_language("c"), Language::C);
    assert_eq!(resolve_language("javascript"), Language::JavaScript);
}

#[test]
fn unknown_language_is_skipped() {
    let src = fixture("sample.zzz");
    let fp = fingerprint_source(&src, Language::Unknown, "sample.zzz", &[]);
    assert_eq!(fp.coverage, "PARSER_SKIPPED");
    assert!(fp.functions.is_empty());
    let fp = fingerprint_source("fn x() {}", Language::Other("ruby".into()), "x.rb", &[]);
    assert_eq!(fp.coverage, "PARSER_SKIPPED");
    assert!(fp.functions.is_empty());
}

#[test]
fn golden_kgram_key_pins_hash_rule() {
    // Contract rule: first 8 hex of sha256 of tokens joined by U+001F.
    assert_eq!(kgram_key(&["call:memcpy", "id", "num"]), "5306d8d4");
    let src = fixture("sample.c");
    let fp = fingerprint_source(&src, Language::C, "sample.c", &[]);
    let f = &fp.functions[0];
    assert!(
        f.kgrams.keys().all(|k| is_hex8(k)),
        "every emitted key follows the pinned rule"
    );
}

fn bin() -> String {
    env!("CARGO_BIN_EXE_code-graph").to_string()
}

#[test]
fn cli_fingerprint_prints_json_and_skips_unknown() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let c_file = format!("{dir}/tests/fixtures/fingerprint/sample.c");
    let out = std::process::Command::new(bin())
        .args(["fingerprint", &c_file])
        .output()
        .unwrap();
    assert!(out.status.success());
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["coverage"], "OK");
    assert_eq!(doc["language"], "c");
    assert_eq!(doc["file"], Value::String(c_file.clone()));
    assert_eq!(doc["functions"][0]["name"], "copy_checked");

    // --ranges filtering through the CLI.
    let out = std::process::Command::new(bin())
        .args(["fingerprint", &c_file, "--ranges", "1-2"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["functions"].as_array().unwrap().len(), 0);

    // Unknown extension: exit 0 with PARSER_SKIPPED.
    let z_file = format!("{dir}/tests/fixtures/fingerprint/sample.zzz");
    let out = std::process::Command::new(bin())
        .args(["fingerprint", &z_file])
        .output()
        .unwrap();
    assert!(out.status.success());
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["coverage"], "PARSER_SKIPPED");
    assert_eq!(doc["functions"].as_array().unwrap().len(), 0);

    // Unreadable file: exit 2.
    let out = std::process::Command::new(bin())
        .args(["fingerprint", "/nonexistent-dir-xyz/nope.c"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn cli_fingerprint_creates_no_database() {
    let tmp = std::env::temp_dir().join(format!("cg-fp-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let dir = env!("CARGO_MANIFEST_DIR");
    let c_file = format!("{dir}/tests/fixtures/fingerprint/sample.c");
    let before: Vec<String> = std::fs::read_dir(&tmp)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let out = std::process::Command::new(bin())
        .args(["fingerprint", &c_file])
        .current_dir(&tmp)
        .output()
        .unwrap();
    assert!(out.status.success());
    let after: Vec<String> = std::fs::read_dir(&tmp)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(before, after, "fingerprint must not create files");
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn c_preprocessor_split_else_if_keeps_function_whole() {
    let fp = fingerprint_source(&fixture("preproc.c"), Language::C, "preproc.c", &[]);
    let v: Value = serde_json::from_str(&to_json_sorted(&fp)).unwrap();
    let names: Vec<(&str, u64, u64)> = v["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["name"].as_str().unwrap(),
                f["start_line"].as_u64().unwrap(),
                f["end_line"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(names, vec![("pick", 4, 23), ("after", 25, 28)]);
    let pick = &v["functions"][0];
    let callees: Vec<&str> = pick["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["callee"].as_str().unwrap())
        .collect();
    // `#if 0` takes the #else branch; the first branch of other conditionals is kept.
    assert_eq!(callees, vec!["memset"]);
    assert_eq!(pick["calls"][0]["line"].as_u64().unwrap(), 20);
}

#[test]
fn c_preprocessor_zero_conditions_with_comments_parens_and_elif() {
    let fp = fingerprint_source(
        &fixture("preproc_zero.c"),
        Language::C,
        "preproc_zero.c",
        &[],
    );
    let names: Vec<String> = fp.functions.iter().map(|f| f.name.clone()).collect();
    assert_eq!(names, vec!["live_a".to_string(), "live_b".to_string()]);
}
