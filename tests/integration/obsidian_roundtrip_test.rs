//! Integration roundtrip test for Obsidian vault import and markdown export.

use anyhow::Result;
use tempfile::tempdir;
use xavier::cli::handlers::export::{
    export_markdown_vault, parse_markdown_vault,
};
use xavier::memory::store::{InMemoryMemoryStore, MemoryStore};

#[tokio::test]
async fn test_obsidian_markdown_import_export_roundtrip() -> Result<()> {
    let workspace_id = "test_obsidian_workspace";
    let store = InMemoryMemoryStore::new();

    // 1. Create a temporary source vault directory with markdown files
    let source_dir = tempdir()?;
    let note1_path = source_dir.path().join("Note1.md");
    let note2_sub_dir = source_dir.path().join("subfolder");
    std::fs::create_dir_all(&note2_sub_dir)?;
    let note2_path = note2_sub_dir.join("Note2.md");

    let note1_content = r#"---
title: "First Test Note"
tags: ["rust", "xavier"]
author: "Bela"
---

# First Test Note

This is content for note 1 linking to [[subfolder/Note2|Second Note]].

#inline_tag
"#;

    let note2_content = r#"---
title: "Second Test Note"
tags: ["integration", "vault"]
---

This is content for note 2 with a wikilink to [[Note1]].
"#;

    std::fs::write(&note1_path, note1_content)?;
    std::fs::write(&note2_path, note2_content)?;

    // 2. Import markdown vault into MemoryStore
    let imported_records = parse_markdown_vault(source_dir.path(), workspace_id)?;
    assert_eq!(imported_records.len(), 2);

    for record in &imported_records {
        store.put(record.clone()).await?;
    }

    // Verify imported records metadata
    let store_list = store.list(workspace_id).await?;
    assert_eq!(store_list.len(), 2);

    let note1_record = store_list
        .iter()
        .find(|r| r.path.contains("Note1.md"))
        .expect("Note1 record found");
    assert_eq!(
        note1_record.metadata.get("title").and_then(|v| v.as_str()),
        Some("First Test Note")
    );
    let note1_tags = note1_record
        .metadata
        .get("tags")
        .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
        .unwrap_or_default();
    assert!(note1_tags.contains(&"rust".to_string()));
    assert!(note1_tags.contains(&"xavier".to_string()));
    assert!(note1_tags.contains(&"inline_tag".to_string()));

    // 3. Export memories to a new target directory
    let export_dir = tempdir()?;
    let exported_count = export_markdown_vault(&store_list, export_dir.path())?;
    assert_eq!(exported_count, 2);

    // 4. Import back from the exported vault to verify roundtrip fidelity
    let reimported_records = parse_markdown_vault(export_dir.path(), workspace_id)?;
    assert_eq!(reimported_records.len(), 2);

    let reimported_note1 = reimported_records
        .iter()
        .find(|r| r.path.contains("Note1.md"))
        .expect("Reimported Note1 record found");

    assert_eq!(reimported_note1.content, note1_record.content);
    assert_eq!(
        reimported_note1.metadata.get("title").and_then(|v| v.as_str()),
        Some("First Test Note")
    );

    let reimported_tags = reimported_note1
        .metadata
        .get("tags")
        .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
        .unwrap_or_default();
    assert!(reimported_tags.contains(&"rust".to_string()));
    assert!(reimported_tags.contains(&"xavier".to_string()));
    assert!(reimported_tags.contains(&"inline_tag".to_string()));

    Ok(())
}
