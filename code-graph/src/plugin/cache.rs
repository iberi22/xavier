//! Local plugin cache.
//!
//! Extracted plugin archives live under `<cache_dir>/xavier/plugins/<name>/<version>/`.
//! `cache_dir` resolves to the platform-appropriate location via [`dirs::cache_dir`]
//! (e.g. `~/.cache` on Linux, `%LOCALAPPDATA%` on Windows) — deliberately *not*
//! a hardcoded `~/.xavier/plugins/`, because the original spec's NTFS-on-WSL
//! path caused SQLite "disk I/O errors" (see `.gitcore/features/FEATURE-plugin-system.md`).
//!
//! Supported archive formats: `tar.gz`/`tgz` (via flate2 + tar) and `zip`.
//! The `zip` crate is built with `default-features = false, features = ["deflate"]`
//! to avoid an liblzma-sys linkage conflict with the xavier burn stack.

use crate::error::{GraphError, Result};
use semver::Version;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// Default number of versions kept per plugin by [`PluginCache::prune`].
pub const DEFAULT_KEEP: usize = 3;

/// Local on-disk store of extracted plugin versions.
pub struct PluginCache {
    base_dir: PathBuf,
}

impl PluginCache {
    /// Use the platform cache dir (`~/.cache` / `%LOCALAPPDATA%`) under
    /// `xavier/plugins/`. Falls back to `./.xavier/plugins` if the platform
    /// exposes no cache dir (rare).
    pub fn default_root() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("xavier")
            .join("plugins")
    }

    pub fn new() -> Self {
        Self {
            base_dir: Self::default_root(),
        }
    }

    /// Construct a cache rooted at an explicit directory (tests / overrides).
    pub fn with_root(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Directory holding all extracted versions of `name`.
    fn plugin_dir(&self, name: &str) -> PathBuf {
        self.base_dir.join(name)
    }

    /// Directory holding version `version` of `name`.
    pub fn version_dir(&self, name: &str, version: &str) -> PathBuf {
        self.plugin_dir(name).join(version)
    }

    /// Extract `archive` into `<base>/<name>/<version>/` and return that path.
    ///
    /// Re-extracting over an existing version dir is allowed (it's cleared first).
    pub fn store(&self, name: &str, version: &str, archive: &Path) -> Result<PathBuf> {
        let dest = self.version_dir(name, version);
        if dest.exists() {
            debug!(?dest, "clearing existing plugin version dir");
            fs::remove_dir_all(&dest).map_err(GraphError::Io)?;
        }
        fs::create_dir_all(&dest).map_err(GraphError::Io)?;
        self.extract(archive, &dest)?;
        Ok(dest)
    }

    /// Extract a `tar.gz`/`tgz` or `zip` archive into `dest`.
    pub fn extract(&self, archive: &Path, dest: &Path) -> Result<()> {
        let ext = archive.extension().and_then(|e| e.to_str()).unwrap_or("");
        let name_lower = archive
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "gz" || ext == "tgz" || name_lower.ends_with(".tar.gz") {
            self.extract_tar_gz(archive, dest)
        } else if ext == "zip" {
            self.extract_zip(archive, dest)
        } else {
            Err(GraphError::Plugin(format!(
                "unsupported archive format: {}",
                archive.display()
            )))
        }
    }

    fn extract_tar_gz(&self, archive: &Path, dest: &Path) -> Result<()> {
        let f = fs::File::open(archive).map_err(GraphError::Io)?;
        let gz = flate2::read::GzDecoder::new(f);
        let mut tar = tar::Archive::new(gz);
        tar.set_overwrite(true);
        // `unpack` writes with the umask applied; entries are prefixed by `dest`.
        // We avoid absolute paths / `..` traversal by unpacking into `dest` and
        // relying on tar's own prefix checks.
        tar.unpack(dest)
            .map_err(|e| GraphError::Plugin(format!("tar.gz extraction failed: {}", e)))?;
        Ok(())
    }

    fn extract_zip(&self, archive: &Path, dest: &Path) -> Result<()> {
        let f = fs::File::open(archive).map_err(GraphError::Io)?;
        let mut zip = zip::ZipArchive::new(f)
            .map_err(|e| GraphError::Plugin(format!("zip open failed: {}", e)))?;
        for i in 0..zip.len() {
            let mut entry = zip
                .by_index(i)
                .map_err(|e| GraphError::Plugin(format!("zip read entry {}: {}", i, e)))?;
            let entry_name = entry.name().to_string();
            // Reject absolute paths and parent traversal to defang Zip Slip.
            if entry_name.contains("..") || Path::new(&entry_name).is_absolute() {
                return Err(GraphError::Plugin(format!(
                    "unsafe zip entry name: {}",
                    entry_name
                )));
            }
            let out_path = dest.join(&entry_name);
            if entry.is_dir() {
                fs::create_dir_all(&out_path).map_err(GraphError::Io)?;
            } else {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent).map_err(GraphError::Io)?;
                }
                let mut out = fs::File::create(&out_path).map_err(GraphError::Io)?;
                io::copy(&mut entry, &mut out).map_err(GraphError::Io)?;
            }
            // Best-effort permission restore (unix only).
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = entry.unix_mode() {
                    let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(mode));
                }
            }
        }
        Ok(())
    }

    /// Locate the executable inside an extracted version dir.
    ///
    /// Convention: the binary is named after the plugin (sans `parser-` prefix
    /// is *not* applied — we look for the plugin name verbatim, plus `.exe`
    /// on Windows). If not at the root, we search one level deep.
    pub fn binary_path(&self, name: &str, version: &str) -> Result<PathBuf> {
        let dir = self.version_dir(name, version);
        let candidates: Vec<PathBuf> = if cfg!(windows) {
            vec![
                dir.join(format!("{}.exe", name)),
                dir.join(name).join(format!("{}.exe", name)),
            ]
        } else {
            vec![dir.join(name), dir.join(name).join(name)]
        };

        for c in &candidates {
            if c.is_file() {
                return Ok(c.clone());
            }
        }
        // Fall back to any executable file directly under the version dir.
        if let Ok(entries) = fs::read_dir(&dir) {
            for ent in entries.flatten() {
                let p = ent.path();
                if p.is_file() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(meta) = fs::metadata(&p) {
                            if meta.permissions().mode() & 0o111 != 0 {
                                return Ok(p);
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        if p.extension().and_then(|e| e.to_str()) == Some("exe") {
                            return Ok(p);
                        }
                    }
                }
            }
        }
        Err(GraphError::Plugin(format!(
            "no executable found for {} {} in {}",
            name,
            version,
            dir.display()
        )))
    }

    /// All cached versions of `name`, sorted descending (newest first).
    pub fn list_versions(&self, name: &str) -> Result<Vec<Version>> {
        let dir = self.plugin_dir(name);
        let mut versions = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for ent in entries.flatten() {
                if let Some(file) = ent.file_name().to_str() {
                    if let Ok(v) = Version::parse(file) {
                        versions.push(v);
                    }
                }
            }
        }
        versions.sort_by(|a, b| b.cmp(a));
        Ok(versions)
    }

    /// Remove old versions, keeping the `keep` newest plus `protect` (the
    /// currently-active version, if any). Never removes a version whose path
    /// equals `protect_path`.
    pub fn prune(&self, name: &str, keep: usize) -> Result<()> {
        self.prune_protect(name, keep, None)
    }

    /// Like [`prune`] but additionally protects `protect_version` from removal
    /// even if it falls outside the top-`keep` window.
    pub fn prune_protect(
        &self,
        name: &str,
        keep: usize,
        protect_version: Option<&str>,
    ) -> Result<()> {
        let versions = self.list_versions(name)?;
        let keep_set: std::collections::HashSet<String> =
            versions.iter().take(keep).map(|v| v.to_string()).collect();

        let dir = self.plugin_dir(name);
        if let Ok(entries) = fs::read_dir(&dir) {
            for ent in entries.flatten() {
                let path = ent.path();
                let Some(file) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                // Skip non-version dirs silently.
                if Version::parse(file).is_err() {
                    continue;
                }
                if keep_set.contains(file) {
                    continue;
                }
                if let Some(pv) = protect_version {
                    if file == pv {
                        continue;
                    }
                }
                debug!(?path, "pruning old plugin version");
                if let Err(e) = fs::remove_dir_all(&path) {
                    warn!(?path, error = %e, "failed to prune version");
                }
            }
        }
        Ok(())
    }

    /// Delete every cached version of `name`.
    pub fn purge(&self, name: &str) -> Result<()> {
        let dir = self.plugin_dir(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(GraphError::Io)?;
        }
        Ok(())
    }
}

