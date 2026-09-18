//! O7 edit-check vs HEAD as a testable library module.
//!
//! Provides the [`HeadProvider`] trait seam for accessing git HEAD state without
//! coupling to git2 or subprocess calls directly, pure `edit_check` delegation
//! to [`crate::query::compare_contract`], and working tree orchestration via
//! [`check_working_tree`].

use crate::error::Result;
use crate::parser::parse_native;
use crate::query::{compare_contract, ContractStatus};
use crate::types::{Language, Symbol};

/// Trait seam for retrieving file content at git HEAD (or a baseline commit).
pub trait HeadProvider {
    /// Retrieve the content of a file at HEAD.
    ///
    /// - `Ok(Some(content))` when the file exists at HEAD.
    /// - `Ok(None)` when the file is new / untracked / absent at HEAD.
    /// - `Err(GraphError)` when HEAD state is unavailable (e.g., git query failed, not a git repo).
    fn file_at_head(&self, path: &str) -> Result<Option<String>>;
}

/// A working tree file to compare against HEAD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingFile {
    pub file_path: String,
    pub content: String,
    pub language: Language,
}

impl WorkingFile {
    pub fn new(
        file_path: impl Into<String>,
        content: impl Into<String>,
        language: Language,
    ) -> Self {
        Self {
            file_path: file_path.into(),
            content: content.into(),
            language,
        }
    }
}

/// A detected breaking contract change for a symbol between HEAD and current content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractBreak {
    pub symbol_name: String,
    pub file_path: String,
    pub was_signature: Option<String>,
    pub now_signature: Option<String>,
    pub status: ContractStatus,
}

/// Pure edit-check function comparing old source text vs new source text.
///
/// Parses symbols using tree-sitter native parsers and delegates comparison
/// directly to [`crate::query::compare_contract`] to prevent algorithm duplication.
pub fn edit_check(
    old_src: &str,
    new_src: &str,
    lang: &Language,
    file_path: &str,
) -> Result<Vec<ContractBreak>> {
    let old_syms = parse_native(old_src, lang, file_path)?;
    let new_syms = parse_native(new_src, lang, file_path)?;

    let mut breaks = Vec::new();

    for old_sym in &old_syms {
        if let Some(new_sym) = new_syms
            .iter()
            .find(|s| s.name == old_sym.name && s.kind == old_sym.kind)
        {
            let status = compare_contract(old_sym, new_sym);
            if let ContractStatus::ContractChange { ref was, ref now } = status {
                breaks.push(ContractBreak {
                    symbol_name: old_sym.name.clone(),
                    file_path: file_path.to_string(),
                    was_signature: was.clone(),
                    now_signature: now.clone(),
                    status,
                });
            }
        }
    }

    Ok(breaks)
}

/// Convenience edit-check helper over Rust source code strings.
pub fn edit_check_rust(old_src: &str, new_src: &str) -> Result<Vec<ContractBreak>> {
    edit_check(old_src, new_src, &Language::Rust, "lib.rs")
}

/// Compare direct symbol slices using [`crate::query::compare_contract`].
pub fn compare_symbol_contract(old_sym: &Symbol, new_sym: &Symbol) -> ContractStatus {
    compare_contract(old_sym, new_sym)
}

