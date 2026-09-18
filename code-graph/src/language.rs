//! Language registry (O2: `feat-cg-language-registry`).
//!
//! Ideas rewritten from `Graphify-Labs/graphify` (`extractors/models.py`
//! `LanguageConfig`, `resolver_registry.py`, `ARCHITECTURE.md#Adding`) and
//! the builtin-globals guard (`base.py#_LANGUAGE_BUILTIN_GLOBALS`).
//! No upstream code. See `docs/EXTRACTION-RIPWIRE-GRAPHIFY.md` (G2).
//!
//! Rule: adding a language = registering one [`LanguageConfig`] (plus its
//! tree-sitter glue elsewhere). The indexer core is never edited.

use crate::types::{Language, LanguageDiscovery};

/// Built-in configs mirroring `Language::from_extension` (+ `mts/cts/mjs/cjs`
/// aliases) and `has_native_parser`. If a new built-in is ever added, it
/// must extend both this table and `from_extension`.
const BUILTINS: [LanguageConfig; 8] = [
    LanguageConfig {
        id: "rust",
        extensions: &["rs"],
        builtin_globals: &[
            "String", "Vec", "Option", "Result", "Box", "Clone", "Debug", "Default", "Iterator",
            "Into", "From", "ToString", "Send", "Sync",
            // Dogfood 2026-09-18: std method names collide across files and
            // top god-node lists without meaning architecture.
            "new", "get", "len", "iter", "map", "push", "is_empty", "main", "clone", "default",
            "fmt", "from",
        ],
        has_native_parser: true,
    },
    LanguageConfig {
        id: "typescript",
        extensions: &["ts", "tsx", "mts", "cts"],
        builtin_globals: &[
            "String",
            "Number",
            "Boolean",
            "Object",
            "Array",
            "Promise",
            "console",
            "undefined",
            "null",
            "get",
            "set",
            "push",
            "map",
            "length",
            "forEach",
        ],
        has_native_parser: true,
    },
    LanguageConfig {
        id: "javascript",
        extensions: &["js", "jsx", "mjs", "cjs"],
        builtin_globals: &[
            "String",
            "Number",
            "Boolean",
            "Object",
            "Array",
            "Promise",
            "console",
            "undefined",
            "null",
            "get",
            "set",
            "push",
            "map",
            "length",
            "forEach",
        ],
        has_native_parser: true,
    },
    LanguageConfig {
        id: "python",
        extensions: &["py"],
        builtin_globals: &[
            "print",
            "len",
            "str",
            "int",
            "list",
            "dict",
            "set",
            "None",
            "True",
            "False",
            "self",
            "range",
            "isinstance",
            "append",
            "get",
            "set",
            "add",
            "update",
            "keys",
            "values",
            "items",
        ],
        has_native_parser: true,
    },
    LanguageConfig {
        id: "go",
        extensions: &["go"],
        builtin_globals: &[
            "string", "int", "error", "nil", "make", "len", "cap", "append", "panic", "print",
            "println", "get", "Errorf",
        ],
        has_native_parser: true,
    },
    LanguageConfig {
        id: "java",
        extensions: &["java"],
        builtin_globals: &[
            "String",
            "Integer",
            "Object",
            "System",
            "Math",
            "List",
            "Exception",
            "get",
            "set",
            "add",
            "size",
            "put",
        ],
        has_native_parser: true,
    },
    LanguageConfig {
        id: "c",
        extensions: &["c", "h"],
        builtin_globals: &[
            "int", "char", "void", "NULL", "size_t", "printf", "malloc", "free", "get", "set",
        ],
        has_native_parser: true,
    },
    LanguageConfig {
        id: "cpp",
        extensions: &["cpp", "cc", "cxx", "hpp"],
        builtin_globals: &[
            "string", "vector", "cout", "endl", "std", "int", "void", "nullptr", "get", "set",
            "push", "size", "begin", "end",
        ],
        has_native_parser: true,
    },
];

/// Canonical [`Language`] for a registry id. Built-in ids map to their
/// variant; anything else becomes a plugin-backed `Other(id)`.
fn language_for_id(id: &str) -> Language {
    match id {
        "rust" => Language::Rust,
        "typescript" => Language::TypeScript,
        "javascript" => Language::JavaScript,
        "python" => Language::Python,
        "go" => Language::Go,
        "java" => Language::Java,
        "c" => Language::C,
        "cpp" => Language::Cpp,
        other => Language::Other(other.to_string()),
    }
}

/// Static description of one indexable language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageConfig {
    /// Canonical lowercase id, e.g. `"rust"`. Matches [`Language::as_str`].
    pub id: &'static str,
    /// File extensions (lowercase, without dot) mapping to this language.
    pub extensions: &'static [&'static str],
    /// Global names that are never architecture signals (anti-god-nodes).
    pub builtin_globals: &'static [&'static str],
    /// Backed by a built-in tree-sitter parser (`parse_native` can succeed).
    pub has_native_parser: bool,
}

/// Ordered registry of [`LanguageConfig`]. Built-ins first, custom
/// registrations appended. Resolution is first-match, so a custom config
/// can never shadow a built-in extension (explicit rejections instead).
#[derive(Debug, Default)]
pub struct LanguageRegistry {
    entries: Vec<LanguageConfig>,
}

impl LanguageRegistry {
    /// Registry preloaded with the 8 built-in tree-sitter languages.
    pub fn with_builtins() -> Self {
        Self {
            entries: BUILTINS.to_vec(),
        }
    }

