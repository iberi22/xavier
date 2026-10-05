//! Per-repository Xavier configuration: `<repo>/.xavier/config.toml`.
//!
//! # Why this exists
//!
//! [`crate::codebase::repo_identity::derive_project_id`] derives a
//! `project_id` from the **repository directory name**. That is stable for a
//! single-product repo, but it collapses every product inside a monorepo onto
//! one identity: `apps/duque-mvp` and `apps/tripro-web` inside the same
//! checkout both resolve to `<monorepo-dir>` as `project_id`, so they cannot
//! have separate memory instances. This module lets the repo *declare* its
//! identity (and its privacy / retention parameters) instead of having it
//! guessed.
//!
//! # Safety contract
//!
//! - **A missing file is never an error.** It means "behave exactly as before",
//!   i.e. the directory-derived `project_id` and the global defaults.
//! - **A malformed file is a clear error, never a panic.** Every failure path
//!   returns [`RepoConfigError`] with the offending path, field and reason.
//! - **Nothing is hardcoded per product.** Every value below either comes from
//!   the repo's own `config.toml` or from the existing global settings/env
//!   (`security::clearance::default_clearance`, which honours
//!   `XAVIER_DEFAULT_CLEARANCE`).
//!
//! # File format
//!
//! ```toml
//! # <repo>/.xavier/config.toml — every key is optional.
//! project_id = "duque-mvp"        # overrides the directory-derived id (monorepo)
//! name       = "Duque MVP"       # human-readable label
//!
//! [privacy]
//! default_clearance = "confidential"   # unclassified|internal|restricted|confidential|secret|top_secret
//!
//! [codegraph]
//! sync_to_repo = false            # opt-in: may `code sync` write state into git?
//!
//! [memory]
//! embedding_dimensions = 768      # inherit global when absent
//! retention_days = 90             # 0/absent = keep forever
//! ```
//!
//! Declaring `project_id` in a sub-product is what makes two products in one
//! repo distinct: each product ships its own `config.toml`, so `duque-mvp` and
//! `tripro-web` get separate memory instances while sharing one checkout.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use xavier_core_logic::ClearanceLevel;

use super::repo_identity::sanitize_project_id;

/// File name inside `<repo>/.xavier/`.
pub const REPO_CONFIG_FILE: &str = "config.toml";

/// Directory (inside the repo) that holds the whole per-repo Xavier package.
pub const REPO_PACKAGE_DIR: &str = ".xavier";

/// Largest `embedding_dimensions` accepted from a repo config.
///
/// A repo may not silently re-declare a vector store the global install cannot
/// serve; the cap catches typos (`7680`) and absurd values.
pub const MAX_EMBEDDING_DIMENSIONS: usize = 8192;

/// Every way reading a per-repo config can fail. Never a panic.
#[derive(Debug, thiserror::Error)]
pub enum RepoConfigError {
    /// The file exists but could not be read (permissions, a directory, …).
    #[error("no se pudo leer el config por repo `{path}`: {source}")]
    Read {
        /// Offending path.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid TOML, or has an unknown/mistyped key.
    #[error("TOML invalido en el config por repo `{path}`: {source}")]
    Parse {
        /// Offending path.
        path: PathBuf,
        /// Underlying TOML error, including the line/column.
        #[source]
        source: toml::de::Error,
    },
    /// The file is valid TOML but a value is out of contract.
    #[error("valor invalido para `{field}` en el config por repo `{path}`: {reason}")]
    Invalid {
        /// Offending path.
        path: PathBuf,
        /// Offending key, dotted for nested keys (`memory.retention_days`).
        field: String,
        /// Why it was rejected.
        reason: String,
    },
    /// Writing the config back failed.
    #[error("no se pudo escribir el config por repo `{path}`: {source}")]
    Write {
        /// Offending path.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
}

/// Where the effective `project_id` came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectIdOrigin {
    /// Derived from the repository directory name (current behaviour).
    Directory,
    /// Declared in `<repo>/.xavier/config.toml`.
    ConfigFile,
}

impl ProjectIdOrigin {
    /// `true` when the repo overrode the derived id.
    pub fn is_override(&self) -> bool {
        matches!(self, ProjectIdOrigin::ConfigFile)
    }
}

/// Effective per-repo configuration: the file's values on top of safe defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoConfig {
    /// Effective identity. Declared value (sanitized) when present, else the
    /// directory-derived id — this is the field that separates two products
    /// living in the same monorepo.
    pub project_id: String,
    /// Whether `project_id` came from the config file or from the directory.
    pub project_id_origin: ProjectIdOrigin,
    /// Human-readable label for the memory instance.
    pub name: String,
    /// Default classification for memories created in this repo. Absent in the
    /// file ⇒ the global default (`XAVIER_DEFAULT_CLEARANCE`, else Internal).
    pub default_clearance: ClearanceLevel,
    /// Whether `code sync` may persist graph state into this repo (git).
    pub sync_codegraph: bool,
    /// Embedding width for this repo's index, or `None` = inherit global.
    pub embedding_dimensions: Option<usize>,
    /// Retention in days for this repo's memories; `None`/`0` = keep forever.
    pub retention_days: Option<u64>,
    /// Path the config was loaded from, or `None` when no file existed.
    pub source_path: Option<PathBuf>,
}

