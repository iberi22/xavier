//! Obsidian vault import and markdown export handler for Xavier memories.
//!
//! Provides functions to import markdown files, Obsidian vaults (directories or .zip files),
//! frontmatter, tags, and wikilinks into `MemoryRecord` objects, and export workspace memories
//! back into structured markdown files with YAML frontmatter.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::info;
use walkdir::WalkDir;
use zip::ZipArchive;

use crate::memory::store::{stable_key, MemoryRecord, MemoryStore};

/// Structure representing parsed markdown document fields.
#[derive(Debug, Clone, Default)]
pub struct ParsedMarkdownNote {
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub wikilinks: Vec<String>,
    pub body: String,
    pub frontmatter: HashMap<String, serde_json::Value>,
}

/// Parse markdown frontmatter (YAML `---` block), inline tags (`#tag`), and wikilinks (`[[link]]`).
pub fn parse_markdown_content(raw_content: &str) -> ParsedMarkdownNote {
    let mut note = ParsedMarkdownNote::default();
    let trimmed = raw_content.trim_start();

    let mut body_str = raw_content.to_string();

    // 1. Extract YAML Frontmatter if present
    if trimmed.starts_with("---") {
        let lines: Vec<&str> = trimmed.lines().collect();
        if lines.len() > 1 {
            let mut end_idx = None;
            for (i, line) in lines.iter().enumerate().skip(1) {
                if line.trim() == "---" {
                    end_idx = Some(i);
                    break;
                }
            }

            if let Some(end) = end_idx {
                let yaml_str = lines[1..end].join("\n");
                if let Ok(yaml_val) = serde_yaml::from_str::<serde_json::Value>(&yaml_str) {
                    if let Some(obj) = yaml_val.as_object() {
                        for (k, v) in obj {
                            note.frontmatter.insert(k.clone(), v.clone());
                        }

                        // Extract special frontmatter keys
                        if let Some(t) = obj.get("title").and_then(|v| v.as_str()) {
                            note.title = Some(t.to_string());
                        }

                        if let Some(tags_val) = obj.get("tags") {
                            extract_tags_from_value(tags_val, &mut note.tags);
                        }
                    }
                }
                body_str = lines[(end + 1)..].join("\n");
            }
        }
    }

    note.body = body_str.trim().to_string();

    // 2. Derive title if not in frontmatter
    if note.title.is_none() {
        for line in note.body.lines() {
            let line_trim = line.trim();
            if line_trim.starts_with("# ") {
                note.title = Some(line_trim[2..].trim().to_string());
                break;
            }
        }
    }

    // 3. Extract Wikilinks: [[TargetNote]] or [[TargetNote|Display Text]]
    static RE_WIKILINK: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\[\[([^\]\|]+)(?:\|[^\]]+)?\]\]").unwrap());
    for cap in RE_WIKILINK.captures_iter(&note.body) {
        if let Some(matched) = cap.get(1) {
            let target = matched.as_str().trim().to_string();
            if !target.is_empty() && !note.wikilinks.contains(&target) {
                note.wikilinks.push(target);
            }
        }
    }

    // 4. Extract inline hashtags: #tag_name
    static RE_TAG: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?:^|\s)#([a-zA-Z0-9_\-\/]+)").unwrap());
    for cap in RE_TAG.captures_iter(&note.body) {
        if let Some(matched) = cap.get(1) {
            let tag = matched.as_str().trim().to_string();
            // Ignore heading numbers like #1 or pure digits if desired, but keep valid tags
            if !tag.is_empty()
                && !tag.chars().all(|c| c.is_ascii_digit())
                && !note.tags.contains(&tag)
            {
                note.tags.push(tag);
            }
        }
    }

    note
}

