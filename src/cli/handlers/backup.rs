use crate::cli::commands::BackupCommand;
use crate::settings::XavierSettings;
use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

pub async fn handle_backup_command(cmd: BackupCommand) -> Result<()> {
    match cmd {
        BackupCommand::Create { out, json } => handle_backup_create(out, json).await,
    }
}

async fn handle_backup_create(out: Option<PathBuf>, json_output: bool) -> Result<()> {
    let data_dir = XavierSettings::resolve_data_dir();

    let (dest_dir, out_explicit) = if let Some(out_dir) = out {
        (out_dir, true)
    } else {
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        (data_dir.join("backups").join(timestamp), false)
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

    if dest_dir.exists() {
        // An unreadable destination must not be treated as empty: we would
        // overwrite existing backup files and the manifest without knowing.
        let is_empty = match fs::read_dir(&dest_dir) {
            Ok(mut d) => d.next().is_none(),
            Err(e) => {
                if json_output {
                    println!(
                        "{}",
                        json!({"ok": false, "error": format!("cannot inspect destination directory: {}", e)})
                    );
                    std::process::exit(1);
                } else {
                    anyhow::bail!(
                        "Cannot inspect destination directory {}: {}",
                        dest_dir.display(),
                        e
                    );
                }
            }
        };
        if !is_empty {
            if json_output {
                println!(
                    "{}",
                    json!({"ok": false, "error": "destination directory is not empty"})
                );
                std::process::exit(1);
            } else {
                anyhow::bail!("Destination directory is not empty");
            }
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

    if out_explicit
        && (dest_dir_canon == data_dir_canon || dest_dir_canon.starts_with(&data_dir_canon))
    {
        let msg = "Output directory cannot be inside the data directory";
        if json_output {
            println!("{}", json!({"ok": false, "error": msg}));
            std::process::exit(1);
        } else {
            anyhow::bail!("{}", msg);
        }
    }

    let mut files_copied = Vec::new();

    let walker = WalkDir::new(&data_dir).into_iter().filter_entry(|e| {
        let p = e.path();
        if p == dest_dir {
            return false;
        }
        if let Ok(rel) = p.strip_prefix(&data_dir) {
            if rel.components().next().map(|c| c.as_os_str())
                == Some(std::ffi::OsStr::new("backups"))
            {
                return false;
            }
        }
        true
    });

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                if json_output {
                    println!(
                        "{}",
                        json!({"ok": false, "error": format!("failed to read directory entry: {}", e)})
                    );
                    std::process::exit(1);
                } else {
                    anyhow::bail!("Failed to read directory entry: {}", e);
                }
            }
        };

        let path = entry.path();

        let rel_path = match path.strip_prefix(&data_dir) {
            Ok(p) => p,
            Err(_) => continue,
        };

        if rel_path.as_os_str() == "manifest.json" {
            let msg =
                "Source directory contains manifest.json which conflicts with backup manifest";
            if json_output {
                println!("{}", json!({"ok": false, "error": msg}));
                std::process::exit(1);
            } else {
                anyhow::bail!("{}", msg);
            }
        }

        let metadata = match fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                if json_output {
                    println!(
                        "{}",
                        json!({"ok": false, "error": format!("failed to read metadata for {:?}: {}", path, e)})
                    );
                    std::process::exit(1);
                } else {
                    anyhow::bail!("Failed to read metadata for {:?}: {}", path, e);
                }
            }
        };

        if !metadata.is_file() {
            continue;
        }

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if let Some(parent) = path.parent() {
                if let Some(base) = name
                    .strip_suffix("-wal")
                    .or_else(|| name.strip_suffix("-shm"))
                    .or_else(|| name.strip_suffix("-journal"))
                {
                    let base_path = parent.join(base);
                    let is_base_sqlite = base_path.extension().and_then(|e| e.to_str())
                        == Some("db")
                        || base_path.extension().and_then(|e| e.to_str()) == Some("sqlite3");

                    if is_base_sqlite && base_path.exists() {
                        continue;
                    }
                }
            }
        }

        let dest_path = dest_dir.join(rel_path);

        if let Some(parent) = dest_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                if json_output {
                    println!(
                        "{}",
                        json!({"ok": false, "error": format!("cannot create output dir {:?}: {}", parent, e)})
                    );
                    std::process::exit(1);
                } else {
                    anyhow::bail!("Cannot create output dir {:?}: {}", parent, e);
                }
            }
        }

        let is_sqlite = path.extension().and_then(|e| e.to_str()) == Some("db")
            || path.extension().and_then(|e| e.to_str()) == Some("sqlite3");

        if is_sqlite {
            let res: anyhow::Result<()> = (|| {
                let src_conn = rusqlite::Connection::open_with_flags(
                    path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                        | rusqlite::OpenFlags::SQLITE_OPEN_URI,
                )?;
                let mut dest_conn = rusqlite::Connection::open(&dest_path)?;
                let backup = rusqlite::backup::Backup::new(&src_conn, &mut dest_conn)?;
                let mut retries = 0;
                let mut success = false;
                loop {
                    match backup.step(-1)? {
                        rusqlite::backup::StepResult::Done => {
                            success = true;
                            break;
                        }
                        rusqlite::backup::StepResult::More => continue,
                        rusqlite::backup::StepResult::Busy
                        | rusqlite::backup::StepResult::Locked => {
                            if retries >= 10 {
                                break;
                            }
                            retries += 1;
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        _ => {
                            anyhow::bail!("Unexpected backup step result");
                        }
                    }
                }
                if !success {
                    anyhow::bail!("Database is locked or busy");
                }
                Ok(())
            })();
            if let Err(e) = res {
                if json_output {
                    println!(
                        "{}",
                        json!({"ok": false, "error": format!("failed to backup sqlite {:?}: {}", path, e)})
                    );
                    std::process::exit(1);
                } else {
                    anyhow::bail!("Failed to backup sqlite {:?}: {}", path, e);
                }
            }
        } else {
            if let Err(e) = fs::copy(path, &dest_path) {
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
            "path": rel_path.to_string_lossy().to_string(),
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