impl RepoConfig {
    /// Defaults for a repo with **no** `config.toml`: identical to the
    /// pre-existing behaviour.
    pub fn defaults_for(root: &Path) -> Self {
        let project_id = super::repo_identity::derive_project_id(root);
        Self {
            name: project_id.clone(),
            project_id,
            project_id_origin: ProjectIdOrigin::Directory,
            // Not hardcoded: honours XAVIER_DEFAULT_CLEARANCE like every other
            // memory write in Xavier.
            default_clearance: crate::security::clearance::default_clearance(),
            sync_codegraph: false,
            embedding_dimensions: None,
            retention_days: None,
            source_path: None,
        }
    }

    /// Read `<root>/.xavier/config.toml`, validate it, apply defaults.
    ///
    /// Missing file ⇒ `Ok(defaults_for(root))`. Malformed file ⇒
    /// `Err(RepoConfigError)` naming path, field and reason.
    pub fn load(root: &Path) -> Result<Self, RepoConfigError> {
        Self::load_from(&repo_config_path(root))
    }

    /// Same as [`RepoConfig::load`] but against an explicit file path. Used by
    /// the CLI against `--root`.
    pub fn load_from(path: &Path) -> Result<Self, RepoConfigError> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let root = root_of(path);
                let mut cfg = Self::defaults_for(&root);
                cfg.source_path = None;
                return Ok(cfg);
            }
            Err(err) => {
                return Err(RepoConfigError::Read {
                    path: path.to_path_buf(),
                    source: err,
                })
            }
        };

        let file: RepoConfigFile =
            toml::from_str(&raw).map_err(|source| RepoConfigError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        file.into_config(path)
    }

    /// Resolve only the effective `project_id` for `root`.
    ///
    /// This is the integration point with
    /// [`crate::codebase::repo_identity::derive_project_id`]: the config can
    /// override it, and when the config is absent or malformed the derived id
    /// is still returned (malformed ⇒ derived, plus the error for the caller to
    /// report) so a broken file never changes identity silently.
    pub fn resolve_project_id(root: &Path) -> (String, ProjectIdOrigin, Option<RepoConfigError>) {
        match Self::load(root) {
            Ok(cfg) => (cfg.project_id, cfg.project_id_origin, None),
            Err(err) => (
                super::repo_identity::derive_project_id(root),
                ProjectIdOrigin::Directory,
                Some(err),
            ),
        }
    }
}

/// On-disk shape of `config.toml`. Everything is optional and unknown keys are
/// rejected: a silently ignored `clearance = "secret"` typo would widen the
/// privacy scope without anybody noticing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    privacy: Option<PrivacySection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    codegraph: Option<CodegraphSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory: Option<MemorySection>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivacySection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_clearance: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CodegraphSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sync_to_repo: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemorySection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    embedding_dimensions: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retention_days: Option<u64>,
}

/// Levels a repo may declare. Parsed explicitly (not via `ClearanceLevel::parse`,
/// which silently promotes an unknown value to `top_secret`) so a typo is
/// reported instead of quietly escalating the declared scope.
fn parse_declared_clearance(raw: &str) -> Option<ClearanceLevel> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "unclassified" => Some(ClearanceLevel::Unclassified),
        "internal" => Some(ClearanceLevel::Internal),
        "restricted" => Some(ClearanceLevel::Restricted),
        "confidential" => Some(ClearanceLevel::Confidential),
        "secret" => Some(ClearanceLevel::Secret),
        "top_secret" | "topsecret" => Some(ClearanceLevel::TopSecret),
        _ => None,
    }
}

