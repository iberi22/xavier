//! Plan data schema, package hashing, and source inventory for the skill controller.
//!
//! D4: `plan` is the default operation and persists nothing. D2: one entry per selected
//! package directory, never a whole discovery directory. D3: the manifest is already parsed
//! and validated v1.
//!
//! This module handles package identity hashing, source inventory resolution, and manifest
//! digest calculation with deterministic ordering.

use super::manifest::{validate_package_frontmatter, Manifest, PlacementRecord, TargetMode, Tool};
use super::policy::{AuthorizedDir, FsPolicy, RootKind, PACKAGE_BOUNDS};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use walkdir::WalkDir;

/// Source and target roots already resolved to absolute paths, keyed by manifest id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedRoots {
    pub sources: BTreeMap<String, PathBuf>,
    pub targets: BTreeMap<String, PathBuf>,
}

/// How a destination entry looks right now: the precondition an apply must re-observe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DestinationState {
    Absent,
    Symlink { to: String },
    Unmanaged { kind: ForeignKind },
}

/// What the plan proposes for one placement in one target. `Conflict` is never applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanAction {
    Create,
    Refresh,
    Unchanged,
    Conflict,
}

/// Why an existing destination entry is left strictly alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForeignKind {
    UnmanagedFile,
    UnmanagedDirectory,
    LinkOutsideSources,
    Irregular,
}

/// One placement projected into one target; the destination is `<root>/<name>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PlanEntry {
    pub name: String,
    pub source: String,
    pub package: String,
    pub package_hash: String,
    pub target: String,
    pub tools: Vec<Tool>,
    pub root: String,
    pub mode: TargetMode,
    pub root_present: bool,
    pub observed: DestinationState,
    pub action: PlanAction,
}

/// An entry the manifest omits and the plan proposes to unlink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PlanRemoval {
    pub name: String,
    pub target: String,
    pub root: String,
    pub link: String,
}

/// An existing destination entry that is not controller-owned: reported, never touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ForeignEntry {
    pub name: String,
    pub target: String,
    pub root: String,
    pub kind: ForeignKind,
}

/// The immutable plan document: the whole precondition set `apply --plan <file>` re-checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ProjectionPlan {
    pub manifest_version: u64,
    pub manifest_hash: String,
    pub entries: Vec<PlanEntry>,
    pub removals: Vec<PlanRemoval>,
    pub foreign: Vec<ForeignEntry>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlannerError {
    #[error("manifest source '{0}' has no resolved directory")]
    UnresolvedSource(String),
    #[error("target '{0}' is not declared or has no resolved root")]
    UnknownTarget(String),
    #[error("source of package '{name}' is not usable: {detail}")]
    SourceRejected { name: String, detail: String },
    #[error("package '{name}' cannot be inventoried: {detail}")]
    Package { name: String, detail: String },
    #[error("package '{name}' does not match its placement: {detail}")]
    Frontmatter { name: String, detail: String },
    #[error("destination '{name}' in target '{target}' cannot be read: {detail}")]
    Destination {
        name: String,
        target: String,
        detail: String,
    },
    #[error("scan bound exceeded while planning the {scope}: {bound}")]
    BoundExceeded {
        scope: &'static str,
        bound: &'static str,
    },
    #[error("the saved plan is stale: {detail}")]
    Stale { detail: String },
    #[error("the plan cannot be rendered as JSON: {0}")]
    Render(String),
}

/// A package directory and the digest of its whole content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageIdentity {
    pub dir: PathBuf,
    pub hash: String,
}

/// Resolve a placement record to an authorized package directory and compute its identity hash.
pub fn resolve_package<'a>(
    placement: &'a PlacementRecord,
    policy: &FsPolicy,
    roots: &ResolvedRoots,
    cache: &mut BTreeMap<(&'a str, &'a str), PackageIdentity>,
) -> Result<PackageIdentity, PlannerError> {
    let key = (placement.source.as_str(), placement.path.as_str());
    if let Some(found) = cache.get(&key) {
        return Ok(found.clone());
    }
    let root = match roots.sources.get(&placement.source) {
        Some(r) => r,
        None => return Err(PlannerError::UnresolvedSource(placement.source.clone())),
    };
    let dir = policy
        .authorize(RootKind::Source, &root.join(&placement.path))
        .map_err(|e| PlannerError::SourceRejected {
            name: placement.name.clone(),
            detail: e.to_string(),
        })?;
    let identity = hash_package(&dir, placement)?;
    cache.insert(key, identity.clone());
    Ok(identity)
}