fn extract_tags_from_value(val: &serde_json::Value, tags_out: &mut Vec<String>) {
    match val {
        serde_json::Value::String(s) => {
            for t in s.split([',', ' ']) {
                let cleaned = t.trim().trim_start_matches('#').to_string();
                if !cleaned.is_empty() && !tags_out.contains(&cleaned) {
                    tags_out.push(cleaned);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                extract_tags_from_value(item, tags_out);
            }
        }
        _ => {}
    }
}

/// Import markdown vault from a directory or a .zip file.
pub fn parse_markdown_vault(source_path: &Path, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
    let mut records = Vec::new();

    if source_path.is_file()
        && source_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.eq_ignore_ascii_case("zip"))
            .unwrap_or(false)
    {
        // Parse .zip file
        let file = File::open(source_path)
            .with_context(|| format!("Failed to open zip vault at {:?}", source_path))?;
        let mut archive = ZipArchive::new(file)
            .with_context(|| format!("Failed to read zip archive at {:?}", source_path))?;

        for i in 0..archive.len() {
            let mut zip_file = archive.by_index(i)?;
            let name = zip_file.name().to_string();

            if name.ends_with(".md") && !name.starts_with("__MACOSX") && !name.starts_with('.') {
                let mut content = String::new();
                if zip_file.read_to_string(&mut content).is_ok() {
                    if let Some(record) =
                        build_memory_record_from_file(&name, &content, workspace_id)
                    {
                        records.push(record);
                    }
                }
            }
        }
    } else if source_path.is_dir() {
        // Parse directory
        for entry in WalkDir::new(source_path).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.eq_ignore_ascii_case("md"))
                    .unwrap_or(false)
            {
                if let Ok(rel_path) = path.strip_prefix(source_path) {
                    let rel_str = rel_path.to_string_lossy().replace('\\', "/");
                    if !rel_str.starts_with('.') && !rel_str.contains("/.") {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            if let Some(record) =
                                build_memory_record_from_file(&rel_str, &content, workspace_id)
                            {
                                records.push(record);
                            }
                        }
                    }
                }
            }
        }
    } else {
        anyhow::bail!(
            "Source path {:?} is neither a directory nor a zip archive",
            source_path
        );
    }

    Ok(records)
}

fn build_memory_record_from_file(
    rel_path: &str,
    raw_content: &str,
    workspace_id: &str,
) -> Option<MemoryRecord> {
    let parsed = parse_markdown_content(raw_content);

    let title = parsed.title.clone().unwrap_or_else(|| {
        Path::new(rel_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string()
    });

    let mut metadata_map = serde_json::Map::new();
    metadata_map.insert("title".to_string(), json!(title));
    metadata_map.insert("tags".to_string(), json!(parsed.tags));
    metadata_map.insert("wikilinks".to_string(), json!(parsed.wikilinks));
    metadata_map.insert("source".to_string(), json!("obsidian_vault"));

    for (k, v) in parsed.frontmatter {
        if !metadata_map.contains_key(&k) {
            metadata_map.insert(k, v);
        }
    }

    let record_id = metadata_map
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| stable_key("memory", &[workspace_id, rel_path]));

    let created_at = metadata_map
        .get("created_at")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    let updated_at = metadata_map
        .get("updated_at")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or(created_at);

    Some(MemoryRecord {
        id: record_id,
        workspace_id: workspace_id.to_string(),
        path: rel_path.to_string(),
        content: parsed.body,
        metadata: serde_json::Value::Object(metadata_map),
        embedding: Vec::new(),
        created_at,
        updated_at,
        revision: 1,
        primary: true,
        parent_id: None,
        cluster_id: None,
        level: crate::memory::schema::MemoryLevel::Raw,
        relation: None,
        clearance: Default::default(),
        revisions: Vec::new(),
        score: 0.0,
        deleted_at: None,
        embedding_status: "pending".to_string(),
        embedding_attempts: 0,
        ..Default::default()
    })
}