impl RepoConfigFile {
    /// Validate and fold onto the safe defaults for `root`.
    fn into_config(self, path: &Path) -> Result<RepoConfig, RepoConfigError> {
        let root = root_of(path);
        let mut cfg = RepoConfig::defaults_for(&root);

        if let Some(raw) = self.project_id {
            let declared = raw.trim();
            if declared.is_empty() {
                return Err(RepoConfigError::Invalid {
                    path: path.to_path_buf(),
                    field: "project_id".to_string(),
                    reason: "no puede estar vacio".to_string(),
                });
            }
            let sanitized = sanitize_project_id(declared);
            if sanitized.is_empty() || super::validate_project_id(&sanitized).is_err() {
                return Err(RepoConfigError::Invalid {
                    path: path.to_path_buf(),
                    field: "project_id".to_string(),
                    reason: format!(
                        "'{declared}' no se puede sanear a un id utilizable en rutas; \
                         usa solo [A-Za-z0-9_-]"
                    ),
                });
            }
            cfg.project_id = sanitized;
            cfg.project_id_origin = ProjectIdOrigin::ConfigFile;
        }

        if let Some(name) = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            cfg.name = name.to_string();
        }

        if let Some(privacy) = self.privacy {
            if let Some(raw) = privacy.default_clearance {
                match parse_declared_clearance(&raw) {
                    Some(level) => cfg.default_clearance = level,
                    None => {
                        return Err(RepoConfigError::Invalid {
                            path: path.to_path_buf(),
                            field: "privacy.default_clearance".to_string(),
                            reason: format!(
                                "'{raw}' no es un nivel conocido \
                                 (unclassified|internal|restricted|confidential|secret|top_secret)"
                            ),
                        })
                    }
                }
            }
        }

        if let Some(codegraph) = self.codegraph {
            if let Some(sync) = codegraph.sync_to_repo {
                cfg.sync_codegraph = sync;
            }
        }

        if let Some(memory) = self.memory {
            if let Some(dims) = memory.embedding_dimensions {
                if dims == 0 || dims > MAX_EMBEDDING_DIMENSIONS {
                    return Err(RepoConfigError::Invalid {
                        path: path.to_path_buf(),
                        field: "memory.embedding_dimensions".to_string(),
                        reason: format!(
                            "{dims} esta fuera de rango (1..={MAX_EMBEDDING_DIMENSIONS})"
                        ),
                    });
                }
                cfg.embedding_dimensions = Some(dims);
            }
            if let Some(days) = memory.retention_days {
                cfg.retention_days = (days > 0).then_some(days);
            }
        }

        cfg.source_path = Some(path.to_path_buf());
        Ok(cfg)
    }
}

/// `<root>/.xavier/config.toml`.
pub fn repo_config_path(root: &Path) -> PathBuf {
    root.join(REPO_PACKAGE_DIR).join(REPO_CONFIG_FILE)
}

/// Recover the repository root from a config path, tolerating a missing file
/// path (the CLI may point at `<root>/.xavier/config.toml` before it exists).
fn root_of(path: &Path) -> PathBuf {
    match path.parent() {
        Some(dir) if dir.file_name().is_some_and(|n| n == REPO_PACKAGE_DIR) => dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| dir.to_path_buf()),
        _ => path.parent().map(Path::to_path_buf).unwrap_or_default(),
    }
}

/// Keys accepted by `xavier repo config set <clave> <valor>`.
pub const SETTABLE_KEYS: &[&str] = &[
    "project_id",
    "name",
    "clearance",
    "sync_codegraph",
    "embedding_dimensions",
    "retention_days",
];

/// Normalise a user-typed key (`privacy.default_clearance` ⇒ `clearance`).
fn normalize_key(key: &str) -> String {
    let tail = key
        .rsplit('.')
        .next()
        .unwrap_or(key)
        .trim()
        .to_ascii_lowercase();
    match tail.as_str() {
        "default_clearance" => "clearance".to_string(),
        "sync_to_repo" => "sync_codegraph".to_string(),
        other => other.to_string(),
    }
}

/// Comment header written on every generated config so the file explains itself
/// when it is committed and read months later.
const HEADER: &str = "\
# Xavier per-repo memory instance (XAV-REPO).
# Every key is optional: an absent file means \"current behaviour\" and is never
# an error. Declaring `project_id` is what lets two products inside ONE monorepo
# have separate memory instances, because each product ships its own config.
# Commit this file to share the instance across the team.
";

