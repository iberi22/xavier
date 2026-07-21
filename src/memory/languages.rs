// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Language Family detection for file-type aware filtering
//!
//! Provides utilities to categorize source files into language families
//! to prevent false associations between different programming languages.

use std::path::Path;

/// Categorizes a file path into a language family based on its extension.
pub fn get_language_family(path: &str) -> Option<String> {
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())?;

    match ext.as_str() {
        "py" | "pyw" => Some("python".to_string()),
        "js" | "jsx" | "mjs" | "ejs" | "ts" | "tsx" | "vue" | "svelte" => Some("js".to_string()),
        "go" => Some("go".to_string()),
        "rs" => Some("rust".to_string()),
        "java" | "kt" | "kts" | "scala" => Some("jvm".to_string()),
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" => Some("c".to_string()),
        "rb" => Some("ruby".to_string()),
        "swift" => Some("swift".to_string()),
        "cs" => Some("dotnet".to_string()),
        "php" => Some("php".to_string()),
        "r" => Some("r".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_language_family() {
        assert_eq!(get_language_family("main.py"), Some("python".to_string()));
        assert_eq!(get_language_family("index.ts"), Some("js".to_string()));
        assert_eq!(get_language_family("lib.rs"), Some("rust".to_string()));
        assert_eq!(get_language_family("main.go"), Some("go".to_string()));
        assert_eq!(get_language_family("App.java"), Some("jvm".to_string()));
        assert_eq!(get_language_family("module.rb"), Some("ruby".to_string()));
        assert_eq!(get_language_family("README.md"), None);
    }
}
