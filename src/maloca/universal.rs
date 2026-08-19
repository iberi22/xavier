//! Universal SWAL backlog features scanner.
//! Reads features from all registered repositories.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SwalFeature {
    pub id: String,
    pub name: String,
    pub progress_pct: f64,
    pub status: String,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SwalProject {
    pub repo_name: String,
    pub features: Vec<SwalFeature>,
    pub decisions_count: u64,
    pub support_open: u64,
    pub inbox_open: u64,
}

/// Dynamic projects scanner. Scans the specified base directory for projects containing `.gitcore/features.json`.
pub fn scan_projects_from_dir(base_dir: &Path) -> Vec<SwalProject> {
    let mut projects = Vec::new();

    if let Ok(entries) = std::fs::read_dir(base_dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                let repo_name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(name) => name.to_string(),
                    None => continue,
                };

                let features_json_path = path.join(".gitcore").join("features.json");
                if features_json_path.is_file() {
                    if let Ok(content) = std::fs::read_to_string(&features_json_path) {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                            let mut features = Vec::new();
                            if let Some(arr) = parsed.get("features").and_then(|f| f.as_array()) {
                                for item in arr {
                                    if let Ok(feat) =
                                        serde_json::from_value::<SwalFeature>(item.clone())
                                    {
                                        features.push(feat);
                                    }
                                }
                            }

                            let metadata = parsed.get("metadata");
                            let decisions_count = metadata
                                .and_then(|m| m.get("decisions_count"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let support_open = metadata
                                .and_then(|m| m.get("support_open"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let inbox_open = metadata
                                .and_then(|m| m.get("inbox_open"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);

                            projects.push(SwalProject {
                                repo_name,
                                features,
                                decisions_count,
                                support_open,
                                inbox_open,
                            });
                        }
                    }
                }
            }
        }
    }

    projects.sort_by(|a, b| a.repo_name.cmp(&b.repo_name));
    projects
}

/// Scan all projects under the canonical SWAL workspace directory `/home/belal/proyectosSWAL`.
pub fn scan_projects() -> Vec<SwalProject> {
    scan_projects_from_dir(Path::new("/home/belal/proyectosSWAL"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};
    use tempfile::tempdir;

    #[test]
    fn test_scan_projects_empty() {
        let tmp = tempdir().unwrap();
        let projects = scan_projects_from_dir(tmp.path());
        assert!(projects.is_empty());
    }

    #[test]
    fn test_scan_projects_with_features() {
        let tmp = tempdir().unwrap();
        let repo_dir = tmp.path().join("test-repo");
        let gitcore_dir = repo_dir.join(".gitcore");
        create_dir_all(&gitcore_dir).unwrap();

        let features_json = r#"{
            "protocol": "3.8.0",
            "metadata": {
                "project": "TestRepo",
                "decisions_count": 5,
                "support_open": 2,
                "inbox_open": 1
            },
            "features": [
                {
                    "id": "feat-1",
                    "name": "Feature One",
                    "progress_pct": 50.0,
                    "status": "draft",
                    "notes": "Work in progress"
                },
                {
                    "id": "feat-2",
                    "name": "Feature Two",
                    "progress_pct": 100.0,
                    "status": "stable"
                }
            ]
        }"#;

        write(gitcore_dir.join("features.json"), features_json).unwrap();

        let projects = scan_projects_from_dir(tmp.path());
        assert_eq!(projects.len(), 1);
        let proj = &projects[0];
        assert_eq!(proj.repo_name, "test-repo");
        assert_eq!(proj.decisions_count, 5);
        assert_eq!(proj.support_open, 2);
        assert_eq!(proj.inbox_open, 1);
        assert_eq!(proj.features.len(), 2);

        let f1 = &proj.features[0];
        assert_eq!(f1.id, "feat-1");
        assert_eq!(f1.name, "Feature One");
        assert_eq!(f1.progress_pct, 50.0);
        assert_eq!(f1.status, "draft");
        assert_eq!(f1.notes.as_deref(), Some("Work in progress"));

        let f2 = &proj.features[1];
        assert_eq!(f2.id, "feat-2");
        assert_eq!(f2.progress_pct, 100.0);
        assert_eq!(f2.status, "stable");
        assert!(f2.notes.is_none());
    }
}