/// Export `MemoryRecord` instances into a markdown vault directory.
pub fn export_markdown_vault(records: &[MemoryRecord], target_dir: &Path) -> Result<usize> {
    std::fs::create_dir_all(target_dir)
        .with_context(|| format!("Failed to create target directory {:?}", target_dir))?;

    let mut exported_count = 0;

    for record in records {
        let mut rel_path = PathBuf::from(&record.path);
        if rel_path.extension().and_then(|e| e.to_str()) != Some("md") {
            rel_path.set_extension("md");
        }

        let out_path = target_dir.join(&rel_path);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut yaml_obj = serde_json::Map::new();
        yaml_obj.insert("id".to_string(), json!(record.id));
        yaml_obj.insert("workspace_id".to_string(), json!(record.workspace_id));
        yaml_obj.insert(
            "created_at".to_string(),
            json!(record.created_at.to_rfc3339()),
        );
        yaml_obj.insert(
            "updated_at".to_string(),
            json!(record.updated_at.to_rfc3339()),
        );

        if let Some(meta) = record.metadata.as_object() {
            for (k, v) in meta {
                yaml_obj.insert(k.clone(), v.clone());
            }
        }

        let yaml_str = serde_yaml::to_string(&serde_json::Value::Object(yaml_obj))
            .unwrap_or_else(|_| String::new());

        let markdown_file_content = format!("---\n{}---\n\n{}", yaml_str, record.content);
        std::fs::write(&out_path, markdown_file_content)
            .with_context(|| format!("Failed to write note to {:?}", out_path))?;

        exported_count += 1;
    }

    Ok(exported_count)
}

/// Handle CLI `xavier memory import-markdown <DIR>` command.
pub async fn handle_import_markdown(
    dir: &Path,
    store: &dyn MemoryStore,
    workspace_id: &str,
) -> Result<()> {
    info!("Importing Markdown vault from {:?}", dir);
    let records = parse_markdown_vault(dir, workspace_id)?;
    let total = records.len();

    for record in records {
        store.put(record).await?;
    }

    println!(
        "✅ Successfully imported {} markdown notes into workspace '{}'",
        total, workspace_id
    );
    Ok(())
}

/// Handle CLI `xavier memory export-markdown <DIR>` command.
pub async fn handle_export_markdown(
    dir: &Path,
    store: &dyn MemoryStore,
    workspace_id: &str,
    public_only: bool,
) -> Result<()> {
    info!("Exporting Markdown vault to {:?}", dir);
    let mut records = store.list(workspace_id).await?;

    if public_only {
        records.retain(|r| {
            let is_private = r
                .metadata
                .get("is_private")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let visibility = r
                .metadata
                .get("visibility")
                .and_then(|v| v.as_str())
                .unwrap_or("public");
            !is_private && visibility != "private"
        });
    }

    let count = export_markdown_vault(&records, dir)?;
    println!(
        "✅ Successfully exported {} memories as Markdown notes to {:?}",
        count, dir
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markdown_frontmatter_and_wikilinks() {
        let content = r#"---
title: "Obsidian Test Note"
tags: ["obsidian", "test"]
category: "architecture"
---

# Obsidian Test Note

This is a test note linking to [[ArchitectureDoc]] and [[UserGuide|User Manual]].

#inline_tag and #xavier/memory
"#;

        let parsed = parse_markdown_content(content);
        assert_eq!(parsed.title.as_deref(), Some("Obsidian Test Note"));
        assert!(parsed.tags.contains(&"obsidian".to_string()));
        assert!(parsed.tags.contains(&"test".to_string()));
        assert!(parsed.tags.contains(&"inline_tag".to_string()));
        assert!(parsed.tags.contains(&"xavier/memory".to_string()));
        assert!(parsed.wikilinks.contains(&"ArchitectureDoc".to_string()));
        assert!(parsed.wikilinks.contains(&"UserGuide".to_string()));
        assert_eq!(
            parsed.frontmatter.get("category").and_then(|v| v.as_str()),
            Some("architecture")
        );
    }
}
