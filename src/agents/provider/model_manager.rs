// SPDX-License-Identifier: MIT OR LICENSE-MESH
use std::path::Path;
use std::sync::OnceLock;
use regex::Regex;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

/// Represents a local model discovered on the system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalModel {
    /// The name of the model (usually the filename).
    pub name: String,
    /// The full file path on the system.
    pub path: String,
    /// Size of the model file in bytes.
    pub size_bytes: u64,
    /// Detected quantization type (e.g. "Q4_K_M", "Q8_0", etc.).
    pub quantization: Option<String>,
}

/// Discovers compatibly named `.gguf` models in the specified directories.
///
/// Scans each directory recursively. If a directory does not exist or is not
/// a valid directory, it is skipped.
pub async fn scan_local_models(directories: &[String]) -> Vec<LocalModel> {
    let dirs: Vec<String> = directories.to_vec();
    tokio::task::spawn_blocking(move || {
        let mut models = Vec::new();
        for dir_str in dirs {
            let path = Path::new(&dir_str);
            if !path.exists() || !path.is_dir() {
                continue;
            }
            for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    if let Some(ext) = entry.path().extension() {
                        if ext.to_string_lossy().to_ascii_lowercase() == "gguf" {
                            let file_name = entry.file_name().to_string_lossy().into_owned();
                            let full_path = entry.path().to_string_lossy().into_owned();
                            let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                            let quantization = extract_quantization(&file_name);
                            models.push(LocalModel {
                                name: file_name,
                                path: full_path,
                                size_bytes,
                                quantization,
                            });
                        }
                    }
                }
            }
        }
        models
    })
    .await
    .unwrap_or_default()
}

/// Helper function to extract quantization patterns from a filename.
pub fn extract_quantization(filename: &str) -> Option<String> {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    let re = REGEX.get_or_init(|| {
        Regex::new(r"(?i)\b(q[2-9]_k_[sml]|q[2-9]_[0-9a-zA-Z_]+|q[2-9]|f16|bf16|fp16|fp32)\b")
            .unwrap()
    });
    re.find(filename).map(|m| m.as_str().to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, File};
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_extract_quantization() {
        assert_eq!(extract_quantization("llama-3-8b-Instruct-Q4_K_M.gguf").as_deref(), Some("Q4_K_M"));
        assert_eq!(extract_quantization("Llama3.Q8_0.gguf").as_deref(), Some("Q8_0"));
        assert_eq!(extract_quantization("mistral-7b-v0.1.Q2_K.gguf").as_deref(), Some("Q2_K"));
        assert_eq!(extract_quantization("phi3-mini-fp16.gguf").as_deref(), Some("FP16"));
        assert_eq!(extract_quantization("no-quantization.gguf"), None);
    }

    #[tokio::test]
    async fn test_scan_local_models() {
        let dir = tempdir().unwrap();
        let base_path = dir.path();

        // Create nested subdirectories
        let sub_dir1 = base_path.join("models1");
        let sub_dir2 = base_path.join("models2");
        create_dir_all(&sub_dir1).unwrap();
        create_dir_all(&sub_dir2).unwrap();

        // Create some mock gguf and non-gguf files
        let file_path_1 = sub_dir1.join("llama-q4_k_m.gguf");
        let mut f1 = File::create(&file_path_1).unwrap();
        f1.write_all(b"dummy gguf content").unwrap();

        let file_path_2 = sub_dir2.join("phi-3.Q8_0.GGUF");
        let mut f2 = File::create(&file_path_2).unwrap();
        f2.write_all(b"dummy long gguf content here").unwrap();

        let file_path_3 = sub_dir1.join("not_a_model.txt");
        let mut f3 = File::create(&file_path_3).unwrap();
        f3.write_all(b"not gguf").unwrap();

        // Scan directories
        let scanned = scan_local_models(&[
            sub_dir1.to_string_lossy().to_string(),
            sub_dir2.to_string_lossy().to_string(),
        ])
        .await;

        assert_eq!(scanned.len(), 2);

        let model1 = scanned.iter().find(|m| m.name == "llama-q4_k_m.gguf").unwrap();
        assert_eq!(model1.size_bytes, 18);
        assert_eq!(model1.quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(model1.path, file_path_1.to_string_lossy().to_string());

        let model2 = scanned.iter().find(|m| m.name == "phi-3.Q8_0.GGUF").unwrap();
        assert_eq!(model2.size_bytes, 28);
        assert_eq!(model2.quantization.as_deref(), Some("Q8_0"));
        assert_eq!(model2.path, file_path_2.to_string_lossy().to_string());
    }

    #[tokio::test]
    async fn test_scan_non_existent_directory() {
        let scanned = scan_local_models(&["/non/existent/path/for/sure/12345".to_string()]).await;
        assert!(scanned.is_empty());
    }
}
