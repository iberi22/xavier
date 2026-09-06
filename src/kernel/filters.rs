//! Output filters for standard developer toolchains
//!
//! Condenses verbose toolchain output (cargo test, git, grep, compiler logs)
//! into compact, information-dense representations for LLM agents.

use regex::Regex;
use std::sync::LazyLock;

static ANSI_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1B\[[0-9;]*[a-zA-Z]").unwrap()
});

/// Strip ANSI escape sequences from terminal text.
pub fn strip_ansi(input: &str) -> String {
    ANSI_REGEX.replace_all(input, "").to_string()
}

/// Filter cargo build / test / clippy output.
pub fn filter_cargo(raw: &str) -> String {
    let clean = strip_ansi(raw);
    let mut out = Vec::new();
    let mut failures = Vec::new();
    let mut capture_fail = false;
    let mut passed_count = 0;
    let mut failed_count = 0;
    let mut ignored_count = 0;

    for line in clean.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("test ") && trimmed.ends_with("... ok") {
            passed_count += 1;
        } else if trimmed.starts_with("test ") && trimmed.ends_with("... FAILED") {
            failed_count += 1;
            failures.push(trimmed.to_string());
        } else if trimmed.starts_with("test ") && trimmed.ends_with("... ignored") {
            ignored_count += 1;
        } else if trimmed.starts_with("failures:") || trimmed.starts_with("error[E") || trimmed.starts_with("error:") {
            capture_fail = true;
            failures.push(line.to_string());
        } else if capture_fail {
            if trimmed.starts_with("test result:") {
                capture_fail = false;
                out.push(line.to_string());
            } else if failures.len() < 100 {
                failures.push(line.to_string());
            }
        } else if trimmed.starts_with("test result:") || trimmed.starts_with("Finished") {
            out.push(line.to_string());
        }
    }

    if !failures.is_empty() {
        let mut res = String::new();
        res.push_str("=== FAILURES & ERRORS ===\n");
        for f in failures.iter().take(80) {
            res.push_str(f);
            res.push('\n');
        }
        if failures.len() > 80 {
            res.push_str(&format!("... and {} more error lines truncated\n", failures.len() - 80));
        }
        res.push_str("\n=== SUMMARY ===\n");
        res.push_str(&format!("passed: {}, failed: {}, ignored: {}\n", passed_count, failed_count, ignored_count));
        for o in out {
            res.push_str(&o);
            res.push('\n');
        }
        res
    } else if passed_count > 0 {
        let mut res = format!("cargo: all {} tests passed (ignored: {})\n", passed_count, ignored_count);
        for o in out {
            res.push_str(&o);
            res.push('\n');
        }
        res
    } else if !out.is_empty() {
        let mut res = String::new();
        for o in out {
            res.push_str(&o);
            res.push('\n');
        }
        res
    } else {
        // General cargo build/check: take first 40 lines or relevant errors
        let lines: Vec<&str> = clean.lines().collect();
        if lines.len() > 50 {
            let mut compact = lines.iter().take(30).copied().collect::<Vec<_>>().join("\n");
            compact.push_str(&format!("\n... [{} lines truncated for token economy]", lines.len() - 30));
            compact
        } else {
            clean
        }
    }
}

/// Filter git status and diff.
pub fn filter_git(raw: &str) -> String {
    let clean = strip_ansi(raw);
    let mut out = Vec::new();
    let lines: Vec<&str> = clean.lines().collect();

    for line in &lines {
        let trimmed = line.trim();
        // Skip git verbose index/metadata comments and advice (multilingual: use/usa)
        if trimmed.starts_with("(use \"git ")
            || trimmed.starts_with("(usa \"git ")
            || trimmed.contains("(use \"git ")
            || trimmed.contains("(usa \"git ")
            || trimmed.starts_with("no changes added to commit")
            || trimmed.starts_with("sin cambios agregados al commit")
            || trimmed.starts_with("index ")
            || trimmed.starts_with("--- a/")
            || trimmed.starts_with("+++ b/")
        {
            continue;
        }
        out.push(*line);
    }

    if out.len() > 100 {
        let mut res = out.iter().take(70).copied().collect::<Vec<_>>().join("\n");
        res.push_str(&format!("\n... [{} lines condensed for context limit]", out.len() - 70));
        res
    } else {
        out.join("\n")
    }
}

/// Filter grep / ripgrep outputs to avoid bloating context.
pub fn filter_grep(raw: &str) -> String {
    let clean = strip_ansi(raw);
    let lines: Vec<&str> = clean.lines().collect();
    if lines.len() > 60 {
        let mut res = lines.iter().take(50).copied().collect::<Vec<_>>().join("\n");
        res.push_str(&format!("\n... [{} matches truncated. Use specific search to refine]", lines.len() - 50));
        res
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_cargo_passed() {
        let raw = "\
test test_one ... ok
test test_two ... ok
test test_three ... ok
test result: ok. 3 passed; 0 failed; 0 ignored
";
        let filtered = filter_cargo(raw);
        assert!(filtered.contains("all 3 tests passed"));
        assert!(!filtered.contains("test_one ... ok"));
    }

    #[test]
    fn test_filter_cargo_failed() {
        let raw = "\
test test_pass ... ok
test test_bad ... FAILED
failures:
---- test_bad stdout ----
assertion failed: false == true
test result: FAILED. 1 failed; 1 passed
";
        let filtered = filter_cargo(raw);
        assert!(filtered.contains("=== FAILURES & ERRORS ==="));
        assert!(filtered.contains("test_bad"));
        assert!(filtered.contains("assertion failed"));
    }

    #[test]
    fn test_filter_git_noise() {
        let raw = "\
On branch main
  (use \"git restore <file>...\" to discard changes in working directory)
	modified:   src/main.rs
";
        let filtered = filter_git(raw);
        assert!(!filtered.contains("use \"git restore"));
        assert!(filtered.contains("modified:   src/main.rs"));
    }
}
