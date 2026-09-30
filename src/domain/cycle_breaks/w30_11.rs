//! Pure contracts shared by search, context, and memory.
//!
//! Issue W30-11 (ADR-033 Wave 0) cuts three module edges: `search` -> `memory`
//! (`hooks`, `rerank`), `search` -> `retrieval` and `context` -> `retrieval`
//! (`hybrid`). The test module below enforces them per module directory, so a
//! new module of `search/` or `context/` has to be classified instead of
//! slipping past the guard.
//!
//! One edge survives and is not enforced here: `search/hybrid.rs` still queries
//! `crate::memory::qmd_memory` in its public signatures. Inverting it needs an
//! `impl` inside `src/memory/**`, which belongs to a follow-up issue.

use serde::{Deserialize, Serialize};
use xavier_core_logic::{ClearanceLevel, ContextZone, MemoryLevel};

pub const DEFAULT_RRF_K: u32 = 60;
pub const DEFAULT_KEYWORD_WEIGHT: f32 = 0.5;
pub const DEFAULT_VECTOR_WEIGHT: f32 = 0.5;

/// Memory contracts moved by W30-01; re-exported here for the search/context cuts.
pub use super::w30_01::{
    EvidenceKind, FederatedSearchRequest, MemoryKind, MemoryQueryFilters, RetrievalScope,
};

#[cfg(test)]
mod tests {
    use super::{FederatedSearchRequest, MemoryQueryFilters};

    const SEARCH_MOD: &str = include_str!("../../search/mod.rs");
    const CONTEXT_MOD: &str = include_str!("../../context/mod.rs");

    /// Every module of `search/`, with its source.
    const SEARCH_SOURCES: &[(&str, &str)] = &[
        ("bm25", include_str!("../../search/bm25.rs")),
        ("hooks", include_str!("../../search/hooks.rs")),
        ("hybrid", include_str!("../../search/hybrid.rs")),
        ("rerank", include_str!("../../search/rerank.rs")),
        ("rrf", include_str!("../../search/rrf.rs")),
    ];

    /// Every file module of `context/`, with its source.
    const CONTEXT_SOURCES: &[(&str, &str)] = &[
        ("bm25", include_str!("../../context/bm25.rs")),
        ("builder", include_str!("../../context/builder.rs")),
        ("classifier", include_str!("../../context/classifier.rs")),
        ("executor", include_str!("../../context/executor.rs")),
        (
            "graph_retriever",
            include_str!("../../context/graph_retriever.rs"),
        ),
        ("hybrid", include_str!("../../context/hybrid.rs")),
        ("indexer", include_str!("../../context/indexer.rs")),
        ("manager", include_str!("../../context/manager.rs")),
        ("monitoring", include_str!("../../context/monitoring.rs")),
        (
            "orchestrator",
            include_str!("../../context/orchestrator.rs"),
        ),
        ("pipeline", include_str!("../../context/pipeline.rs")),
        (
            "query_processor",
            include_str!("../../context/query_processor.rs"),
        ),
        (
            "query_router",
            include_str!("../../context/query_router.rs"),
        ),
        ("regen_loop", include_str!("../../context/regen_loop.rs")),
        (
            "skill_dispatcher",
            include_str!("../../context/skill_dispatcher.rs"),
        ),
        (
            "skill_registry",
            include_str!("../../context/skill_registry.rs"),
        ),
        ("skills", include_str!("../../context/skills.rs")),
        (
            "tests_query_router",
            include_str!("../../context/tests_query_router.rs"),
        ),
        ("timeline", include_str!("../../context/timeline.rs")),
        (
            "token_estimate",
            include_str!("../../context/token_estimate.rs"),
        ),
    ];

    /// `context/mod.rs` modules that live in a directory
    /// (`src/context/<name>/mod.rs`) and have no single source file to include.
    const CONTEXT_DIRECTORY_MODULES: &[&str] = &["reranker", "skill_controller"];

    /// `search/hybrid.rs` queries the QMD store in its public signatures, so it
    /// is exempt from the `memory` check until that edge is inverted. Every
    /// other `search/` module must not reach `memory` at all.
    const SEARCH_MEMORY_EXEMPT: &[&str] = &["hybrid"];

