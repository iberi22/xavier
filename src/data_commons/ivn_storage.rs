//! Shared IVN storage paths and private, atomic JSON persistence.

use serde::{de::DeserializeOwned, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Resolve the IVN directory independently of the process working directory.
pub fn ivn_root() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("XAVIER_DATA_DIR") {
        return Ok(PathBuf::from(root).join("ivn"));
    }
    dirs::data_dir()
        .map(|root| root.join("xavier").join("ivn"))
        .ok_or_else(|| "Cannot resolve IVN storage: no data directory available".to_string())
}

/// Missing stores begin empty; malformed or unreadable stores are errors.
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("Failed to parse {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(format!("Failed to read {}: {error}", path.display())),
    }
}

/// Publish a complete JSON document by renaming a private sibling temporary file.
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Failed to serialize {}: {error}", path.display()))?;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(parent)
        .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    let temporary = parent.join(format!(".ivn-{}.tmp", ulid::Ulid::new()));
    let result = (|| -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("Failed to persist {}: {error}", path.display()));
    }
    Ok(())
}
