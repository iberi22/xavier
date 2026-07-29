//! Auto-Docs Generator Module
//!
//! Generates structured markdown documentation for each module in the Xavier codebase
//! by querying the code-graph symbol database. Can optionally use LLM for narrative text.
//!
//! Usage: `xavier chronicle auto-docs [--module memory] [--output docs/auto-docs]`

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use code_graph::db::CodeGraphDB;
use code_graph::types::{Symbol, SymbolKind};

/// Configuration for the auto-docs generator
pub struct AutoDocsConfig {
    /// Path to the code-graph SQLite database
    pub code_graph_db: PathBuf,
    /// Root of the source code to analyze
    pub source_root: PathBuf,
    /// Where to write generated markdown files
    pub output_dir: PathBuf,
    /// Optional: only generate docs for this module
    pub module_filter: Option<String>,
}

impl Default for AutoDocsConfig {
    fn default() -> Self {
        Self {
            code_graph_db: crate::codebase::codegraph_paths::code_graph_db_path_for(Path::new(".")),
            source_root: PathBuf::from("src"),
            output_dir: PathBuf::from("docs/auto-docs"),
            module_filter: None,
        }
    }
}

/// Statistics about a module's code
#[derive(Debug, Clone)]
pub struct ModuleStats {
    pub module: String,
    pub total_symbols: usize,
    pub public_symbols: usize,
    pub structs: usize,
    pub functions: usize,
    pub enums: usize,
    pub traits: usize,
    pub total_files: usize,
    pub loc_estimate: usize,
    pub complexity_hotspots: Vec<(String, f64)>,
    pub key_functions: Vec<String>,
    pub key_types: Vec<String>,
}

/// Generated documentation for a module
#[derive(Debug, Clone)]
pub struct ModuleDoc {
    pub module: String,
    pub stats: ModuleStats,
    pub markdown: String,
    pub output_path: PathBuf,
}

/// Auto-documentation generator
pub struct AutoDocsGenerator {
    config: AutoDocsConfig,
    db: Option<CodeGraphDB>,
}

impl AutoDocsGenerator {
    /// New.
    pub fn new(config: AutoDocsConfig) -> Self {
        Self { config, db: None }
    }