    /// Every `crate::…` path named by the non-test part of `source`. Grouped
    /// `use` trees are expanded too, so a branch such as the `memory` in
    /// `use crate::{embedding, memory::…}` is reported like an inline path.
    fn crate_paths(source: &str) -> Vec<String> {
        let head = match source.split_once("#[cfg(test)]") {
            Some((head, _)) => head,
            None => source,
        };
        let code: String = head
            .lines()
            .map(strip_line_comment)
            .collect::<Vec<&str>>()
            .join("\n");

        let mut paths: Vec<String> = Vec::new();
        let mut buffer = String::new();
        for line in code.lines() {
            let trimmed = line.trim();
            let starts_use = trimmed.starts_with("use ") || trimmed.starts_with("pub use ");
            if buffer.is_empty() && !starts_use {
                continue;
            }
            buffer.push(' ');
            buffer.push_str(trimmed);
            if !trimmed.ends_with(';') {
                continue;
            }
            let statement = buffer.trim();
            let body = statement
                .trim_start_matches("pub ")
                .trim_start()
                .trim_start_matches("use ")
                .trim()
                .trim_end_matches(';');
            push_use_tree("", body.trim(), &mut paths);
            buffer.clear();
        }

        let mut rest = code.as_str();
        while let Some(index) = rest.find("crate::") {
            rest = &rest[index + "crate::".len()..];
            let segments: String = rest
                .chars()
                .take_while(|character| {
                    character.is_alphanumeric() || *character == '_' || *character == ':'
                })
                .collect();
            paths.push(format!("crate::{}", segments.trim_end_matches(':')));
        }

        paths.sort();
        paths.dedup();
        paths
    }