/// Digest the whole package, not just `SKILL.md`: sorted relative paths, entry types, executable bits and
/// bytes. Bounded like the secret scan; a member that is not a regular file or a directory (a link, a
/// socket) aborts, because it cannot be inventoried.
pub fn hash_package(
    dir: &AuthorizedDir,
    placement: &PlacementRecord,
) -> Result<PackageIdentity, PlannerError> {
    let rejected = |detail: String| PlannerError::Package {
        name: placement.name.clone(),
        detail,
    };
    dir.revalidate().map_err(|e| rejected(e.to_string()))?;
    let mut members: Vec<(String, u8, u8, Vec<u8>)> = Vec::new();
    let (mut total, mut nodes) = (0u64, 0usize);
    for entry in WalkDir::new(dir.path())
        .follow_links(false)
        .max_depth(PACKAGE_BOUNDS.0)
    {
        let entry = entry.map_err(|e| rejected(e.to_string()))?;
        nodes += 1;
        if nodes > PACKAGE_BOUNDS.1 {
            return Err(PlannerError::BoundExceeded {
                scope: "package",
                bound: "node count",
            });
        }
        let relative = relative_to(dir.path(), entry.path());
        let kind = entry.file_type();
        if kind.is_dir() {
            members.push((relative, b'd', 0, Vec::new()));
            continue;
        }
        if !kind.is_file() {
            return Err(rejected(format!(
                "{relative} is not a regular file or directory"
            )));
        }
        let meta = fs::symlink_metadata(entry.path()).map_err(|e| rejected(e.to_string()))?;
        if meta.len() > PACKAGE_BOUNDS.3 {
            return Err(PlannerError::BoundExceeded {
                scope: "package",
                bound: "member size",
            });
        }
        let bytes = fs::read(entry.path()).map_err(|e| rejected(e.to_string()))?;
        total += bytes.len() as u64;
        if total > PACKAGE_BOUNDS.2 {
            return Err(PlannerError::BoundExceeded {
                scope: "package",
                bound: "total bytes",
            });
        }
        members.push((relative, b'f', executable_bit(&meta), bytes));
    }
    dir.revalidate().map_err(|e| rejected(e.to_string()))?;
    members.sort_by(|a, b| a.0.cmp(&b.0));
    let (mut hasher, mut body) = (Sha256::new(), None);
    for (relative, tag, exec, bytes) in &members {
        hasher.update(relative.as_bytes());
        hasher.update([*tag, *exec]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
        if relative == "SKILL.md" {
            body = Some(bytes.clone());
        }
    }
    let body = match body {
        Some(b) => b,
        None => return Err(rejected("SKILL.md is missing".to_string())),
    };
    validate_package_frontmatter(placement, &String::from_utf8_lossy(&body)).map_err(|e| {
        PlannerError::Frontmatter {
            name: placement.name.clone(),
            detail: e.to_string(),
        }
    })?;
    Ok(PackageIdentity {
        dir: dir.path().to_path_buf(),
        hash: to_hex(&hasher.finalize()),
    })
}

/// Canonical digest of the manifest fields that change a plan, from sorted values, never `HashMap` order.
pub fn manifest_digest(manifest: &Manifest) -> String {
    let mut text = format!("v{}\n", manifest.version);
    let mut sources: Vec<(&str, &str)> = manifest
        .sources
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    sources.sort_unstable();
    for (id, dir) in sources {
        let _ = writeln!(text, "s\t{id}\t{dir}");
    }
    let mut targets: Vec<&super::manifest::TargetRecord> = manifest.targets.iter().collect();
    targets.sort_by(|a, b| a.id.cmp(&b.id));
    for target in targets {
        let mut tools: Vec<&str> = target.tools.iter().copied().map(tool_name).collect();
        tools.sort_unstable();
        let _ = writeln!(
            text,
            "t\t{}\t{}\t{:?}\t{}",
            target.id,
            target.root,
            target.mode,
            tools.join(",")
        );
    }
    for placement in sorted_placements(manifest) {
        let mut ids: Vec<&str> = placement.targets.iter().map(String::as_str).collect();
        ids.sort_unstable();
        let _ = writeln!(
            text,
            "p\t{}\t{}\t{}\t{}",
            placement.source,
            placement.path,
            placement.name,
            ids.join(",")
        );
    }
    digest(text.as_bytes())
}

/// Name-stable placement order, so entries and the manifest digest are reproducible.
pub fn sorted_placements(manifest: &Manifest) -> Vec<&PlacementRecord> {
    let mut placements: Vec<&PlacementRecord> = manifest.placements.iter().collect();
    placements.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.source.cmp(&b.source))
            .then(a.path.cmp(&b.path))
    });
    placements
}

