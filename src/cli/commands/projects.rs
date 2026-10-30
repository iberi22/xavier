use crate::agents::RuntimeConfig;
use crate::cli::commands::enums::ProjectsCommand;
use crate::settings::serialization::resolve_data_dir;
use anyhow::Result;
use serde_json::json;

/// Filter out scratch/temp projects from the list based on specific patterns.
pub fn filter_projects(
    projects: std::collections::BTreeMap<String, usize>,
) -> (std::collections::BTreeMap<String, usize>, usize) {
    let mut filtered_projects = std::collections::BTreeMap::new();
    let mut filtered_count = 0;

    for (name, count) in projects {
        if name.is_empty()
            || name.contains("/tmp/")
            || name.contains("/nix/store/")
            || name.ends_with("-tmp")
            || name.starts_with(std::env::temp_dir().to_string_lossy().as_ref())
        {
            filtered_count += 1;
        } else {
            filtered_projects.insert(name, count);
        }
    }

    (filtered_projects, filtered_count)
}

pub async fn handle_projects_command(cmd: ProjectsCommand) -> Result<()> {
    match cmd {
        ProjectsCommand::List { json, all } => {
            let data_dir = resolve_data_dir();
            let runtime_config = RuntimeConfig::default();
            // We use offline memory store for the CLI
            let workspace = crate::workspace::WorkspaceState::new(
                crate::workspace::WorkspaceConfig {
                    id: "default".to_string(),
                    token: "cli-projects-list-token".to_string(),
                    plan: crate::workspace::PlanTier::Personal,
                    memory_backend: crate::memory::store::MemoryBackend::Vec,
                    storage_limit_bytes: None,
                    request_limit: None,
                    request_unit_limit: None,
                    embedding_provider_mode: crate::workspace::EmbeddingProviderMode::BringYourOwn,
                    managed_google_embeddings: false,
                    sync_policy: crate::workspace::SyncPolicy::CloudMirror,
                    protocol: Default::default(),
                    dedup: crate::settings::types::DedupSettings::default(),
                },
                runtime_config,
                data_dir,
            )
            .await?;

            let store = workspace.durable_store();
            let mut projects = store.project_counts("default").await?;
            let mut filtered = 0;

            if !all {
                let (fp, fc) = filter_projects(projects);
                projects = fp;
                filtered = fc;
            }

            if json {
                let projects_json = projects
                    .into_iter()
                    .map(|(name, memory_count)| json!({"name": name, "memory_count": memory_count}))
                    .collect::<Vec<_>>();

                let output = json!({
                    "projects": projects_json,
                    "filtered": filtered
                });
                println!("{}", serde_json::to_string(&output)?);
            } else {
                if projects.is_empty() {
                    println!("No projects found.");
                } else {
                    println!("{:<50} | Memories", "Project");
                    println!("{:-<50}-|-{:-<10}", "", "");
                    for (name, count) in projects {
                        println!("{:<50} | {}", name, count);
                    }
                    if filtered > 0 {
                        println!(
                            "\n{} scratch/temp projects hidden (use --all to show).",
                            filtered
                        );
                    }
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_filter_projects() {
        let mut projects = BTreeMap::new();
        projects.insert("my-project".to_string(), 10);
        projects.insert("another_project".to_string(), 5);
        projects.insert("".to_string(), 1);
        projects.insert("/tmp/scratch".to_string(), 2);
        projects.insert("/nix/store/something".to_string(), 3);
        projects.insert("project-tmp".to_string(), 4);

        let temp_dir_proj = format!("{}/some-temp", std::env::temp_dir().to_string_lossy());
        projects.insert(temp_dir_proj, 5);

        let (filtered, count) = filter_projects(projects);

        assert_eq!(filtered.len(), 2);
        assert_eq!(count, 5);
        assert!(filtered.contains_key("my-project"));
        assert!(filtered.contains_key("another_project"));
    }
}
