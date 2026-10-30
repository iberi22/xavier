use crate::cli::commands::enums::ProjectsCommand;
use anyhow::Result;
use rusqlite::OpenFlags;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct ProjectListOutput {
    projects: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filtered: Option<usize>,
}

pub fn run_projects_command(cmd: &ProjectsCommand) -> Result<()> {
    match cmd {
        ProjectsCommand::List { json, all } => {
            let db_path = xavier::memory::sqlite_vec_store::VecSqliteStoreConfig::from_env().path;

            // If the database doesn't exist, just print an empty list
            if !db_path.exists() {
                if *json {
                    println!(
                        "{}",
                        serde_json::to_string(&ProjectListOutput {
                            projects: vec![],
                            filtered: None
                        })?
                    );
                }
                return Ok(());
            }

            let temp_dir = std::env::temp_dir();
            let (projects, filtered) = list_projects(&db_path, &temp_dir, *all)?;

            if *json {
                let out = ProjectListOutput {
                    projects,
                    filtered: if *all { None } else { Some(filtered) },
                };
                println!("{}", serde_json::to_string(&out)?);
            } else {
                if projects.is_empty() {
                    if !*all && filtered > 0 {
                        println!(
                            "No active projects found. ({} hidden temp projects)",
                            filtered
                        );
                    } else {
                        println!("No projects found.");
                    }
                } else {
                    for proj in &projects {
                        println!("{}", proj);
                    }
                    if !*all && filtered > 0 {
                        println!("({} hidden temp projects)", filtered);
                    }
                }
            }
        }
    }
    Ok(())
}

fn list_projects(db_path: &Path, temp_dir: &Path, all: bool) -> Result<(Vec<String>, usize)> {
    let conn = rusqlite::Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    // Check if memory_records exists
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='memory_records'")?;
    if !stmt.exists([])? {
        return Ok((vec![], 0));
    }

    let mut stmt = conn.prepare(
        "SELECT DISTINCT workspace_id FROM memory_records WHERE workspace_id IS NOT NULL",
    )?;
    let mut rows = stmt.query([])?;

    let mut projects = Vec::new();
    let mut filtered_count = 0;

    while let Some(row) = rows.next()? {
        let ws: String = row.get(0)?;

        let mut is_temp = false;
        let ws_path = Path::new(&ws);

        if ws_path.is_absolute() {
            is_temp = ws_path.starts_with(temp_dir);
        } else {
            // For safety, just checking if it is literally /tmp/...
            if ws.starts_with("/tmp/") || ws.starts_with("/tmp-") {
                // handle naive cases
                if ws_path.starts_with(temp_dir) {
                    is_temp = true;
                }
            }
        }
        // Strict start_with for the temp dir
        is_temp = ws_path.starts_with(temp_dir);

        if !all && is_temp {
            filtered_count += 1;
            continue;
        }

        projects.push(ws);
    }

    projects.sort();
    Ok((projects, filtered_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_project_filtering() {
        let db_dir = TempDir::new().unwrap();
        let db_path = db_dir.path().join("test.db");

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute("CREATE TABLE memory_records (workspace_id TEXT)", [])
            .unwrap();

        let temp_dir_path = PathBuf::from("/tmp");

        // Insert dummy data
        conn.execute(
            "INSERT INTO memory_records (workspace_id) VALUES ('/tmp/scratch1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO memory_records (workspace_id) VALUES ('/tmp-project')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO memory_records (workspace_id) VALUES ('/home/user/real_project')",
            [],
        )
        .unwrap();

        // Default output (all = false)
        let (projects, filtered) = list_projects(&db_path, &temp_dir_path, false).unwrap();
        assert_eq!(projects, vec!["/home/user/real_project", "/tmp-project"]);
        assert_eq!(filtered, 1);

        // All output (all = true)
        let (projects_all, _) = list_projects(&db_path, &temp_dir_path, true).unwrap();
        assert_eq!(
            projects_all,
            vec!["/home/user/real_project", "/tmp-project", "/tmp/scratch1"]
        );
    }
}