/// Canonical lowercase name for a tool enum variant.
pub fn tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Codex => "codex",
        Tool::Claude => "claude",
        Tool::Opencode => "opencode",
        Tool::Gemini => "gemini",
        Tool::Agy => "agy",
        Tool::Hermes => "hermes",
        Tool::Openclaw => "openclaw",
    }
}

fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    to_hex(&hasher.finalize())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn relative_to(root: &Path, path: &Path) -> String {
    let stripped = match path.strip_prefix(root) {
        Ok(p) => p,
        Err(_) => path,
    };
    stripped
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(unix)]
fn executable_bit(meta: &fs::Metadata) -> u8 {
    use std::os::unix::fs::PermissionsExt;
    u8::from(meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable_bit(_meta: &fs::Metadata) -> u8 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_file(dir: &Path, relative: &str, content: &str) {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().expect("parent dir")).expect("create parent");
        fs::write(&path, content).expect("write file");
    }

    #[test]
    fn test_missing_or_unreadable_sources_produce_error_without_deletions() {
        let tmp = TempDir::new().expect("temp dir");
        let source_root = tmp.path().join("store");
        let target_root = tmp.path().join("targets");
        fs::create_dir_all(&source_root).expect("create source");
        fs::create_dir_all(&target_root).expect("create target");

        write_file(
            &target_root,
            "existing_skill/SKILL.md",
            "---\nname: existing\n---\n",
        );
        let target_file = target_root.join("existing_skill/SKILL.md");

        let policy = FsPolicy::new(
            std::slice::from_ref(&source_root),
            std::slice::from_ref(&target_root),
        )
        .expect("policy");

        let placement = PlacementRecord {
            source: "canonical".to_string(),
            path: "missing_pkg".to_string(),
            name: "missing_pkg".to_string(),
            targets: vec!["t1".to_string()],
        };

        // 1. Unresolved source in roots
        let empty_roots = ResolvedRoots::default();
        let mut cache = BTreeMap::new();
        let err = resolve_package(&placement, &policy, &empty_roots, &mut cache)
            .expect_err("should fail when source root not in roots");
        assert_eq!(err, PlannerError::UnresolvedSource("canonical".to_string()));
        assert!(
            target_file.exists(),
            "target files must not be deleted on unresolved source"
        );

        // 2. Unreadable / missing package directory on disk
        let roots = ResolvedRoots {
            sources: BTreeMap::from([("canonical".to_string(), source_root.clone())]),
            targets: BTreeMap::from([("t1".to_string(), target_root.clone())]),
        };
        let err2 = resolve_package(&placement, &policy, &roots, &mut cache)
            .expect_err("should fail when package dir does not exist on disk");
        assert!(
            matches!(err2, PlannerError::SourceRejected { ref name, .. } if name == "missing_pkg"),
            "expected SourceRejected, got {err2:?}"
        );
        assert!(
            target_file.exists(),
            "target files must not be deleted on missing source"
        );
    }

    #[test]
    fn test_package_identity_hash_and_cache() {
        let tmp = TempDir::new().expect("temp dir");
        let source_root = tmp.path().join("store");
        fs::create_dir_all(&source_root).expect("create source");

        write_file(
            &source_root,
            "demo/SKILL.md",
            "---\nname: demo\n---\n\nSkill body.\n",
        );
        write_file(&source_root, "demo/extra.txt", "extra content\n");

        let policy = FsPolicy::new(std::slice::from_ref(&source_root), &[]).expect("policy");
        let roots = ResolvedRoots {
            sources: BTreeMap::from([("src".to_string(), source_root.clone())]),
            targets: BTreeMap::new(),
        };

        let placement = PlacementRecord {
            source: "src".to_string(),
            path: "demo".to_string(),
            name: "demo".to_string(),
            targets: vec![],
        };

        let mut cache = BTreeMap::new();
        let ident1 = resolve_package(&placement, &policy, &roots, &mut cache).expect("resolve");
        assert_eq!(ident1.hash.len(), 64);
        assert_eq!(cache.len(), 1);

        // Cache hit
        let ident2 = resolve_package(&placement, &policy, &roots, &mut cache).expect("cache hit");
        assert_eq!(ident1, ident2);

        // Modification changes hash
        write_file(&source_root, "demo/extra.txt", "modified content\n");
        let mut fresh_cache = BTreeMap::new();
        let ident3 =
            resolve_package(&placement, &policy, &roots, &mut fresh_cache).expect("re-resolve");
        assert_ne!(ident1.hash, ident3.hash);
    }

    #[test]
    fn test_hash_package_frontmatter_and_missing_skill_md() {
        let tmp = TempDir::new().expect("temp dir");
        let source_root = tmp.path().join("store");
        fs::create_dir_all(&source_root).expect("create source");

        // Missing SKILL.md
        write_file(&source_root, "pkg1/other.txt", "no skill md\n");
        let policy = FsPolicy::new(std::slice::from_ref(&source_root), &[]).expect("policy");
        let dir1 = policy
            .authorize(RootKind::Source, &source_root.join("pkg1"))
            .expect("auth");
        let placement1 = PlacementRecord {
            source: "src".to_string(),
            path: "pkg1".to_string(),
            name: "pkg1".to_string(),
            targets: vec![],
        };
        let err1 = hash_package(&dir1, &placement1).expect_err("should fail without SKILL.md");
        assert!(
            matches!(err1, PlannerError::Package { ref detail, .. } if detail.contains("SKILL.md is missing"))
        );

        // Frontmatter mismatch
        write_file(
            &source_root,
            "pkg2/SKILL.md",
            "---\nname: different_name\n---\n",
        );
        let dir2 = policy
            .authorize(RootKind::Source, &source_root.join("pkg2"))
            .expect("auth");
        let placement2 = PlacementRecord {
            source: "src".to_string(),
            path: "pkg2".to_string(),
            name: "pkg2".to_string(),
            targets: vec![],
        };
        let err2 = hash_package(&dir2, &placement2).expect_err("frontmatter mismatch");
        assert!(matches!(err2, PlannerError::Frontmatter { .. }));
    }

    #[test]
    fn test_manifest_digest_deterministic_ordering() {
        use std::collections::HashMap;
        let m1 = Manifest {
            version: 1,
            sources: HashMap::from([
                ("b".to_string(), "/path/b".to_string()),
                ("a".to_string(), "/path/a".to_string()),
            ]),
            targets: vec![
                super::super::manifest::TargetRecord {
                    id: "t2".to_string(),
                    tools: vec![Tool::Opencode, Tool::Claude],
                    root: "/root/2".to_string(),
                    mode: TargetMode::Symlink,
                },
                super::super::manifest::TargetRecord {
                    id: "t1".to_string(),
                    tools: vec![Tool::Codex],
                    root: "/root/1".to_string(),
                    mode: TargetMode::Copy,
                },
            ],
            placements: vec![
                PlacementRecord {
                    source: "b".to_string(),
                    path: "p2".to_string(),
                    name: "n2".to_string(),
                    targets: vec!["t2".to_string()],
                },
                PlacementRecord {
                    source: "a".to_string(),
                    path: "p1".to_string(),
                    name: "n1".to_string(),
                    targets: vec!["t1".to_string()],
                },
            ],
            ephemeral: None,
            telemetry: None,
        };

        let digest1 = manifest_digest(&m1);
        let digest2 = manifest_digest(&m1);
        assert_eq!(digest1, digest2);
        assert_eq!(digest1.len(), 64);

        let sorted = sorted_placements(&m1);
        assert_eq!(sorted[0].name, "n1");
        assert_eq!(sorted[1].name, "n2");

        assert_eq!(tool_name(Tool::Claude), "claude");
        assert_eq!(tool_name(Tool::Codex), "codex");
    }
}
