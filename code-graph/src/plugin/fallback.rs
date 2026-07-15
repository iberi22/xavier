//! Per-language fallback chains.
//!
//! Each language maps to an ordered list of [`FallbackStep`]s tried in turn.
//! The default chain for a built-in language is `[Native, NoOp]`; once a plugin
//! is registered via [`crate::plugin::PluginManager`] the live chain becomes
//! `[Plugin(name), Native, NoOp]` regardless of what's persisted here.
//!
//! User-customised chains persist to `<config_dir>/code-graph/fallback.json`
//! so an operator can pin e.g. Python to plugin-only or disable native parsers.
//! A missing or malformed file is non-fatal — defaults take over.

use crate::plugin::types::{FallbackResolver, FallbackStep};
use crate::types::Language;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, warn};

/// Serializable shape persisted to `fallback.json`. Keys are language
/// identifiers produced by [`Language::as_db_str`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FallbackConfig {
    pub chains: HashMap<String, Vec<FallbackStep>>,
}

/// Resolves which steps to try for a given language, with optional overrides.
pub struct FallbackChain {
    overrides: HashMap<String, Vec<FallbackStep>>,
}

impl Default for FallbackChain {
    fn default() -> Self {
        Self::load_or_default()
    }
}

impl FallbackChain {
    /// Load overrides from disk, falling back to empty (i.e. all-default) on
    /// any error or missing file. Never returns `Err`.
    pub fn load_or_default() -> Self {
        let mut overrides = HashMap::new();
        if let Some(path) = config_path() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<FallbackConfig>(&content) {
                    Ok(cfg) => overrides = cfg.chains,
                    Err(e) => warn!(?path, error = %e, "fallback.json malformed, using defaults"),
                },
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    warn!(?path, error = %e, "cannot read fallback.json, using defaults")
                }
                Err(_) => debug!("no fallback.json, using defaults"),
            }
        }
        Self { overrides }
    }

    /// The chain to use when no plugin is registered for `lang`.
    ///
    /// - If an override exists for this language, use it verbatim.
    /// - Else if the language has a built-in tree-sitter parser, use `[Native, NoOp]`.
    /// - Else (plugin-only / unknown language) use `[NoOp]`.
    fn default_chain(lang: &Language) -> Vec<FallbackStep> {
        if crate::parser::has_native_parser(lang) {
            vec![FallbackStep::Native, FallbackStep::NoOp]
        } else {
            vec![FallbackStep::NoOp]
        }
    }

    /// Persist current overrides to disk (best-effort).
    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        let cfg = FallbackConfig {
            chains: self.overrides.clone(),
        };
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!(?parent, error = %e, "cannot create fallback.json parent dir");
                return;
            }
        }
        match serde_json::to_string_pretty(&cfg) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    warn!(?path, error = %e, "cannot write fallback.json");
                }
            }
            Err(e) => warn!(error = %e, "cannot serialize fallback config"),
        }
    }

    /// Replace the chain for a single language. Caller is responsible for
    /// calling [`save`] if persistence is desired.
    pub fn set(&mut self, lang: &Language, steps: Vec<FallbackStep>) {
        self.overrides.insert(lang.as_db_str(), steps);
    }

    /// Remove any override for a language, reverting it to the default chain.
    pub fn clear(&mut self, lang: &Language) {
        self.overrides.remove(&lang.as_db_str());
    }

    /// Return all explicitly configured chains.
    pub fn all_chains(&self) -> Vec<(Language, Vec<FallbackStep>)> {
        self.overrides
            .iter()
            .map(|(lang_str, steps)| (Language::from_db_str(lang_str), steps.clone()))
            .collect()
    }
}

impl FallbackResolver for FallbackChain {
    fn chain_for(&self, lang: &Language) -> Vec<FallbackStep> {
        if let Some(steps) = self.overrides.get(&lang.as_db_str()) {
            // A present override is authoritative — including an empty one,
            // which an operator can use to disable a language entirely
            // (resolves to `[NoOp]`).
            if steps.is_empty() {
                return vec![FallbackStep::NoOp];
            }
            return steps.clone();
        }
        Self::default_chain(lang)
    }
}

/// Path to `fallback.json` under the platform config dir (or `None` if the
/// platform exposes no config dir).
fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("code-graph").join("fallback.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_language_defaults_to_native_then_noop() {
        let chain = FallbackChain {
            overrides: HashMap::new(),
        };
        assert_eq!(
            chain.chain_for(&Language::Rust),
            vec![FallbackStep::Native, FallbackStep::NoOp],
        );
        assert_eq!(
            chain.chain_for(&Language::Python),
            vec![FallbackStep::Native, FallbackStep::NoOp],
        );
    }

    #[test]
    fn unknown_or_plugin_only_language_defaults_to_noop() {
        let chain = FallbackChain {
            overrides: HashMap::new(),
        };
        assert_eq!(
            chain.chain_for(&Language::Unknown),
            vec![FallbackStep::NoOp]
        );
        assert_eq!(
            chain.chain_for(&Language::Other("ruby".into())),
            vec![FallbackStep::NoOp],
        );
    }

    #[test]
    fn override_is_respected_when_present() {
        let mut overrides = HashMap::new();
        overrides.insert(
            Language::Python.as_db_str(),
            vec![FallbackStep::Plugin("parser-py".into())],
        );
        let chain = FallbackChain { overrides };
        assert_eq!(
            chain.chain_for(&Language::Python),
            vec![FallbackStep::Plugin("parser-py".into())],
        );
        // Rust keeps its default.
        assert_eq!(
            chain.chain_for(&Language::Rust),
            vec![FallbackStep::Native, FallbackStep::NoOp],
        );
    }

    #[test]
    fn empty_override_means_noop_only() {
        let mut overrides = HashMap::new();
        overrides.insert(Language::Rust.as_db_str(), vec![]);
        let chain = FallbackChain { overrides };
        assert_eq!(chain.chain_for(&Language::Rust), vec![FallbackStep::NoOp]);
    }

    #[test]
    fn set_and_clear_round_trip() {
        let mut chain = FallbackChain {
            overrides: HashMap::new(),
        };
        chain.set(
            &Language::Go,
            vec![FallbackStep::Plugin("parser-go".into()), FallbackStep::NoOp],
        );
        assert_eq!(
            chain.chain_for(&Language::Go),
            vec![FallbackStep::Plugin("parser-go".into()), FallbackStep::NoOp],
        );
        chain.clear(&Language::Go);
        assert_eq!(
            chain.chain_for(&Language::Go),
            vec![FallbackStep::Native, FallbackStep::NoOp],
        );
    }

    #[test]
    fn load_or_default_does_not_panic_without_a_file() {
        // There is almost certainly no fallback.json under the test's config
        // dir, so this exercises the NotFound branch.
        let _ = FallbackChain::load_or_default();
    }
}
