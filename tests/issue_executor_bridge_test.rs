//! Integration tests for Issue Context Packager Executor Bridge (Issue #1435).

use xavier::codebase::executor_bridge::{
    apply_precise_change, apply_precise_changes, calculate_token_savings,
};
use xavier::codebase::snapshot::PreciseChange;

#[test]
fn test_line_exact_replacement() {
    let source = "line 1\nfn old_function() {\n    println!(\"old\");\n}\nline 5\n";
    let change = PreciseChange {
        repo: "iberi22/xavier".to_string(),
        file: "src/main.rs".to_string(),
        symbol: "old_function".to_string(),
        start_line: 2,
        end_line: 4,
        before_snippet: "fn old_function() {\n    println!(\"old\");\n}".to_string(),
        after_snippet: "fn new_function() {\n    println!(\"new\");\n}".to_string(),
    };

    let result = apply_precise_change(source, &change).unwrap();
    assert_eq!(
        result,
        "line 1\nfn new_function() {\n    println!(\"new\");\n}\nline 5\n"
    );
}

#[test]
fn test_substring_fallback_replacement() {
    let source = "// header\n\nconst MAGIC: u32 = 42;\n\n// footer";
    let change = PreciseChange {
        repo: "iberi22/xavier".to_string(),
        file: "src/constants.rs".to_string(),
        symbol: "MAGIC".to_string(),
        start_line: 999, // Intentional line drift
        end_line: 1000,
        before_snippet: "const MAGIC: u32 = 42;".to_string(),
        after_snippet: "const MAGIC: u32 = 1337;".to_string(),
    };

    let result = apply_precise_change(source, &change).unwrap();
    assert_eq!(result, "// header\n\nconst MAGIC: u32 = 1337;\n\n// footer");
}

#[test]
fn test_pure_insertion() {
    let source = "line 1\nline 2\n";
    let change = PreciseChange {
        repo: "iberi22/xavier".to_string(),
        file: "src/lib.rs".to_string(),
        symbol: "inserted_item".to_string(),
        start_line: 2,
        end_line: 2,
        before_snippet: "".to_string(),
        after_snippet: "// inserted comment".to_string(),
    };

    let result = apply_precise_change(source, &change).unwrap();
    assert_eq!(result, "line 1\n// inserted comment\nline 2\n");
}

#[test]
fn test_pure_deletion() {
    let source = "line 1\n// delete me\nline 3\n";
    let change = PreciseChange {
        repo: "iberi22/xavier".to_string(),
        file: "src/lib.rs".to_string(),
        symbol: "delete_comment".to_string(),
        start_line: 2,
        end_line: 2,
        before_snippet: "// delete me".to_string(),
        after_snippet: "".to_string(),
    };

    let result = apply_precise_change(source, &change).unwrap();
    assert_eq!(result, "line 1\nline 3\n");
}

#[test]
fn test_multiple_sequential_changes() {
    let source = "let a = 1;\nlet b = 2;\n";
    let changes = vec![
        PreciseChange {
            repo: "iberi22/xavier".to_string(),
            file: "src/lib.rs".to_string(),
            symbol: "a".to_string(),
            start_line: 1,
            end_line: 1,
            before_snippet: "let a = 1;".to_string(),
            after_snippet: "let a = 10;".to_string(),
        },
        PreciseChange {
            repo: "iberi22/xavier".to_string(),
            file: "src/lib.rs".to_string(),
            symbol: "b".to_string(),
            start_line: 2,
            end_line: 2,
            before_snippet: "let b = 2;".to_string(),
            after_snippet: "let b = 20;".to_string(),
        },
    ];

    let result = apply_precise_changes(source, &changes).unwrap();
    assert_eq!(result, "let a = 10;\nlet b = 20;\n");
}

#[test]
fn test_mismatch_error() {
    let source = "fn target() { 1 }";
    let change = PreciseChange {
        repo: "iberi22/xavier".to_string(),
        file: "src/lib.rs".to_string(),
        symbol: "nonexistent".to_string(),
        start_line: 1,
        end_line: 1,
        before_snippet: "fn completely_different() { 2 }".to_string(),
        after_snippet: "fn replacement() { 3 }".to_string(),
    };

    let err = apply_precise_change(source, &change);
    assert!(err.is_err());
    let err_msg = err.unwrap_err().to_string();
    assert!(err_msg.contains("PreciseChange mismatch"));
}

#[test]
fn test_token_savings_benchmark() {
    let full_content = "/* 1000 lines of complex codebase file */\n".repeat(50);
    let change = PreciseChange {
        repo: "iberi22/xavier".to_string(),
        file: "src/big_file.rs".to_string(),
        symbol: "small_patch".to_string(),
        start_line: 10,
        end_line: 12,
        before_snippet: "let x = 1;".to_string(),
        after_snippet: "let x = 2;".to_string(),
    };

    let report = calculate_token_savings(&full_content, &[change]);
    assert!(report.full_file_tokens > 100);
    assert!(report.tokens_saved > 0);
    assert!(report.savings_percentage > 90.0);
}