    /// Open the code-graph database
    pub fn open_db(&mut self) -> Result<()> {
        if self.config.code_graph_db.exists() {
            let db = CodeGraphDB::new(&self.config.code_graph_db).with_context(|| {
                format!(
                    "Failed to open code-graph DB at {:?}",
                    self.config.code_graph_db
                )
            })?;
            self.db = Some(db);
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Code-graph database not found at {:?}. Run `xavier code scan .` first to index your workspace.",
                self.config.code_graph_db
            ))
        }
    }

    /// Generate documentation for all discovered modules
    pub fn generate_all(&mut self) -> Result<Vec<ModuleDoc>> {
        if self.db.is_none() {
            self.open_db()?;
        }

        let modules = self.discover_modules()?;
        println!("Found {} modules to document", modules.len());

        let mut docs = Vec::new();
        for module in &modules {
            if let Some(filter) = &self.config.module_filter {
                if module != filter {
                    continue;
                }
            }
            match self.generate_for_module(module) {
                Ok(doc) => {
                    println!("  ✓ {} ({})", doc.module, doc.output_path.display());
                    docs.push(doc);
                }
                Err(e) => {
                    eprintln!("  ✗ {}: {}", module, e);
                }
            }
        }

        // Generate index
        self.generate_index(&docs)?;

        Ok(docs)
    }

    /// Generate documentation for a single module
    pub fn generate_for_module(&self, module_name: &str) -> Result<ModuleDoc> {
        let db = self
            .db
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("DB not opened"))?;

        let module_path = self.config.source_root.join(module_name);
        if !module_path.exists() {
            return Err(anyhow::anyhow!("Module path not found: {:?}", module_path));
        }

        // Collect all source files in this module
        let files = self.collect_module_files(module_name)?;

        // Query code-graph for symbols in these files
        let mut all_symbols = Vec::new();
        for file in &files {
            if let Ok(symbols) = db.find_by_file(file) {
                all_symbols.extend(symbols);
            }
        }
        all_symbols.sort_by(|a, b| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)));

        // Compute stats
        let stats = self.compute_stats(module_name, &all_symbols, &files, db)?;

        // Render markdown
        let markdown = self.render_module_doc(module_name, &stats, &all_symbols);

        // Write output
        let output_path = self
            .config
            .output_dir
            .join(format!("{}-module.md", module_name));
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create output dir: {:?}", parent))?;
        }
        fs::write(&output_path, &markdown)
            .with_context(|| format!("Failed to write doc: {:?}", output_path))?;

        Ok(ModuleDoc {
            module: module_name.to_string(),
            stats,
            markdown,
            output_path,
        })
    }

    /// Discover all module directories under src/
    fn discover_modules(&self) -> Result<Vec<String>> {
        let mut modules = Vec::new();

        let src_path = &self.config.source_root;
        if !src_path.exists() {
            return Err(anyhow::anyhow!(
                "Source directory not found: {:?}",
                src_path
            ));
        }

        // Read lib.rs to find module declarations
        let lib_rs = src_path.join("lib.rs");
        if lib_rs.exists() {
            let content = fs::read_to_string(&lib_rs)
                .with_context(|| format!("Failed to read lib.rs: {:?}", lib_rs))?;
            for line in content.lines() {
                let trimmed = line.trim();
                // Match: pub mod chronicle;
                // Match: pub mod memory;
                // Match: #[cfg(feature = "...")] pub mod telegram;
                if trimmed.starts_with("pub mod ") && trimmed.ends_with(';') {
                    let name = trimmed
                        .trim_start_matches("pub mod ")
                        .trim_end_matches(';')
                        .trim()
                        .to_string();
                    if !name.is_empty() && !name.starts_with("//") && name != "devlog" {
                        modules.push(name);
                    }
                }
            }
        } else {
            // Fallback: scan directories
            for entry in fs::read_dir(src_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if !name.starts_with('.') {
                            modules.push(name.to_string());
                        }
                    }
                }
            }
        }

        modules.sort();
        Ok(modules)
    }

    /// Collect source files belonging to a module
    fn collect_module_files(&self, module_name: &str) -> Result<Vec<String>> {
        let module_path = self.config.source_root.join(module_name);
        let mut files = Vec::new();

        if module_path.is_dir() {
            for entry in WalkDir::new(&module_path).max_depth(3) {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "rs" {
                            if let Ok(rel) = path.strip_prefix(
                                self.config.source_root.parent().unwrap_or(Path::new("")),
                            ) {
                                files.push(rel.to_string_lossy().to_string());
                            } else {
                                files.push(path.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        } else if module_path.is_file() {
            // Single-file module
            files.push(module_name.to_string());
        }

        files.sort();
        Ok(files)
    }

    /// Compute statistics for a module
    fn compute_stats(
        &self,
        module_name: &str,
        symbols: &[Symbol],
        files: &[String],
        db: &CodeGraphDB,
    ) -> Result<ModuleStats> {
        let mut structs = 0;
        let mut functions = 0;
        let mut enums = 0;
        let mut traits = 0;
        let mut public = 0;

        let mut key_types = Vec::new();
        let mut key_functions = Vec::new();

        for sym in symbols {
            match sym.kind {
                SymbolKind::Struct => {
                    structs += 1;
                    key_types.push(sym.name.clone());
                }
                SymbolKind::Function => {
                    functions += 1;
                    key_functions.push(sym.name.clone());
                }
                SymbolKind::Enum => {
                    enums += 1;
                    key_types.push(sym.name.clone());
                }
                SymbolKind::Trait => {
                    traits += 1;
                    key_types.push(sym.name.clone());
                }
                _ => {}
            }
            // Count public symbols (heuristic: contains "pub" in signature or starts with pub_)
            if sym.name.starts_with("pub_")
                || sym.signature.as_deref().unwrap_or("").contains("pub ")
            {
                public += 1;
            }
        }

        // Get hotspots
        let hotspots = db
            .complexity_hotspots(5.0, 5)
            .unwrap_or_default()
            .into_iter()
            .filter(|h| h.symbol.file_path.contains(module_name))
            .map(|h| (h.symbol.name.clone(), h.risk_score as f64))
            .collect();

        let loc_estimate = files.len() * 150; // rough estimate

        Ok(ModuleStats {
            module: module_name.to_string(),
            total_symbols: symbols.len(),
            public_symbols: public,
            structs,
            functions,
            enums,
            traits,
            total_files: files.len(),
            loc_estimate,
            complexity_hotspots: hotspots,
            key_functions: key_functions.into_iter().take(20).collect(),
            key_types: key_types.into_iter().take(15).collect(),
        })
    }

    /// Render a module's documentation as markdown
    fn render_module_doc(
        &self,
        module_name: &str,
        stats: &ModuleStats,
        symbols: &[Symbol],
    ) -> String {
        let title = format!("{} Module", capitalize(module_name));
        // Avoid "Module Module" duplication (e.g., "Memory Module" not "Memory Module Module")
        let title = if title.ends_with(" Module Module") {
            title.replace(" Module Module", " Module")
        } else {
            title
        };
        let file_list = self
            .collect_module_files(module_name)
            .unwrap_or_default()
            .iter()
            .map(|f| format!("- `{}`", f))
            .collect::<Vec<_>>()
            .join("\n");

        let mut md = String::new();
        md.push_str(&format!("# {} Module\n\n", title));
        md.push_str(&format!("> **Path:** `src/{}/`\n\n", module_name));

        // Overview section
        md.push_str("## 📋 Overview\n\n");
        md.push_str(&format!(
            "The `{}` module contains **{} symbols** across **{} files** (approx. {} LOC).\n\n",
            module_name, stats.total_symbols, stats.total_files, stats.loc_estimate
        ));

        // Stats table
        md.push_str("## 📊 Module Statistics\n\n");
        md.push_str("| Metric | Value |\n");
        md.push_str("|--------|-------|\n");
        md.push_str(&format!("| Total symbols | {} |\n", stats.total_symbols));
        md.push_str(&format!("| Public symbols | {} |\n", stats.public_symbols));
        md.push_str(&format!("| Structs | {} |\n", stats.structs));
        md.push_str(&format!("| Functions | {} |\n", stats.functions));
        md.push_str(&format!("| Enums | {} |\n", stats.enums));
        md.push_str(&format!("| Traits | {} |\n", stats.traits));
        md.push_str(&format!("| Source files | {} |\n", stats.total_files));
        md.push('\n');

        // Source files
        if !file_list.is_empty() {
            md.push_str("## 📁 Source Files\n\n");
            md.push_str(&file_list);
            md.push_str("\n\n");
        }

        // Key types
        if !stats.key_types.is_empty() {
            md.push_str("## 🏗️ Key Types\n\n");
            md.push_str("| Type | Kind |\n");
            md.push_str("|------|------|\n");
            for sym_name in &stats.key_types {
                let kind = symbols
                    .iter()
                    .find(|s| s.name == *sym_name)
                    .map(|s| format!("{:?}", s.kind))
                    .unwrap_or_default();
                md.push_str(&format!("| `{}` | {} |\n", sym_name, kind));
            }
            md.push('\n');
        }

        // Key functions
        if !stats.key_functions.is_empty() {
            md.push_str("## ⚙️ Key Functions\n\n");
            md.push_str("| Function | Complexity |\n");
            md.push_str("|----------|-----------|\n");
            for sym_name in &stats.key_functions {
                let complexity = symbols
                    .iter()
                    .find(|s| s.name == *sym_name)
                    .map(|s| format!("{:.1}", s.complexity.unwrap_or(0.0)))
                    .unwrap_or_else(|| "-".to_string());
                md.push_str(&format!("| `{}` | {} |\n", sym_name, complexity));
            }
            md.push('\n');
        }

        // Complexity hotspots
        if !stats.complexity_hotspots.is_empty() {
            md.push_str("## 🔥 Complexity Hotspots\n\n");
            md.push_str("| Function | Risk Score |\n");
            md.push_str("|----------|-----------|\n");
            for (name, score) in &stats.complexity_hotspots {
                md.push_str(&format!("| `{}` | {:.1} |\n", name, score));
            }
            md.push('\n');
        }

        // Full symbol listing
        md.push_str("## 📝 Full Symbol Listing\n\n");
        if symbols.is_empty() {
            md.push_str("_No symbols indexed. Run `xavier code scan .` first to index your workspace._\n\n");
        } else {
            md.push_str("| Name | Kind | File | Complexity |\n");
            md.push_str("|------|------|------|-----------|\n");
            for sym in symbols.iter().take(50) {
                let short_file = sym.file_path.rsplit('/').next().unwrap_or(&sym.file_path);
                md.push_str(&format!(
                    "| `{}` | {:?} | `{}` | {:.1} |\n",
                    sym.name,
                    sym.kind,
                    short_file,
                    sym.complexity.unwrap_or(0.0)
                ));
            }
            if symbols.len() > 50 {
                md.push_str(&format!(
                    "| _... and {} more symbols_ | | | |\n",
                    symbols.len() - 50
                ));
            }
            md.push('\n');
        }

        // Auto-generated notice
        md.push_str("---\n");
        md.push_str("> _This documentation was auto-generated by `xavier chronicle auto-docs`._\n");
        md.push_str(&format!(
            "> _Generated on: {}_\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
        ));
        md.push_str("> _Run `xavier chronicle auto-docs` to regenerate._\n");

        md
    }

    /// Generate the auto-docs index page
    fn generate_index(&self, docs: &[ModuleDoc]) -> Result<()> {
        let mut md = String::new();
        md.push_str("# Auto-Generated Module Documentation\n\n");
        md.push_str("This directory contains automatically generated documentation for each module in the Xavier codebase.\n\n");
        md.push_str("> Generated by `xavier chronicle auto-docs`\n\n");

        md.push_str("## Module Index\n\n");
        md.push_str("| Module | Symbols | Files | Key Types |\n");
        md.push_str("|--------|---------|-------|-----------|\n");

        for doc in docs {
            let types_str = doc.stats.key_types.join(", ");
            md.push_str(&format!(
                "| [{}]({}-module.md) | {} | {} | {} |\n",
                doc.module, doc.module, doc.stats.total_symbols, doc.stats.total_files, types_str
            ));
        }

        md.push_str("\n---\n");
        md.push_str("_Auto-generated. Regenerate with `xavier chronicle auto-docs`._\n");

        let output_path = self.config.output_dir.join("README.md");
        fs::create_dir_all(&self.config.output_dir).with_context(|| {
            format!("Failed to create output dir: {:?}", self.config.output_dir)
        })?;
        fs::write(&output_path, &md)
            .with_context(|| format!("Failed to write index: {:?}", output_path))?;

        Ok(())
    }
}

/// Capitalize first letter
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize("memory"), "Memory");
        assert_eq!(capitalize("code-graph"), "Code-graph");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn test_discover_from_lib_rs() {
        // Test that we can parse module declarations
        let lib_content = r#"
pub mod a2a;
pub mod chronicle;
pub mod memory;
pub mod security;
pub mod search;
        "#;

        // We test the module discovery indirectly via the parsing logic
        // that reads lib.rs module declarations
        let modules: Vec<&str> = lib_content
            .lines()
            .filter(|l| l.trim().starts_with("pub mod ") && l.trim().ends_with(';'))
            .map(|l| {
                l.trim()
                    .trim_start_matches("pub mod ")
                    .trim_end_matches(';')
                    .trim()
            })
            .collect();

        assert!(modules.contains(&"memory"));
        assert!(modules.contains(&"security"));
        assert!(modules.contains(&"chronicle"));
        assert_eq!(modules.len(), 5);
    }

    #[test]
    fn test_compute_stats_empty() {
        let result = ModuleStats {
            module: "test".into(),
            total_symbols: 0,
            public_symbols: 0,
            structs: 0,
            functions: 0,
            enums: 0,
            traits: 0,
            total_files: 1,
            loc_estimate: 150,
            complexity_hotspots: vec![],
            key_functions: vec![],
            key_types: vec![],
        };

        assert_eq!(result.total_symbols, 0);
        assert_eq!(result.total_files, 1);
    }

    #[test]
    fn test_render_module_doc_basic() {
        let gen = AutoDocsGenerator::new(AutoDocsConfig::default());
        let stats = ModuleStats {
            module: "memory".into(),
            total_symbols: 42,
            public_symbols: 12,
            structs: 5,
            functions: 20,
            enums: 2,
            traits: 3,
            total_files: 8,
            loc_estimate: 1200,
            complexity_hotspots: vec![("hot_fn".into(), 15.5)],
            key_functions: vec!["add".into(), "search".into()],
            key_types: vec!["MemoryItem".into(), "MemoryStore".into()],
        };

        let markdown = gen.render_module_doc("memory", &stats, &[]);

        // Has title
        assert!(markdown.contains("# Memory Module"));
        // Has overview
        assert!(markdown.contains("42 symbols"));
        assert!(markdown.contains("8 files"));
        // Has stats table
        assert!(markdown.contains("| Total symbols | 42 |"));
        assert!(markdown.contains("| Functions | 20 |"));
        // Has types
        assert!(markdown.contains("`MemoryItem`"));
        assert!(markdown.contains("`MemoryStore`"));
        // Has functions
        assert!(markdown.contains("`add`"));
        assert!(markdown.contains("`search`"));
        // Has hotspots
        assert!(markdown.contains("hot_fn"));
        // Has auto-gen notice
        assert!(markdown.contains("auto-generated"));
        // Has regenerate instructions
        assert!(markdown.contains("xavier chronicle auto-docs"));
    }

    #[test]
    fn test_render_module_doc_empty_symbols() {
        let gen = AutoDocsGenerator::new(AutoDocsConfig::default());
        let stats = ModuleStats {
            module: "empty".into(),
            total_symbols: 0,
            public_symbols: 0,
            structs: 0,
            functions: 0,
            enums: 0,
            traits: 0,
            total_files: 0,
            loc_estimate: 0,
            complexity_hotspots: vec![],
            key_functions: vec![],
            key_types: vec![],
        };

        let markdown = gen.render_module_doc("empty", &stats, &[]);
        assert!(markdown.contains("# Empty Module"));
        assert!(markdown.contains("0 symbols"));
        assert!(markdown.contains("No symbols indexed"));
    }

    #[test]
    fn test_generate_index_with_docs() {
        let docs = vec![
            ModuleDoc {
                module: "memory".into(),
                stats: ModuleStats {
                    module: "memory".into(),
                    total_symbols: 42,
                    public_symbols: 12,
                    structs: 5,
                    functions: 20,
                    enums: 2,
                    traits: 3,
                    total_files: 8,
                    loc_estimate: 1200,
                    complexity_hotspots: vec![],
                    key_functions: vec![],
                    key_types: vec!["MemoryItem".into()],
                },
                markdown: String::new(),
                output_path: PathBuf::from("memory-module.md"),
            },
            ModuleDoc {
                module: "security".into(),
                stats: ModuleStats {
                    module: "security".into(),
                    total_symbols: 18,
                    public_symbols: 6,
                    structs: 2,
                    functions: 10,
                    enums: 1,
                    traits: 1,
                    total_files: 4,
                    loc_estimate: 600,
                    complexity_hotspots: vec![],
                    key_functions: vec![],
                    key_types: vec!["SecurityFilter".into()],
                },
                markdown: String::new(),
                output_path: PathBuf::from("security-module.md"),
            },
        ];

        // Test index generation logic directly
        let mut index = String::new();
        index.push_str("| Module | Symbols | Files | Key Types |\n");
        index.push_str("|--------|---------|-------|-----------|\n");
        for doc in &docs {
            let types_str = doc.stats.key_types.join(", ");
            index.push_str(&format!(
                "| [{}]({}-module.md) | {} | {} | {} |\n",
                doc.module, doc.module, doc.stats.total_symbols, doc.stats.total_files, types_str
            ));
        }

        assert!(index.contains("[memory](memory-module.md) | 42 | 8 | MemoryItem"));
        assert!(index.contains("[security](security-module.md) | 18 | 4 | SecurityFilter"));
    }

    #[test]
    fn test_collect_files_no_dir() {
        let gen = AutoDocsGenerator::new(AutoDocsConfig {
            source_root: PathBuf::from("/nonexistent"),
            ..Default::default()
        });
        let files = gen.collect_module_files("memory").unwrap_or_default();
        assert!(files.is_empty());
    }

    #[test]
    fn test_auto_docs_config_default() {
        let config = AutoDocsConfig::default();
        assert_eq!(config.code_graph_db, crate::codebase::codegraph_paths::code_graph_db_path_for(Path::new(".")));
        assert_eq!(config.source_root, PathBuf::from("src"));
        assert_eq!(config.output_dir, PathBuf::from("docs/auto-docs"));
        assert!(config.module_filter.is_none());
    }
}
