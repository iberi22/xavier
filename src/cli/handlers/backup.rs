use crate::cli::commands::BackupCommand;
use crate::settings::XavierSettings;
use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub async fn handle_backup_command(cmd: BackupCommand) -> Result<()> {
    match cmd {
        BackupCommand::Create { out, json } => handle_backup_create(out, json).await,
    }
}

async fn handle_backup_create(out: Option<PathBuf>, json_output: bool) -> Result<()> {
    let data_dir = XavierSettings::resolve_data_dir();

    let dest_dir = if let Some(out_dir) = out {
        out_dir
    } else {
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        data_dir.join("backups").join(timestamp)
    };

    if !data_dir.exists() {
        if json_output {
            println!(
                "{}",
                json!({"ok": false, "error": "data dir does not exist"})
            );
            std::process::exit(1);
        } else {
            anyhow::bail!("Data directory does not exist: {}", data_dir.display());
        }
    }

    if let Err(e) = fs::create_dir_all(&dest_dir) {
        if json_output {
            println!(
                "{}",
                json!({"ok": false, "error": format!("cannot create output dir: {}", e)})
            );
            std::process::exit(1);
        } else {
            anyhow::bail!("Cannot create output dir: {}", e);
        }
    }

    let data_dir_canon = match fs::canonicalize(&data_dir) {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("Cannot canonicalize data dir {:?}: {}", data_dir, e);
            if json_output {
                println!("{}", json!({"ok": false, "error": msg}));
                std::process::exit(1);
            } else {
                anyhow::bail!("{}", msg);
            }
        }
    };

    let dest_dir_canon = match fs::canonicalize(&dest_dir) {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("Cannot canonicalize output dir {:?}: {}", dest_dir, e);
            if json_output {
                println!("{}", json!({"ok": false, "error": msg}));
                std::process::exit(1);
            } else {
                anyhow::bail!("{}", msg);
            }
        }
    };

    if dest_dir_canon == data_dir_canon || dest_dir_canon.starts_with(&data_dir_canon) {
        let msg = "Output directory cannot be inside the data directory";
        if json_output {
            println!("{}", json!({"ok": false, "error": msg}));
            std::process::exit(1);
        } else {
            anyhow::bail!("{}", msg);
        }
    }

    let mut files_copied = Vec::new();

    let entries = match fs::read_dir(&data_dir) {
        Ok(e) => e,
        Err(e) => {
            if json_output {
                println!(
                    "{}",
                    json!({"ok": false, "error": format!("cannot read data dir: {}", e)})
                );
                std::process::exit(1);
            } else {
                anyhow::bail!("Cannot read data dir: {}", e);
            }
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let file_name = entry.file_name();

        if file_name == "backups" {
            continue;
        }

        let metadata = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if !metadata.is_file() {
            continue;
        }

        let dest_path = dest_dir.join(&file_name);

        let is_sqlite = path.extension().and_then(|e| e.to_str()) == Some("db")
            || path.extension().and_then(|e| e.to_str()) == Some("sqlite3");

        if is_sqlite {
            match rusqlite::Connection::open(&path) {
                Ok(conn) => {
                    let dest_str = dest_path.to_string_lossy().to_string();
                    if let Err(e) = conn.execute("VACUUM INTO ?", rusqlite::params![dest_str]) {
                        if json_output {
                            println!(
                                "{}",
                                json!({"ok": false, "error": format!("failed to vacuum {:?}: {}", path, e)})
                            );
                            std::process::exit(1);
                        } else {
                            anyhow::bail!("Failed to vacuum {:?}: {}", path, e);
                        }
                    }
                }
                Err(e) => {
                    if json_output {
                        println!(
                            "{}",
                            json!({"ok": false, "error": format!("failed to open {:?}: {}", path, e)})
                        );
                        std::process::exit(1);
                    } else {
                        anyhow::bail!("Failed to open {:?}: {}", path, e);
                    }
                }
            }
        } else {
            if let Err(e) = fs::copy(&path, &dest_path) {
                if json_output {
                    println!(
                        "{}",
                        json!({"ok": false, "error": format!("failed to copy {:?}: {}", path, e)})
                    );
                    std::process::exit(1);
                } else {
                    anyhow::bail!("Failed to copy {:?}: {}", path, e);
                }
            }
        }

        let dest_meta = match fs::metadata(&dest_path) {
            Ok(m) => m,
            Err(e) => {
                if json_output {
                    println!(
                        "{}",
                        json!({"ok": false, "error": format!("failed to stat dest {:?}: {}", dest_path, e)})
                    );
                    std::process::exit(1);
                } else {
                    anyhow::bail!("Failed to stat dest {:?}: {}", dest_path, e);
                }
            }
        };

        let file_content = match fs::read(&dest_path) {
            Ok(c) => c,
            Err(e) => {
                if json_output {
                    println!(
                        "{}",
                        json!({"ok": false, "error": format!("failed to read dest {:?}: {}", dest_path, e)})
                    );
                    std::process::exit(1);
                } else {
                    anyhow::bail!("Failed to read dest {:?}: {}", dest_path, e);
                }
            }
        };

        let mut hasher = Sha256::new();
        hasher.update(&file_content);
        let hash = hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        files_copied.push(json!({
            "path": file_name.to_string_lossy().to_string(),
            "size": dest_meta.len(),
            "sha256": hash
        }));
    }

    let manifest = json!({
        "created_at": Utc::now().to_rfc3339(),
        "xavier_version": env!("CARGO_PKG_VERSION"),
        "files": files_copied
    });

    let manifest_path = dest_dir.join("manifest.json");
    if let Err(e) = fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    ) {
        if json_output {
            println!(
                "{}",
                json!({"ok": false, "error": format!("failed to write manifest: {}", e)})
            );
            std::process::exit(1);
        } else {
            anyhow::bail!("Failed to write manifest: {}", e);
        }
    }

    if json_output {
        println!(
            "{}",
            json!({
                "ok": true,
                "path": dest_dir.to_string_lossy().to_string(),
                "files": files_copied.len()
            })
        );
    } else {
        println!(
            "Backup created successfully at {} ({} files)",
            dest_dir.display(),
            files_copied.len()
        );
    }

    Ok(())
}
