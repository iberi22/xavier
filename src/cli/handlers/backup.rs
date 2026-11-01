use crate::cli::commands::BackupCommand;
use crate::settings::XavierSettings;
use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub async fn handle_backup_command(cmd: BackupCommand) -> Result<()> {
    match cmd {
        BackupCommand::Create { out, json } => handle_backup_create(out, json).await,
        BackupCommand::Restore { from } => handle_backup_restore(from).await,
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

    let dest_dir = match reserve_destination(&dest_dir, out_explicit) {
        Ok(path) => path,
        Err(e) => {
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
    };

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

                    if is_base_sqlite && base_path.is_file() {
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
                // Reserve a private file before SQLite opens it. Apply stricter
                // source permissions after opening so read-only sources work too.
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                options.mode(0o600);
                options.open(&dest_path)?;
                let mut dest_conn = rusqlite::Connection::open(&dest_path)?;
                #[cfg(unix)]
                fs::set_permissions(
                    &dest_path,
                    fs::Permissions::from_mode(metadata.permissions().mode() & 0o600),
                )?;
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
                drop(backup);
                drop(dest_conn);
                #[cfg(unix)]
                fs::set_permissions(
                    &dest_path,
                    fs::Permissions::from_mode(metadata.permissions().mode() & 0o600),
                )?;
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
            let res: std::io::Result<()> = (|| {
                let mut source = fs::File::open(path)?;
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                options.mode(metadata.permissions().mode() & 0o600);
                let mut destination = options.open(&dest_path)?;
                std::io::copy(&mut source, &mut destination)?;
                Ok(())
            })();
            if let Err(e) = res {
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

// Only the invocation that creates the leaf directory may write into it.
fn reserve_destination(base: &Path, explicit: bool) -> std::io::Result<PathBuf> {
    if let Some(parent) = base.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut candidate = base.to_path_buf();
    let mut suffix = 0u64;
    loop {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        match builder.create(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if explicit {
                    return Err(std::io::Error::new(
                        e.kind(),
                        "Destination directory already exists (not empty or not created by this invocation)",
                    ));
                }
                suffix += 1;
                let mut name = base.as_os_str().to_os_string();
                name.push(format!("-{suffix}"));
                candidate = PathBuf::from(name);
            }
            Err(e) => return Err(e),
        }
    }
}

async fn handle_backup_restore(from: PathBuf) -> Result<()> {
    let data_dir = XavierSettings::resolve_data_dir();

    // Validate backup directory
    if !from.exists() || !from.is_dir() {
        anyhow::bail!("Backup directory does not exist: {}", from.display());
    }

    let manifest_path = from.join("manifest.json");
    if !manifest_path.exists() {
        anyhow::bail!(
            "Invalid backup: manifest.json not found in {}",
            from.display()
        );
    }

    // Read manifest to ensure it is valid json
    let manifest_content = fs::read_to_string(&manifest_path)
        .map_err(|e| anyhow::anyhow!("Failed to read manifest.json: {}", e))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_content)
        .map_err(|e| anyhow::anyhow!("Corrupted backup: invalid manifest.json: {}", e))?;

    let files = manifest
        .get("files")
        .and_then(|f| f.as_array())
        .ok_or_else(|| {
            anyhow::anyhow!("Corrupted backup: missing or invalid 'files' array in manifest")
        })?;

    // Validate files against manifest
    for file_entry in files {
        let rel_path_str = file_entry
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("Corrupted backup: missing or invalid file path in manifest")
            })?;

        let expected_hash = file_entry
            .get("sha256")
            .and_then(|h| h.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Corrupted backup: missing or invalid sha256 for {}",
                    rel_path_str
                )
            })?;

        let backup_file_path = from.join(rel_path_str);
        if !backup_file_path.exists() {
            anyhow::bail!(
                "Corrupted backup: file {} listed in manifest but missing from backup",
                rel_path_str
            );
        }

        if backup_file_path.is_file() {
            let file_content = fs::read(&backup_file_path).map_err(|e| {
                anyhow::anyhow!("Failed to read backup file {}: {}", rel_path_str, e)
            })?;

            let mut hasher = Sha256::new();
            hasher.update(&file_content);
            let actual_hash = hasher
                .finalize()
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();

            if actual_hash != expected_hash {
                anyhow::bail!(
                    "Corrupted backup: hash mismatch for {}. Expected {}, got {}",
                    rel_path_str,
                    expected_hash,
                    actual_hash
                );
            }
        }
    }

    // Wipe current state
    if data_dir.exists() {
        let read_dir = fs::read_dir(&data_dir)
            .map_err(|e| anyhow::anyhow!("Failed to read data directory: {}", e))?;

        for entry in read_dir {
            let entry =
                entry.map_err(|e| anyhow::anyhow!("Failed to read directory entry: {}", e))?;
            let path = entry.path();

            if path.is_file() {
                fs::remove_file(&path).map_err(|e| {
                    anyhow::anyhow!("Failed to remove file {}: {}", path.display(), e)
                })?;
            } else if path.is_dir() {
                if let Err(e) = fs::remove_dir_all(&path) {
                    anyhow::bail!("Failed to remove directory {}: {}", path.display(), e);
                }
            }
        }
    } else {
        fs::create_dir_all(&data_dir)
            .map_err(|e| anyhow::anyhow!("Failed to create data dir: {}", e))?;
    }

    // Restore files
    for file_entry in files {
        let rel_path_str = file_entry.get("path").unwrap().as_str().unwrap();
        let backup_file_path = from.join(rel_path_str);
        let dest_path = data_dir.join(rel_path_str);

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                anyhow::anyhow!("Failed to create directory {}: {}", parent.display(), e)
            })?;
        }

        // Just copy files back.
        if backup_file_path.is_file() {
            fs::copy(&backup_file_path, &dest_path).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to copy file {} to {}: {}",
                    backup_file_path.display(),
                    dest_path.display(),
                    e
                )
            })?;
        }
    }

    // Copy manifest as well for completeness if we want, or not. The task just says "wipe state -> xavier backup restore"

    println!("Backup restored successfully from {}", from.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_destinations_are_exclusive() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("backups").join("20261030T120000Z");
        let first = reserve_destination(&base, false).unwrap();
        fs::write(first.join("sentinel"), "original").unwrap();
        let second = reserve_destination(&base, false).unwrap();
        assert_ne!(first, second);
        assert!(second.is_dir());
        assert_eq!(
            fs::read_to_string(first.join("sentinel")).unwrap(),
            "original"
        );
    }

    #[test]
    fn explicit_destination_rejects_existing_empty_directory() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("output");
        reserve_destination(&base, true).unwrap();
        let error = reserve_destination(&base, true).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_dir(base).unwrap().count(), 0);
    }
}