    /// Drop the `//` comment of a line. String and char literals are honoured,
    /// so `"http://host/v1"` keeps the rest of its line and a `crate::` path
    /// that follows such a literal is still reported. A misread literal can only
    /// keep a comment that should have been dropped, which fails a guard loudly
    /// instead of passing it silently.
    fn strip_line_comment(line: &str) -> &str {
        let bytes = line.as_bytes();
        let mut index = 0;
        let mut in_string = false;
        let mut in_char = false;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' if in_string || in_char => index += 1,
                b'"' if !in_char => in_string = !in_string,
                b'\'' if !in_string && !opens_lifetime(bytes, index) => in_char = !in_char,
                b'/' if !in_string && !in_char && bytes.get(index + 1) == Some(&b'/') => {
                    return &line[..index];
                }
                _ => {}
            }
            index += 1;
        }
        line
    }

    /// `'a` opens a lifetime, `'x'` and `'\n'` open a char literal.
    fn opens_lifetime(bytes: &[u8], index: usize) -> bool {
        bytes
            .get(index + 1)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    }

    /// Expand one level of a `use` tree, prefixing every leaf with `prefix`.
    fn push_use_tree(prefix: &str, body: &str, paths: &mut Vec<String>) {
        for item in split_at_top_level_commas(body) {
            let item = item.trim();
            let open = item.find('{').map(|brace| (brace, brace + 1));
            let Some((brace, first)) = open else {
                if !item.is_empty() {
                    paths.push(join_path(prefix, item.split(" as ").next().unwrap_or(item)));
                }
                continue;
            };
            let Some(last) = item.rfind('}') else {
                continue;
            };
            let head = item[..brace].trim().trim_end_matches(':');
            let nested = if head.is_empty() {
                prefix.to_string()
            } else {
                join_path(prefix, head)
            };
            push_use_tree(&nested, &item[first..last], paths);
        }
    }

    /// Split on the commas that sit outside any brace group.
    fn split_at_top_level_commas(body: &str) -> Vec<&str> {
        let mut items = Vec::new();
        let mut depth = 0usize;
        let mut start = 0usize;
        for (index, character) in body.char_indices() {
            match character {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    items.push(&body[start..index]);
                    start = index + 1;
                }
                _ => {}
            }
        }
        items.push(&body[start..]);
        items
    }

    fn join_path(prefix: &str, segment: &str) -> String {
        if prefix.is_empty() {
            segment.to_string()
        } else {
            format!("{prefix}::{segment}")
        }
    }

    fn offenders(paths: &[String], module: &str) -> Vec<String> {
        let bare = format!("crate::{module}");
        paths
            .iter()
            .filter(|path| path.starts_with(&format!("{bare}::")) || **path == bare)
            .cloned()
            .collect()
    }

    /// Module names declared by a `mod.rs`, sorted.
    fn declared_modules(source: &str) -> Vec<String> {
        let mut names: Vec<String> = source
            .lines()
            .map(str::trim)
            .filter_map(|line| {
                let rest = line
                    .strip_prefix("pub mod ")
                    .or_else(|| line.strip_prefix("mod "))?;
                let name = rest.split(';').next()?.trim();
                if name.is_empty() || name.contains([' ', '{', ':']) {
                    return None;
                }
                Some(name.to_string())
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Module names the guard covers, sorted.
    fn guarded_names(sources: &[(&str, &str)]) -> Vec<String> {
        let mut names: Vec<String> = sources
            .iter()
            .map(|(name, _source)| (*name).to_string())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn moved_filters_deserialize_with_the_same_defaults() {
        let filters: MemoryQueryFilters =
            serde_json::from_value(serde_json::json!({})).expect("filters deserialize");
        assert_eq!(filters.kinds, None);
        assert_eq!(filters.include_activity, None);
        assert_eq!(filters.federated, None);

        let request: FederatedSearchRequest =
            serde_json::from_value(serde_json::json!({})).expect("request deserializes");
        assert_eq!(request.max_hops, 1);
        assert!(!request.propagate_to_mesh);
    }

    /// `search` and `retrieval` resolve the shared contract through separate
    /// code paths, so a literal or a divergent default on either side fails here.
    #[test]
    fn search_and_retrieval_resolve_the_shared_fusion_defaults() {
        let _env = crate::settings::tests::TempEnv::new();
        // A settings file would win over the defaults under test, so point the
        // loader at a path that cannot exist.
        std::env::set_var(
            "XAVIER_CONFIG_PATH",
            std::env::temp_dir()
                .join("xavier-w30-11-absent")
                .join("xavier.toml"),
        );
        // `XAVIER_RRF_K` is the only fusion knob the settings loader reads from
        // the environment, so an ambient value must not reach the comparison.
        std::env::remove_var("XAVIER_RRF_K");

        let searcher = crate::search::hybrid::HybridSearcher::default();
        assert_eq!(searcher.rrf_k, super::DEFAULT_RRF_K);
        assert_eq!(
            searcher.keyword_weight.to_bits(),
            super::DEFAULT_KEYWORD_WEIGHT.to_bits()
        );
        assert_eq!(
            searcher.vector_weight.to_bits(),
            super::DEFAULT_VECTOR_WEIGHT.to_bits()
        );
        assert_eq!(
            crate::search::hybrid::configured_rrf_k(),
            crate::retrieval::config::configured_rrf_k()
        );
        assert_eq!(
            crate::retrieval::config::configured_keyword_weight().to_bits(),
            searcher.keyword_weight.to_bits()
        );
        assert_eq!(
            crate::retrieval::config::configured_vector_weight().to_bits(),
            searcher.vector_weight.to_bits()
        );
    }

    #[test]
    fn the_guard_covers_every_search_module() {
        assert_eq!(declared_modules(SEARCH_MOD), guarded_names(SEARCH_SOURCES));
    }

    #[test]
    fn the_guard_covers_every_context_module() {
        let declared: Vec<String> = declared_modules(CONTEXT_MOD)
            .into_iter()
            .filter(|name| !CONTEXT_DIRECTORY_MODULES.contains(&name.as_str()))
            .collect();
        assert_eq!(declared, guarded_names(CONTEXT_SOURCES));
    }

    #[test]
    fn no_search_module_imports_retrieval() {
        for (name, source) in SEARCH_SOURCES {
            let hits = offenders(&crate_paths(source), "retrieval");
            assert!(
                hits.is_empty(),
                "search/{name}.rs imports retrieval: {hits:?}"
            );
        }
    }

    #[test]
    fn no_context_module_imports_retrieval() {
        for (name, source) in CONTEXT_SOURCES {
            let hits = offenders(&crate_paths(source), "retrieval");
            assert!(
                hits.is_empty(),
                "context/{name}.rs imports retrieval: {hits:?}"
            );
        }
    }

    #[test]
    fn search_modules_other_than_hybrid_do_not_import_memory() {
        for (name, source) in SEARCH_SOURCES {
            if SEARCH_MEMORY_EXEMPT.contains(name) {
                continue;
            }
            let hits = offenders(&crate_paths(source), "memory");
            assert!(hits.is_empty(), "search/{name}.rs imports memory: {hits:?}");
        }
    }

    /// The cycle guards are only as good as this scanner, and the literal
    /// acceptance greps of the issue cannot see inside a grouped `use` tree.
    #[test]
    fn the_scanner_sees_grouped_use_trees_strings_and_comments() {
        let probe = r#"
use crate::{embedding, memory::schema::MemoryQueryFilters};
use crate::retrieval::config::DEFAULT_RRF_K;
// use crate::memory::schema::MemoryKind;
fn probe<'a>(hook: &'a dyn crate::search::hooks::SearchHook) {
    let probe_pair = ("http://localhost:11434/v1/rerank", hook.name(), crate::search::rerank::RerankHook::name);
    let _ = probe_pair;
}
"#;
        let paths = crate_paths(probe);
        assert!(
            paths.contains(&"crate::memory::schema::MemoryQueryFilters".to_string()),
            "grouped use tree not expanded: {paths:?}"
        );
        assert!(
            paths.contains(&"crate::retrieval::config::DEFAULT_RRF_K".to_string()),
            "use statement not read: {paths:?}"
        );
        assert!(
            paths.contains(&"crate::search::hooks::SearchHook".to_string()),
            "path in a signature not read: {paths:?}"
        );
        assert!(
            paths.contains(&"crate::search::rerank::RerankHook::name".to_string()),
            "crate path after a string literal lost: {paths:?}"
        );
        assert_eq!(offenders(&paths, "memory").len(), 1);
        assert_eq!(offenders(&paths, "retrieval").len(), 1);
    }
}