    /// Register a custom language. Fails (never panics, never partial) when
    /// `id` is taken or any extension collides with an existing entry —
    /// warn-and-continue at the call site (graphify `resolver_registry`).
    pub fn register(&mut self, config: LanguageConfig) -> Result<(), String> {
        if self.entries.iter().any(|e| e.id == config.id) {
            return Err(format!("language id '{}' already registered", config.id));
        }
        if let Some(hit) = self.entries.iter().find(|e| {
            e.extensions
                .iter()
                .any(|have| config.extensions.contains(have))
        }) {
            return Err(format!(
                "extension collision: '{}' already handled by '{}'",
                config.extensions.join(","),
                hit.id
            ));
        }
        self.entries.push(config);
        Ok(())
    }

    /// Resolve an extension (case-insensitive, dot optional) to a
    /// [`Language`]. Unknown extensions yield [`Language::Unknown`] so
    /// callers can report `unindexed` (O1 honesty) instead of guessing.
    pub fn language_for_extension(&self, ext: &str) -> Language {
        let norm = ext.trim().trim_start_matches('.').to_lowercase();
        for entry in &self.entries {
            if entry.extensions.contains(&norm.as_str()) {
                return language_for_id(entry.id);
            }
        }
        Language::Unknown
    }

    /// Config backing `lang`, if registered.
    pub fn config_for(&self, lang: &Language) -> Option<&LanguageConfig> {
        let id = lang.as_str();
        self.entries.iter().find(|e| e.id == id)
    }

    /// True when `name` is a builtin global of `lang` (never a god-node).
    pub fn is_builtin_global(&self, lang: &Language, name: &str) -> bool {
        self.config_for(lang)
            .map(|c| c.builtin_globals.contains(&name))
            .unwrap_or(false)
    }

    /// Extensions with no registered language, lowercased, deduplicated,
    /// sorted — the `unindexed="ext:N"` map-header material (O1).
    pub fn unindexed_extensions<'a>(&self, exts: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for ext in exts {
            let norm = ext.trim().trim_start_matches('.').to_lowercase();
            if self.language_for_extension(&norm) == Language::Unknown && !out.contains(&norm) {
                out.push(norm);
            }
        }
        out.sort();
        out
    }
}

impl LanguageDiscovery for LanguageRegistry {
    fn language_for_extension(&self, ext: &str) -> Language {
        LanguageRegistry::language_for_extension(self, ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZIG: LanguageConfig = LanguageConfig {
        id: "zig",
        extensions: &["zig"],
        builtin_globals: &["u8", "void", "unreachable", "undefined"],
        has_native_parser: false,
    };

    #[test]
    fn test_register_language_without_touching_core() {
        // O2 proof (US-103): a whole language plugs in via one struct.
        let mut reg = LanguageRegistry::with_builtins();
        assert_eq!(reg.language_for_extension("zig"), Language::Unknown);
        reg.register(ZIG).expect("register zig");
        assert_eq!(
            reg.language_for_extension("zig"),
            Language::Other("zig".to_string())
        );
        assert_eq!(
            reg.language_for_extension("ZIG"),
            Language::Other("zig".to_string())
        );
        let cfg = reg.config_for(&Language::Other("zig".to_string()));
        assert!(cfg.is_some());
        assert!(!cfg.unwrap().has_native_parser);
        // Built-ins untouched by the registration.
        assert_eq!(reg.language_for_extension("rs"), Language::Rust);
    }

    #[test]
    fn test_builtin_globals_excluded_from_hubs() {
        // O2 (US-104/G2): globals are never architecture signals.
        let reg = LanguageRegistry::with_builtins();
        assert!(reg.is_builtin_global(&Language::Python, "print"));
        assert!(reg.is_builtin_global(&Language::Rust, "String"));
        assert!(!reg.is_builtin_global(&Language::Rust, "require_permission"));
        assert!(!reg.is_builtin_global(&Language::Unknown, "print"));
    }

    #[test]
    fn test_method_noise_excluded_from_hubs() {
        // Dogfood fix (review 2026-09-18): std method names (`len`,
        // `iter`, `new`, …) top god-node lists via name collisions but are
        // never architecture. Structural limit documented in O7 notes:
        // per-name blocklists cannot cover every verb; file-spread
        // downranking stays future work.
        let reg = LanguageRegistry::with_builtins();
        for name in ["new", "get", "len", "iter", "map", "push", "is_empty"] {
            assert!(
                reg.is_builtin_global(&Language::Rust, name),
                "{name} must be method noise"
            );
        }
        assert!(reg.is_builtin_global(&Language::Python, "append"));
        assert!(reg.is_builtin_global(&Language::TypeScript, "push"));
        assert!(!reg.is_builtin_global(&Language::Rust, "blast_radius"));
    }

    #[test]
    fn test_unindexed_language_reported_not_empty() {
        // O2 + O1 honesty: unsupported extensions are named, not swallowed.
        let reg = LanguageRegistry::with_builtins();
        let got = reg.unindexed_extensions(["rs", "xyz", "XYZ", "abc"]);
        assert_eq!(got, vec!["abc".to_string(), "xyz".to_string()]);
        let none = reg.unindexed_extensions(["rs", "py"]);
        assert!(none.is_empty());
    }

    #[test]
    fn test_duplicate_registration_fails_without_breaking_registry() {
        // Warn-and-continue: a bad registration is an Err, never a panic,
        // and the registry keeps working afterwards.
        let mut reg = LanguageRegistry::with_builtins();
        let clash = LanguageConfig {
            id: "zig",
            extensions: &["rs"],
            builtin_globals: &[],
            has_native_parser: false,
        };
        assert!(reg.register(clash).is_err());
        assert_eq!(reg.language_for_extension("rs"), Language::Rust);
        reg.register(ZIG).expect("valid registration still works");
        assert_eq!(
            reg.language_for_extension("zig"),
            Language::Other("zig".to_string())
        );
    }
}
