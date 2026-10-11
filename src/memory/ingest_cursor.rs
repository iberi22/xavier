//! Persisted ingestion cursor: a versioned JSON file written atomically.
//!
//! Inspired by fsearch (MIT, noahdunnagan/fsearch): a restart only replays
//! what changed since the last saved state, and the state file is replaced
//! with write-to-temp, fsync, rename so a crash never leaves a half-written
//! cursor behind. Callers own the path; this module never picks a location.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::warn;

/// Bump when the serialized state changes shape. A mismatch means "start over".
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum IngestCursorError {
    #[error("ingest cursor io at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("ingest cursor encoding: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("ingest cursor format version {found} is not the supported one")]
    Version { found: u32 },
}

#[derive(Serialize)]
struct EnvelopeRef<'a, T> {
    version: u32,
    state: &'a T,
}

#[derive(Deserialize)]
struct Envelope {
    version: u32,
    state: serde_json::Value,
}

/// Load a saved state. Returns `None` (full pass) when the file is missing,
/// unreadable, corrupt, or written by another format version. Never panics.
pub fn load<T: DeserializeOwned>(path: &Path) -> Option<T> {
    match read_state(path) {
        Ok(state) => Some(state),
        Err(e) => {
            warn!(path = ?path, error = %e, "ingest cursor not usable; running a full pass");
            None
        }
    }
}

fn read_state<T: DeserializeOwned>(path: &Path) -> Result<T, IngestCursorError> {
    let bytes = fs::read(path).map_err(|source| IngestCursorError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let envelope: Envelope = serde_json::from_slice(&bytes)?;
    if envelope.version != FORMAT_VERSION {
        return Err(IngestCursorError::Version {
            found: envelope.version,
        });
    }
    Ok(serde_json::from_value(envelope.state)?)
}

/// Replace `path` with `state`: write `<file>.tmp`, fsync, then rename.
///
/// A crash before the rename leaves the previous cursor intact; a crash after
/// it leaves the new one. Creates the parent directory when missing.
pub fn save<T: Serialize>(path: &Path, state: &T) -> Result<(), IngestCursorError> {
    let body = serde_json::to_vec(&EnvelopeRef {
        version: FORMAT_VERSION,
        state,
    })?;
    let io = |source| IngestCursorError::Io {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    let tmp = path.with_extension("tmp");
    let mut file = File::create(&tmp).map_err(io)?;
    file.write_all(&body).map_err(io)?;
    file.sync_all().map_err(io)?;
    drop(file);
    fs::rename(&tmp, path).map_err(io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Sample {
        watermark_ms: i64,
        seen: BTreeMap<String, i64>,
    }

    fn sample() -> Sample {
        Sample {
            watermark_ms: 1_600_000_123_456,
            seen: BTreeMap::from([("ses_a".to_string(), 7)]),
        }
    }

    #[test]
    fn roundtrip_preserves_state() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("cursors").join("opencode.json");
        save(&path, &sample()).unwrap();
        assert_eq!(load::<Sample>(&path), Some(sample()));
    }

    #[test]
    fn save_is_atomic_and_leaves_no_tmp() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("opencode.json");
        save(&path, &sample()).unwrap();
        save(&path, &Sample::default()).unwrap();
        assert!(!path.with_extension("tmp").exists());
        assert_eq!(load::<Sample>(&path), Some(Sample::default()));
    }

    #[test]
    fn version_mismatch_loads_as_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("opencode.json");
        fs::write(
            &path,
            r#"{"version":999,"state":{"watermark_ms":1,"seen":{}}}"#,
        )
        .unwrap();
        assert_eq!(load::<Sample>(&path), None);
    }

    #[test]
    fn missing_corrupt_or_mistyped_loads_as_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("opencode.json");
        assert_eq!(load::<Sample>(&path), None, "missing");
        fs::write(&path, "{not json").unwrap();
        assert_eq!(load::<Sample>(&path), None, "corrupt");
        fs::write(&path, r#"{"version":1,"state":"wrong shape"}"#).unwrap();
        assert_eq!(load::<Sample>(&path), None, "mistyped state");
        fs::create_dir(dir.path().join("as_dir")).unwrap();
        assert_eq!(
            load::<Sample>(&dir.path().join("as_dir")),
            None,
            "directory"
        );
    }
}
