//! Formats code context as Markdown for LLMs

use crate::impact::ImpactResult;
use crate::types::Symbol;
use std::collections::HashMap;

pub struct ContextBuilder {
    max_chars: usize,
    stale_files: Vec<String>,
}

impl ContextBuilder {
    pub fn new(max_chars: usize, stale_files: Vec<String>) -> Self {
        Self {
            max_chars,
            stale_files,
        }
    }

    pub fn build_surgical_context(
        &self,
        symbols: Vec<Symbol>,
        impact_analyses: Vec<ImpactResult>,
    ) -> String {
        let mut output = String::new();

        // 1. Staleness Warning
        if !self.stale_files.is_empty() {
            output.push_str("⚠️ **WARNING**: The following files have changed since the last index and results may be stale:\n");
            for file in &self.stale_files {
                output.push_str(&format!("- {}\n", file));
            }
            output.push_str("\n---\n\n");
        }

        // 2. Group symbols by file
        let mut files: HashMap<String, Vec<&Symbol>> = HashMap::new();
        for sym in &symbols {
            files.entry(sym.file_path.clone()).or_default().push(sym);
        }

        output.push_str("# Codebase Context\n\n");

        for (file_path, syms) in files {
            output.push_str(&format!("## File: `{}`\n\n", file_path));
            for sym in syms {
                output.push_str(&format!("### {} `{}`\n", format!("{:?}", sym.kind), sym.name));
                if let Some(ref sig) = sym.signature {
                    let lang_highlight = match sym.lang {
                        crate::types::Language::Rust => "rust",
                        crate::types::Language::TypeScript => "typescript",
                        crate::types::Language::JavaScript => "javascript",
                        crate::types::Language::Python => "python",
                        crate::types::Language::Go => "go",
                        crate::types::Language::Java => "java",
                        crate::types::Language::C => "c",
                        crate::types::Language::Cpp => "cpp",
                        crate::types::Language::Unknown => "",
                    };
                    output.push_str(&format!("```{}\n", lang_highlight));
                    output.push_str(sig);
                    output.push_str("\n```\n");
                }
                output.push_str(&format!("Lines: {}-{}\n\n", sym.start_line, sym.end_line));
            }
        }

        // 3. Impact Analysis
        if !impact_analyses.is_empty() {
            output.push_str("# Impact Analysis\n\n");
            for impact in impact_analyses {
                output.push_str(&format!("## Symbol: `{}`\n", impact.symbol.name));

                if !impact.callers.is_empty() {
                    output.push_str("### ⬆️ Callers (Backward Impact)\n");
                    for caller in &impact.callers {
                        output.push_str(&format!("- `{}` (depth: {}) in `{}`\n",
                            caller.symbol.name, caller.depth, caller.symbol.file_path));
                    }
                    output.push_str("\n");
                }

                if !impact.callees.is_empty() {
                    output.push_str("### ⬇️ Callees (Forward Impact)\n");
                    for callee in &impact.callees {
                        output.push_str(&format!("- `{}` (depth: {}) in `{}`\n",
                            callee.symbol.name, callee.depth, callee.symbol.file_path));
                    }
                    output.push_str("\n");
                }
            }
        }

        // Truncate to max_chars safely
        if output.chars().count() > self.max_chars {
            let mut truncated: String = output.chars().take(self.max_chars).collect();
            truncated.push_str("\n\n... (context truncated due to token budget) ...");
            truncated
        } else {
            output
        }
    }
}