/// Write a fresh, fully commented `config.toml` for `root`.
///
/// Fails with [`RepoConfigError::Invalid`] if a file already exists, so `init`
/// can never clobber a repo's declared identity.
pub fn init_config(root: &Path, force: bool) -> Result<PathBuf, RepoConfigError> {
    let path = repo_config_path(root);
    if path.exists() && !force {
        return Err(RepoConfigError::Invalid {
            path,
            field: "config".to_string(),
            reason: "ya existe; usa --force para sobrescribirlo".to_string(),
        });
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| RepoConfigError::Write {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    let cfg = RepoConfig::defaults_for(root);
    let file = RepoConfigFile {
        project_id: Some(cfg.project_id.clone()),
        name: Some(cfg.project_id.clone()),
        privacy: Some(PrivacySection {
            default_clearance: Some(cfg.default_clearance.as_str().to_string()),
        }),
        codegraph: Some(CodegraphSection {
            sync_to_repo: Some(cfg.sync_codegraph),
        }),
        memory: None,
    };
    let body = render(&file);
    std::fs::write(&path, body).map_err(|source| RepoConfigError::Write {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

fn render(file: &RepoConfigFile) -> String {
    let body = toml::to_string_pretty(file).unwrap_or_default();
    format!("{HEADER}\n{body}")
}

/// Read the file shape for `path`, treating absence as an empty config.
fn read_file_shape(path: &Path) -> Result<RepoConfigFile, RepoConfigError> {
    match std::fs::read_to_string(path) {
        Ok(raw) => toml::from_str(&raw).map_err(|source| RepoConfigError::Parse {
            path: path.to_path_buf(),
            source,
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(RepoConfigFile::default()),
        Err(err) => Err(RepoConfigError::Read {
            path: path.to_path_buf(),
            source: err,
        }),
    }
}

/// Set one key in `<root>/.xavier/config.toml`, validating before writing.
///
/// The new value is validated through the same code path as `load`, so `set`
/// can never persist a config that `load` would later reject.
pub fn set_key(root: &Path, key: &str, value: &str) -> Result<RepoConfig, RepoConfigError> {
    let path = repo_config_path(root);
    let mut file = read_file_shape(&path)?;

    let key_norm = normalize_key(key);
    match key_norm.as_str() {
        "project_id" => file.project_id = Some(value.to_string()),
        "name" => file.name = Some(value.to_string()),
        "clearance" => {
            if parse_declared_clearance(value).is_none() {
                return Err(RepoConfigError::Invalid {
                    path,
                    field: "privacy.default_clearance".to_string(),
                    reason: format!(
                        "'{value}' no es un nivel conocido \
                         (unclassified|internal|restricted|confidential|secret|top_secret)"
                    ),
                });
            }
            let privacy = file.privacy.get_or_insert_with(PrivacySection::default);
            privacy.default_clearance = Some(value.to_ascii_lowercase());
        }
        "sync_codegraph" => {
            let parsed = match value.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "1" | "on" => true,
                "false" | "no" | "0" | "off" => false,
                other => {
                    return Err(RepoConfigError::Invalid {
                        path,
                        field: "codegraph.sync_to_repo".to_string(),
                        reason: format!("'{other}' no es un booleano (true|false)"),
                    })
                }
            };
            let codegraph = file.codegraph.get_or_insert_with(CodegraphSection::default);
            codegraph.sync_to_repo = Some(parsed);
        }
        "embedding_dimensions" => {
            let parsed: usize = value.trim().parse().map_err(|_| RepoConfigError::Invalid {
                path: path.clone(),
                field: "memory.embedding_dimensions".to_string(),
                reason: format!("'{value}' no es un entero"),
            })?;
            let memory = file.memory.get_or_insert_with(MemorySection::default);
            memory.embedding_dimensions = Some(parsed);
        }
        "retention_days" => {
            let parsed: u64 = value.trim().parse().map_err(|_| RepoConfigError::Invalid {
                path: path.clone(),
                field: "memory.retention_days".to_string(),
                reason: format!("'{value}' no es un entero de dias"),
            })?;
            let memory = file.memory.get_or_insert_with(MemorySection::default);
            memory.retention_days = Some(parsed);
        }
        other => {
            return Err(RepoConfigError::Invalid {
                path,
                field: other.to_string(),
                reason: format!(
                    "clave desconocida; usa una de: {}",
                    SETTABLE_KEYS.join(", ")
                ),
            })
        }
    }

    // Validate exactly as `load` would, before it reaches disk.
    let cfg = file.clone().into_config(&path)?;

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| RepoConfigError::Write {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(&path, render(&file)).map_err(|source| RepoConfigError::Write {
        path: path.clone(),
        source,
    })?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codebase::{repo_identity, validate_project_id};

    fn repo_with_config(toml_body: &str, name: &str) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join(name);
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");
        std::fs::create_dir_all(repo.join(REPO_PACKAGE_DIR)).expect("mkdir .xavier");
        std::fs::write(repo_config_path(&repo), toml_body).expect("write config");
        (tmp, repo)
    }

    // ── default: absent file == current behaviour, never an error ──────────

    #[test]
    fn repo_config_absent_file_uses_defaults_and_never_errors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("xavier-plain-repo");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");

        let cfg = RepoConfig::load(&repo).expect("absent config must not error");
        assert_eq!(cfg.project_id, "xavier-plain-repo");
        assert_eq!(cfg.project_id_origin, ProjectIdOrigin::Directory);
        assert!(!cfg.project_id_origin.is_override());
        assert_eq!(cfg.name, "xavier-plain-repo");
        assert!(
            !cfg.sync_codegraph,
            "codegraph sync is opt-in, never implicit"
        );
        assert_eq!(cfg.embedding_dimensions, None);
        assert_eq!(cfg.retention_days, None);
        assert_eq!(cfg.source_path, None, "no file was read");
        assert_eq!(
            cfg.default_clearance,
            crate::security::clearance::default_clearance(),
            "the privacy default is the global one, never hardcoded here"
        );
    }

    // ── a valid config overrides the derived project_id ─────────────────────

    #[test]
    fn repo_config_valid_file_overrides_derived_project_id() {
        let (_tmp, repo) = repo_with_config(
            r#"
project_id = "duque-mvp"
name = "Duque MVP"

[privacy]
default_clearance = "confidential"

[codegraph]
sync_to_repo = true

[memory]
embedding_dimensions = 1024
retention_days = 90
"#,
            "swal-monorepo-xavrepo",
        );

        let cfg = RepoConfig::load(&repo).expect("valid config loads");
        assert_eq!(cfg.project_id, "duque-mvp");
        assert_eq!(cfg.project_id_origin, ProjectIdOrigin::ConfigFile);
        assert!(cfg.project_id_origin.is_override());
        assert_eq!(cfg.name, "Duque MVP");
        assert_eq!(cfg.default_clearance, ClearanceLevel::Confidential);
        assert!(cfg.sync_codegraph);
        assert_eq!(cfg.embedding_dimensions, Some(1024));
        assert_eq!(cfg.retention_days, Some(90));
        assert_eq!(cfg.source_path, Some(repo_config_path(&repo)));

        // The integration point: derive_project_id still says the directory
        // name, the config says "duque-mvp", and the config wins.
        assert_eq!(
            repo_identity::derive_project_id(&repo),
            "swal-monorepo-xavrepo"
        );
        let (pid, origin, err) = RepoConfig::resolve_project_id(&repo);
        assert_eq!(pid, "duque-mvp");
        assert_eq!(origin, ProjectIdOrigin::ConfigFile);
        assert!(err.is_none());
    }

    // ── THE MONOREPO GAP: one repo, two products, two distinct identities ────

    #[test]
    fn repo_config_two_products_in_one_repo_get_distinct_project_ids() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("swal");
        let duque = repo.join("apps").join("duque-mvp");
        let tripro = repo.join("apps").join("tripro-web");
        for d in [&duque, &tripro] {
            std::fs::create_dir_all(d).expect("mkdir product");
        }
        // ONE checkout, ONE git root.
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");

        // Precondition: without a config, both products collapse to one id.
        assert_eq!(
            repo_identity::derive_repo_identity(&duque).project_id,
            repo_identity::derive_repo_identity(&tripro).project_id,
            "precondition: the directory-derived id cannot separate them"
        );

        // Each product ships its own config declaring its own identity. A repo
        // with several products uses one config per product subtree.
        for (dir, pid) in [(&duque, "duque-mvp"), (&tripro, "tripro-web")] {
            std::fs::create_dir_all(dir.join(REPO_PACKAGE_DIR)).expect("mkdir .xavier");
            std::fs::write(repo_config_path(dir), format!("project_id = \"{pid}\"\n"))
                .expect("write product config");
        }

        let duque_root = repo_identity::find_repo_root(&duque).expect("root duque");
        let tripro_root = repo_identity::find_repo_root(&tripro).expect("root tripro");
        assert_eq!(
            duque_root, tripro_root,
            "both products are inside the same checkout"
        );

        let (duque_pid, duque_origin, duque_err) = RepoConfig::resolve_project_id(&duque);
        let (tripro_pid, tripro_origin, tripro_err) = RepoConfig::resolve_project_id(&tripro);

        assert!(duque_err.is_none() && tripro_err.is_none());
        assert_eq!(duque_origin, ProjectIdOrigin::ConfigFile);
        assert_eq!(tripro_origin, ProjectIdOrigin::ConfigFile);
        assert_eq!(duque_pid, "duque-mvp");
        assert_eq!(tripro_pid, "tripro-web");
        assert_ne!(
            duque_pid, tripro_pid,
            "GAP CLOSED: two products in ONE repo must have distinct project_ids"
        );
    }

    // ── malformed TOML: clear error, no panic ───────────────────────────────

    #[test]
    fn repo_config_invalid_toml_reports_a_clear_error() {
        let (_tmp, repo) = repo_with_config("project_id = \"unterminated\n", "bad-toml-repo");
        let err = RepoConfig::load(&repo).expect_err("invalid TOML must error");
        let text = err.to_string();
        assert!(
            text.contains("TOML invalido"),
            "error must name the failure class, got: {text}"
        );
        assert!(
            text.contains("config.toml"),
            "error must name the offending file, got: {text}"
        );
    }

    #[test]
    fn repo_config_unknown_key_reports_the_typo_instead_of_ignoring_it() {
        // `clearance` at top level (instead of under [privacy]) would silently
        // be ignored; it must be rejected loudly rather than widen the scope.
        let (_tmp, repo) = repo_with_config("clearance = \"public\"\n", "typo-repo");
        let err = RepoConfig::load(&repo).expect_err("unknown key must error");
        let text = err.to_string();
        assert!(
            text.contains("TOML invalido") && text.contains("clearance"),
            "error must name the offending key, got: {text}"
        );
    }

    #[test]
    fn repo_config_rejects_out_of_contract_values() {
        let (_tmp, clearance) = repo_with_config(
            "[privacy]\ndefault_clearance = \"abierto-todo\"\n",
            "bad-clearance-repo",
        );
        let err = RepoConfig::load(&clearance).expect_err("unknown level must error");
        assert!(err.to_string().contains("privacy.default_clearance"));

        let (_tmp, dims) =
            repo_with_config("[memory]\nembedding_dimensions = 0\n", "zero-dims-repo");
        let err = RepoConfig::load(&dims).expect_err("0 dims must error");
        assert!(err.to_string().contains("memory.embedding_dimensions"));

        let (_tmp, huge) =
            repo_with_config("[memory]\nembedding_dimensions = 99999\n", "huge-dims-repo");
        let err = RepoConfig::load(&huge).expect_err("absurd dims must error");
        assert!(err.to_string().contains("memory.embedding_dimensions"));

        let (_tmp, empty) = repo_with_config("project_id = \"   \"\n", "empty-id-repo");
        let err = RepoConfig::load(&empty).expect_err("empty project_id must error");
        assert!(err.to_string().contains("project_id"));
    }

    // ── sanitization parity with derive_project_id ──────────────────────────

    #[test]
    fn repo_config_project_id_is_sanitized_exactly_like_derive_project_id() {
        let (_tmp, repo) =
            repo_with_config("project_id = \"duque mvp.prod/2026\"\n", "weird-chars-repo");
        let cfg = RepoConfig::load(&repo).expect("odd chars must sanitize, not fail");
        assert_eq!(cfg.project_id, "duque_mvp_prod_2026");
        assert_eq!(
            cfg.project_id,
            repo_identity::sanitize_project_id("duque mvp.prod/2026"),
            "the same sanitizer as derive_project_id must be used"
        );
        // And the same result as the directory route for the same raw input.
        let dir = PathBuf::from("/tmp/duque mvp.prod-2026");
        assert_eq!(
            repo_identity::derive_project_id(&dir),
            repo_identity::sanitize_project_id("duque mvp.prod-2026")
        );
        assert!(
            validate_project_id(&cfg.project_id).is_ok(),
            "a declared id must always be safe for file paths"
        );

        // Path traversal can never survive sanitization.
        let (_tmp2, trav) = repo_with_config("project_id = \"../etc/passwd\"\n", "trav-repo");
        let cfg2 = RepoConfig::load(&trav).expect("traversal sanitizes");
        assert_eq!(cfg2.project_id, "___etc_passwd");
        assert!(validate_project_id(&cfg2.project_id).is_ok());
    }

    // ── a malformed file never changes identity silently ────────────────────

    #[test]
    fn repo_config_malformed_file_falls_back_to_derived_id_but_reports_it() {
        let (_tmp, repo) = repo_with_config("project_id = = 3\n", "broken-repo");
        let (pid, origin, err) = RepoConfig::resolve_project_id(&repo);
        assert_eq!(pid, "broken-repo", "identity must stay the derived one");
        assert_eq!(origin, ProjectIdOrigin::Directory);
        assert!(
            err.is_some(),
            "the caller must be able to report the broken file"
        );
    }

    // ── CLI-side mutations round-trip through the same validation ───────────

    #[test]
    fn repo_config_set_writes_a_config_load_accepts() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("set-repo");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");

        let cfg = set_key(&repo, "project_id", "tripro.web").expect("set project_id");
        assert_eq!(cfg.project_id, "tripro_web", "sanitized on write");
        let cfg = set_key(&repo, "privacy.default_clearance", "SECRET").expect("set clearance");
        assert_eq!(cfg.default_clearance, ClearanceLevel::Secret);
        let cfg = set_key(&repo, "sync_codegraph", "yes").expect("set sync");
        assert!(cfg.sync_codegraph);
        let cfg = set_key(&repo, "retention_days", "0").expect("set retention 0");
        assert_eq!(cfg.retention_days, None, "0 means keep forever");

        let reloaded = RepoConfig::load(&repo).expect("what set wrote, load must accept");
        assert_eq!(reloaded.project_id, "tripro_web");
        assert_eq!(reloaded.default_clearance, ClearanceLevel::Secret);
        assert!(reloaded.sync_codegraph);
        assert_eq!(reloaded.retention_days, None);
    }

    #[test]
    fn repo_config_set_rejects_unknown_keys_and_values() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("set-bad-repo");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");

        let err = set_key(&repo, "nope", "1").expect_err("unknown key must error");
        assert!(err.to_string().contains("clave desconocida"));

        let err = set_key(&repo, "clearance", "abierto").expect_err("bad level must error");
        assert!(err.to_string().contains("privacy.default_clearance"));

        let err = set_key(&repo, "sync_codegraph", "quiza").expect_err("bad bool must error");
        assert!(err.to_string().contains("booleano"));

        let err = set_key(&repo, "retention_days", "mil").expect_err("bad int must error");
        assert!(err.to_string().contains("entero"));
    }

    #[test]
    fn repo_config_init_creates_a_loadable_commented_file_and_refuses_clobber() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("init-repo");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");

        let path = init_config(&repo, false).expect("init creates the file");
        let body = std::fs::read_to_string(&path).expect("read generated config");
        assert!(body.contains("# Xavier per-repo memory instance"));
        assert!(body.contains("project_id"));

        let cfg = RepoConfig::load(&repo).expect("generated config must load");
        assert_eq!(cfg.project_id, "init-repo");
        assert_eq!(cfg.project_id_origin, ProjectIdOrigin::ConfigFile);
        assert!(
            !cfg.sync_codegraph,
            "generated config keeps the safe default"
        );

        let err = init_config(&repo, false).expect_err("init must not clobber silently");
        assert!(err.to_string().contains("--force"));
        init_config(&repo, true).expect("--force overwrites");
    }

    #[test]
    fn repo_config_path_lives_in_the_repo_package_dir() {
        assert_eq!(
            repo_config_path(Path::new("/tmp/some-repo")),
            PathBuf::from("/tmp/some-repo/.xavier/config.toml")
        );
        assert_eq!(
            root_of(Path::new("/tmp/some-repo/.xavier/config.toml")),
            PathBuf::from("/tmp/some-repo")
        );
    }
}