/// Orchestrate edit checks across working tree files using a [`HeadProvider`].
///
/// If HEAD state is unavailable for any file or provider fails, returns an [`Err`]
/// for honest refusal (O1 honesty: never silent `Ok(vec![])` on missing HEAD state).
pub fn check_working_tree<P: HeadProvider>(
    provider: &P,
    files: &[WorkingFile],
) -> Result<Vec<ContractBreak>> {
    let mut all_breaks = Vec::new();

    for wf in files {
        let head_content = provider.file_at_head(&wf.file_path)?;
        match head_content {
            Some(old_src) => {
                let file_breaks = edit_check(&old_src, &wf.content, &wf.language, &wf.file_path)?;
                all_breaks.extend(file_breaks);
            }
            None => {
                // New file absent at HEAD - no contract breaks against HEAD.
            }
        }
    }

    Ok(all_breaks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::GraphError;
    use std::collections::HashMap;

    /// In-memory fake implementation of [`HeadProvider`] for testing.
    pub struct FakeHeadProvider {
        pub files: HashMap<String, Option<String>>,
        pub should_fail: bool,
    }

    impl FakeHeadProvider {
        pub fn new() -> Self {
            Self {
                files: HashMap::new(),
                should_fail: false,
            }
        }

        pub fn with_file(mut self, path: impl Into<String>, content: impl Into<String>) -> Self {
            self.files.insert(path.into(), Some(content.into()));
            self
        }

        pub fn with_failure(mut self) -> Self {
            self.should_fail = true;
            self
        }
    }

    impl HeadProvider for FakeHeadProvider {
        fn file_at_head(&self, path: &str) -> Result<Option<String>> {
            if self.should_fail {
                return Err(GraphError::Query(
                    "HEAD state unavailable: git repository not found or HEAD unreadable".into(),
                ));
            }
            match self.files.get(path) {
                Some(Some(content)) => Ok(Some(content.clone())),
                Some(None) => Ok(None),
                None => Ok(None),
            }
        }
    }

    #[test]
    fn test_edit_check_arity_break() {
        let old_code = "pub fn run(a: i32) -> i32 { a }";
        let broke_code = "pub fn run(a: i32, b: i32) -> i32 { a + b }";

        let provider = FakeHeadProvider::new().with_file("src/main.rs", old_code);
        let files = vec![WorkingFile::new("src/main.rs", broke_code, Language::Rust)];

        let breaks = check_working_tree(&provider, &files).expect("check_working_tree");
        assert_eq!(breaks.len(), 1);
        assert_eq!(breaks[0].symbol_name, "run");
        assert_eq!(breaks[0].file_path, "src/main.rs");
        assert!(matches!(
            breaks[0].status,
            ContractStatus::ContractChange { .. }
        ));
    }

    #[test]
    fn test_clean_edit_no_breaks() {
        let old_code = "pub fn process(data: &str) -> usize {\n    data.len()\n}";
        let same_code = "pub fn process(data: &str) -> usize { data.len() }";

        let provider = FakeHeadProvider::new().with_file("src/lib.rs", old_code);
        let files = vec![WorkingFile::new("src/lib.rs", same_code, Language::Rust)];

        let breaks = check_working_tree(&provider, &files).expect("check_working_tree");
        assert!(
            breaks.is_empty(),
            "formatting changes must yield zero contract breaks"
        );
    }

    #[test]
    fn test_missing_head_honest_refusal() {
        let provider = FakeHeadProvider::new().with_failure();
        let files = vec![WorkingFile::new(
            "src/lib.rs",
            "pub fn x() {}",
            Language::Rust,
        )];

        let res = check_working_tree(&provider, &files);
        assert!(
            res.is_err(),
            "missing/failing HEAD must yield explicit Err refusal, never Ok(vec![]) silence"
        );
        let err = res.unwrap_err();
        assert!(
            err.to_string().contains("HEAD state unavailable"),
            "Error string should indicate HEAD state unavailable: {}",
            err
        );
    }

    #[test]
    fn test_agrees_with_compare_contract() {
        // Cross-check test: verify contracts::edit_check and contracts::compare_symbol_contract
        // strictly agree with query::compare_contract across >= 3 fixture pairs.

        // Fixture 1: Unchanged whitespace / formatting
        let sym_old_1 = Symbol {
            name: "calculate".into(),
            signature: Some("pub fn calculate(x: f64) -> f64".into()),
            ..Default::default()
        };
        let sym_new_1 = Symbol {
            name: "calculate".into(),
            signature: Some("pub fn calculate( x : f64 ) -> f64".into()),
            ..Default::default()
        };
        let query_status_1 = compare_contract(&sym_old_1, &sym_new_1);
        let contracts_status_1 = compare_symbol_contract(&sym_old_1, &sym_new_1);
        assert_eq!(query_status_1, ContractStatus::Unchanged);
        assert_eq!(contracts_status_1, query_status_1);

        // Fixture 2: Arity break (added parameter)
        let sym_old_2 = Symbol {
            name: "fetch_data".into(),
            signature: Some("pub fn fetch_data(id: u64)".into()),
            ..Default::default()
        };
        let sym_new_2 = Symbol {
            name: "fetch_data".into(),
            signature: Some("pub fn fetch_data(id: u64, timeout_ms: u32)".into()),
            ..Default::default()
        };
        let query_status_2 = compare_contract(&sym_old_2, &sym_new_2);
        let contracts_status_2 = compare_symbol_contract(&sym_old_2, &sym_new_2);
        assert!(matches!(
            query_status_2,
            ContractStatus::ContractChange { .. }
        ));
        assert_eq!(contracts_status_2, query_status_2);

        // Fixture 3: Different symbol identity (new symbol)
        let sym_old_3 = Symbol {
            name: "old_name".into(),
            signature: Some("pub fn old_name()".into()),
            ..Default::default()
        };
        let sym_new_3 = Symbol {
            name: "new_name".into(),
            signature: Some("pub fn new_name()".into()),
            ..Default::default()
        };
        let query_status_3 = compare_contract(&sym_old_3, &sym_new_3);
        let contracts_status_3 = compare_symbol_contract(&sym_old_3, &sym_new_3);
        assert_eq!(query_status_3, ContractStatus::NewSymbol);
        assert_eq!(contracts_status_3, query_status_3);

        // Fixture 4: Code-level source string edit-check matching compare_contract behavior
        let src_old = "pub fn query(q: &str) -> Vec<String> { vec![] }";
        let src_broke = "pub fn query(q: &str, limit: usize) -> Vec<String> { vec![] }";
        let breaks = edit_check_rust(src_old, src_broke).expect("edit_check_rust");
        assert_eq!(breaks.len(), 1);
        assert_eq!(breaks[0].symbol_name, "query");
        if let ContractStatus::ContractChange { ref was, ref now } = breaks[0].status {
            assert!(was.as_ref().unwrap().contains("query(q: &str)"));
            assert!(now
                .as_ref()
                .unwrap()
                .contains("query(q: &str, limit: usize)"));
        } else {
            panic!("expected ContractChange");
        }
    }
}
