use crate::plugin::types::PluginDescriptor;
use crate::plugin::PluginManager;
use crate::types::Language;
use std::collections::HashMap;
use std::sync::Arc;

/// Discovers languages and their associated plugins.
pub struct LanguageDiscovery {
    manager: Arc<PluginManager>,
}

impl LanguageDiscovery {
    pub fn new(manager: Arc<PluginManager>) -> Self {
        Self { manager }
    }

    /// name -> list of plugin names that handle it, e.g. { "python": ["parser-python"], "ruby": ["parser-ruby"] }
    ///
    /// Built-in languages with only native parsers are NOT included (only plugin-backed languages appear).
    pub fn discover(&self) -> HashMap<String, Vec<String>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for plugin in self.manager.list() {
            for lang in &plugin.descriptor.languages {
                if let Language::Unknown = lang {
                    continue;
                }
                map.entry(lang.as_str().to_string())
                    .or_default()
                    .push(plugin.descriptor.name.clone());
            }
        }
        map
    }

    /// All languages known to the system: built-in enum variants + one
    /// Language::Other(name) per plugin-backed language. De-duplicated, sorted.
    /// Language::Unknown is excluded.
    pub fn languages(&self) -> Vec<Language> {
        let mut langs = vec![
            Language::Rust,
            Language::TypeScript,
            Language::JavaScript,
            Language::Python,
            Language::Go,
            Language::Java,
            Language::C,
            Language::Cpp,
        ];

        let discovered = self.discover();
        for lang_name in discovered.keys() {
            let is_builtin = matches!(
                lang_name.as_str(),
                "rust"
                    | "typescript"
                    | "javascript"
                    | "python"
                    | "go"
                    | "java"
                    | "c"
                    | "cpp"
            );
            if !is_builtin {
                langs.push(Language::Other(lang_name.clone()));
            }
        }

        // De-duplicate: although built-ins are unique and we only add non-builtins,
        // we should ensure uniqueness.
        // Actually, since we only add if !is_builtin, and discovered keys are unique,
        // we just need to sort.
        langs.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        langs
    }

    /// Plugins installed for a language string (matches Language::as_str()).
    /// Case-insensitive match on the input string.
    pub fn plugins_for(&self, lang_name: &str) -> Vec<PluginDescriptor> {
        let target = lang_name.to_lowercase();
        self.manager
            .list()
            .into_iter()
            .filter(|p| {
                p.descriptor
                    .languages
                    .iter()
                    .any(|l| l.as_str().to_lowercase() == target)
            })
            .map(|p| p.descriptor)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::types::PluginDescriptor;

    #[test]
    fn discover_maps_languages_to_plugins() {
        let manager = Arc::new(PluginManager::new());
        manager.register(PluginDescriptor {
            name: "parser-python".into(),
            version: "1.0.0".into(),
            command: "python-cmd".into(),
            languages: vec![Language::Python],
            capabilities: vec!["parse".into()],
        });
        manager.register(PluginDescriptor {
            name: "parser-ruby".into(),
            version: "1.0.0".into(),
            command: "ruby-cmd".into(),
            languages: vec![Language::Other("ruby".into())],
            capabilities: vec!["parse".into()],
        });

        let discovery = LanguageDiscovery::new(manager);
        let discovered = discovery.discover();

        assert_eq!(discovered.get("python"), Some(&vec!["parser-python".into()]));
        assert_eq!(discovered.get("ruby"), Some(&vec!["parser-ruby".into()]));
    }

    #[test]
    fn languages_includes_builtins_and_plugin_discovered() {
        let manager = Arc::new(PluginManager::new());
        manager.register(PluginDescriptor {
            name: "parser-ruby".into(),
            version: "1.0.0".into(),
            command: "ruby-cmd".into(),
            languages: vec![Language::Other("ruby".into())],
            capabilities: vec!["parse".into()],
        });

        let discovery = LanguageDiscovery::new(manager);
        let langs = discovery.languages();

        assert!(langs.contains(&Language::Rust));
        assert!(langs.contains(&Language::Cpp));
        assert!(langs.contains(&Language::Other("ruby".into())));
        assert!(!langs.contains(&Language::Unknown));

        // Verify sorted order (by as_str())
        let sorted_names: Vec<String> = langs.iter().map(|l| l.as_str().to_string()).collect();
        let mut expected = sorted_names.clone();
        expected.sort();
        assert_eq!(sorted_names, expected);
    }

    #[test]
    fn plugins_for_is_case_insensitive() {
        let manager = Arc::new(PluginManager::new());
        manager.register(PluginDescriptor {
            name: "parser-python".into(),
            version: "1.0.0".into(),
            command: "python-cmd".into(),
            languages: vec![Language::Python],
            capabilities: vec!["parse".into()],
        });

        let discovery = LanguageDiscovery::new(manager);

        let p1 = discovery.plugins_for("python");
        let p2 = discovery.plugins_for("Python");

        assert_eq!(p1.len(), 1);
        assert_eq!(p1[0].name, "parser-python");
        assert_eq!(p1, p2);
    }

    #[test]
    fn empty_when_nothing_installed() {
        let manager = Arc::new(PluginManager::new());
        let discovery = LanguageDiscovery::new(manager);

        assert!(discovery.discover().is_empty());
        let langs = discovery.languages();
        // Should contain 8 built-ins
        assert_eq!(langs.len(), 8);
        assert!(langs.contains(&Language::Rust));
        assert!(langs.contains(&Language::TypeScript));
        assert!(langs.contains(&Language::JavaScript));
        assert!(langs.contains(&Language::Python));
        assert!(langs.contains(&Language::Go));
        assert!(langs.contains(&Language::Java));
        assert!(langs.contains(&Language::C));
        assert!(langs.contains(&Language::Cpp));
    }
}