impl Default for PluginCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write; // for `zip.start_file(...).write_all(...)`

    /// Build a tar.gz containing `files` (name -> bytes), return its path.
    fn make_tar_gz(dir: &Path, files: &[(&str, &[u8])]) -> PathBuf {
        let archive_path = dir.join("plugin.tar.gz");
        let tar_file = fs::File::create(&archive_path).unwrap();
        let enc = flate2::write::GzEncoder::new(tar_file, flate2::Compression::default());
        let mut builder = tar::Builder::new(enc);
        for (name, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, name, io::Cursor::new(*content))
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
        archive_path
    }

    /// Build a zip containing `files`, return its path.
    fn make_zip(dir: &Path, files: &[(&str, &[u8])]) -> PathBuf {
        let archive_path = dir.join("plugin.zip");
        let f = fs::File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions =
            zip::write::SimpleFileOptions::default().unix_permissions(0o755);
        for (name, content) in files {
            zip.start_file(name, opts).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
        archive_path
    }

    #[test]
    fn store_extracts_tar_gz_and_locates_binary() {
        let root = tempfile::tempdir().unwrap();
        let cache = PluginCache::with_root(root.path().to_path_buf());

        // On Windows the binary lookup prefers `<name>.exe`; build the archive
        // so it contains the right entry name for the host platform.
        let binary_name = if cfg!(windows) {
            "parser-python.exe"
        } else {
            "parser-python"
        };
        let bin_bytes = if cfg!(windows) {
            b"fake exe".as_slice()
        } else {
            b"#!/bin/sh\necho mock"
        };
        let archive = make_tar_gz(
            root.path(),
            &[(binary_name, bin_bytes), ("README.md", b"hi")],
        );

        let dest = cache
            .store("parser-python", "1.0.0", &archive)
            .expect("store");
        assert!(dest.is_dir());
        assert!(dest.join("README.md").exists());

        let bin = cache
            .binary_path("parser-python", "1.0.0")
            .expect("binary located");
        assert!(bin.is_file());
        assert_eq!(bin.file_name().unwrap().to_str().unwrap(), binary_name);
    }

    #[test]
    fn store_extracts_zip() {
        let root = tempfile::tempdir().unwrap();
        let cache = PluginCache::with_root(root.path().to_path_buf());
        let archive = make_zip(
            root.path(),
            &[("parser-ruby", b"#!/bin/sh\n"), ("a.txt", b"x")],
        );
        let dest = cache
            .store("parser-ruby", "0.1.0", &archive)
            .expect("store");
        assert!(dest.join("a.txt").exists());
    }

    #[test]
    fn unsupported_format_errors() {
        let root = tempfile::tempdir().unwrap();
        let cache = PluginCache::with_root(root.path().to_path_buf());
        let bad = root.path().join("plugin.wasm");
        fs::write(&bad, b"x").unwrap();
        let err = cache.store("p", "1.0.0", &bad).unwrap_err();
        assert!(err.to_string().contains("unsupported archive format"));
    }

    #[test]
    fn list_versions_sorted_desc() {
        let root = tempfile::tempdir().unwrap();
        let cache = PluginCache::with_root(root.path().to_path_buf());
        let archive = make_tar_gz(root.path(), &[("parser-ruby", b"x")]);
        for v in ["1.0.0", "0.9.0", "1.2.0", "0.1.0"] {
            cache.store("parser-ruby", v, &archive).unwrap();
        }
        let versions = cache.list_versions("parser-ruby").unwrap();
        assert_eq!(
            versions.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
            vec!["1.2.0", "1.0.0", "0.9.0", "0.1.0"]
        );
    }

    #[test]
    fn prune_keeps_n_plus_protected() {
        let root = tempfile::tempdir().unwrap();
        let cache = PluginCache::with_root(root.path().to_path_buf());
        let archive = make_tar_gz(root.path(), &[("parser-ruby", b"x")]);
        for v in ["1.0.0", "0.9.0", "1.2.0", "0.1.0", "0.5.0"] {
            cache.store("parser-ruby", v, &archive).unwrap();
        }
        // Keep newest 3 (1.2.0, 1.0.0, 0.9.0); protect 0.1.0 too.
        cache
            .prune_protect("parser-ruby", 3, Some("0.1.0"))
            .unwrap();
        let remaining = cache.list_versions("parser-ruby").unwrap();
        let remaining_str: Vec<String> = remaining.iter().map(|v| v.to_string()).collect();
        assert!(remaining_str.contains(&"1.2.0".to_string()));
        assert!(remaining_str.contains(&"1.0.0".to_string()));
        assert!(remaining_str.contains(&"0.9.0".to_string()));
        assert!(remaining_str.contains(&"0.1.0".to_string()), "protected");
        assert!(
            !remaining_str.contains(&"0.5.0".to_string()),
            "old unprotected pruned"
        );
    }

    #[test]
    fn purge_removes_everything() {
        let root = tempfile::tempdir().unwrap();
        let cache = PluginCache::with_root(root.path().to_path_buf());
        let archive = make_tar_gz(root.path(), &[("parser-ruby", b"x")]);
        cache.store("parser-ruby", "1.0.0", &archive).unwrap();
        assert!(cache.plugin_dir("parser-ruby").exists());
        cache.purge("parser-ruby").unwrap();
        assert!(!cache.plugin_dir("parser-ruby").exists());
    }
}
