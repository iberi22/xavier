//! Deterministic, write-free projection plans for the skill controller. D4: `plan` is the default
//! operation and persists nothing; the JSON it emits carries the preconditions `apply --plan <file>`
//! re-checks (docs/design/skill-controller/01-ARCHITECTURE-AND-ADR.md:124). D2: one entry per selected
//! package directory, never a whole discovery directory. D3: the manifest is already parsed and
//! validated v1. Entries, removals and unmanaged entries are sorted and the manifest digest is built
//! from sorted fields, so two plans over unchanged fixtures are byte-identical. This module writes
//! nothing and reads no environment variable (the caller resolves roots, so no `.env.example` key is
//! added); on a Tokio worker call it through `spawn_blocking`. Reads go through `FsPolicy::authorize`;
//! the apply path re-authorizes each target root before writing.

use super::manifest::{Manifest, Tool};
use super::plan::{
    manifest_digest, resolve_package, sorted_placements, tool_name, DestinationState, ForeignEntry,
    ForeignKind, PlanAction, PlanEntry, PlanRemoval, PlannerError, ProjectionPlan, ResolvedRoots,
};
use super::policy::{FsPolicy, RootKind, PACKAGE_BOUNDS};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Inventory every placement against every declared target; aborts with `Err` on a missing or unreadable
/// source instead of proposing deletions (01-ARCHITECTURE-AND-ADR.md:122).
pub fn build_plan(
    manifest: &Manifest,
    policy: &FsPolicy,
    roots: &ResolvedRoots,
) -> Result<ProjectionPlan, PlannerError> {
    manifest.validate().map_err(|e| PlannerError::Package {
        name: "manifest".into(),
        detail: e.to_string(),
    })?;
    let mut target_roots: BTreeMap<&str, (PathBuf, bool)> = BTreeMap::new();
    for target in &manifest.targets {
        let root = roots
            .targets
            .get(&target.id)
            .ok_or_else(|| PlannerError::UnknownTarget(target.id.clone()))?;
        target_roots.insert(
            target.id.as_str(),
            authorize_root(policy, &target.id, root)?,
        );
    }
    let (mut cache, mut by_target) = (BTreeMap::new(), BTreeMap::<&str, Vec<PlanEntry>>::new());
    for placement in sorted_placements(manifest) {
        let package = resolve_package(placement, policy, roots, &mut cache)?;
        let mut ids: Vec<&str> = placement.targets.iter().map(String::as_str).collect();
        ids.sort_unstable();
        for id in ids {
            let target = manifest
                .targets
                .iter()
                .find(|t| t.id == id)
                .ok_or_else(|| PlannerError::UnknownTarget(id.to_string()))?;
            let (root, present) = target_roots
                .get(id)
                .ok_or_else(|| PlannerError::UnknownTarget(id.to_string()))?;
            let observed = observe(policy, &root.join(&placement.name), &placement.name, id)?;
            let action = match &observed {
                Observed::Absent => PlanAction::Create,
                Observed::Owned { points_at, .. } if points_at == &package.dir => {
                    PlanAction::Unchanged
                }
                Observed::Owned { .. } => PlanAction::Refresh,
                Observed::Unmanaged { .. } => PlanAction::Conflict,
            };
            let mut tools = target.tools.clone();
            tools.sort_by_key(|tool| tool_name(*tool));
            by_target.entry(id).or_default().push(PlanEntry {
                name: placement.name.clone(),
                source: placement.source.clone(),
                package: placement.path.clone(),
                package_hash: package.hash.clone(),
                target: target.id.clone(),
                tools,
                root: display(root),
                mode: target.mode,
                root_present: *present,
                observed: observed.state(),
                action,
            });
        }
    }
    let (mut entries, mut removals, mut foreign) = (Vec::new(), Vec::new(), Vec::new());
    for (id, mut target_entries) in by_target {
        let (root, _) = target_roots
            .get(id)
            .cloned()
            .ok_or_else(|| PlannerError::UnknownTarget(id.to_string()))?;
        let desired: BTreeSet<&str> = target_entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        let (found_removals, found_foreign) = scan_target(policy, id, &root, &desired)?;
        removals.extend(found_removals);
        foreign.extend(found_foreign);
        entries.append(&mut target_entries);
    }
    Ok(ProjectionPlan {
        manifest_version: manifest.version,
        manifest_hash: manifest_digest(manifest),
        entries,
        removals,
        foreign,
    })
}

/// Re-derive every precondition of a saved plan; a changed package hash, destination identity or manifest
/// invalidates the plan, so the operator has to plan again before applying.
pub fn recheck(
    plan: &ProjectionPlan,
    manifest: &Manifest,
    policy: &FsPolicy,
    roots: &ResolvedRoots,
) -> Result<(), PlannerError> {
    let current = build_plan(manifest, policy, roots)?;
    if current == *plan {
        return Ok(());
    }
    let reason = drifted(&plan.entries, &current.entries, "entry")
        .or_else(|| drifted(&plan.removals, &current.removals, "removal"))
        .or_else(|| drifted(&plan.foreign, &current.foreign, "unmanaged entry"));
    Err(PlannerError::Stale {
        detail: reason.unwrap_or_else(|| "the plan changed".to_string()),
    })
}

/// Byte-stable JSON for the caller to save: `serde_json` keeps the field order, the collections are sorted.
pub fn to_json(plan: &ProjectionPlan) -> Result<String, PlannerError> {
    serde_json::to_string_pretty(plan).map_err(|e| PlannerError::Render(e.to_string()))
}

/// How a destination entry currently resolves inside an allowlisted source root.
#[derive(Debug)]
enum Observed {
    Absent,
    Owned { link: String, points_at: PathBuf },
    Unmanaged { kind: ForeignKind },
}

impl Observed {
    fn state(&self) -> DestinationState {
        match self {
            Self::Absent => DestinationState::Absent,
            Self::Owned { link, .. } => DestinationState::Symlink { to: link.clone() },
            Self::Unmanaged { kind } => DestinationState::Unmanaged { kind: *kind },
        }
    }
}

/// Authorize a declared target root; an absent root is not yet a directory, so it is recorded as
/// `root_present: false` for the apply path to create under its own authorization.
fn authorize_root(
    policy: &FsPolicy,
    id: &str,
    root: &Path,
) -> Result<(PathBuf, bool), PlannerError> {
    let unreadable = |detail: String| PlannerError::Destination {
        name: id.to_string(),
        target: id.to_string(),
        detail,
    };
    match fs::symlink_metadata(root) {
        Ok(_) => policy
            .authorize(RootKind::Target, root)
            .map(|dir| (dir.path().to_path_buf(), true))
            .map_err(|e| unreadable(e.to_string())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok((root.to_path_buf(), false)),
        Err(e) => Err(unreadable(e.to_string())),
    }
}

/// Classify one destination entry. Ownership requires the link to resolve inside an authorized source root:
/// without that proof the entry is a conflict, never a deletion.
fn observe(
    policy: &FsPolicy,
    destination: &Path,
    name: &str,
    target: &str,
) -> Result<Observed, PlannerError> {
    let unreadable = |detail: String| PlannerError::Destination {
        name: name.to_string(),
        target: target.to_string(),
        detail,
    };
    let meta = match fs::symlink_metadata(destination) {
        Ok(meta) => meta,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Observed::Absent),
        Err(e) => return Err(unreadable(e.to_string())),
    };
    let kind = meta.file_type();
    if !kind.is_symlink() {
        return Ok(Observed::Unmanaged {
            kind: if kind.is_file() {
                ForeignKind::UnmanagedFile
            } else if kind.is_dir() {
                ForeignKind::UnmanagedDirectory
            } else {
                ForeignKind::Irregular
            },
        });
    }
    let link = fs::read_link(destination).map_err(|e| unreadable(e.to_string()))?;
    let candidate = if link.is_absolute() {
        link.clone()
    } else {
        destination.parent().unwrap_or(destination).join(&link)
    };
    let points_at = match fs::canonicalize(&candidate) {
        Ok(resolved) => resolved,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Ok(Observed::Unmanaged {
                kind: ForeignKind::Irregular,
            })
        }
        Err(e) => return Err(unreadable(e.to_string())),
    };
    if policy.authorize(RootKind::Source, &points_at).is_err() {
        return Ok(Observed::Unmanaged {
            kind: ForeignKind::LinkOutsideSources,
        });
    }
    Ok(Observed::Owned {
        link: display(&link),
        points_at,
    })
}

/// Split a target root's existing entries: an omitted controller-owned link is a removal, anything else is
/// reported and left intact. Sorted, so the plan is reproducible.
fn scan_target(
    policy: &FsPolicy,
    id: &str,
    root: &Path,
    desired: &BTreeSet<&str>,
) -> Result<(Vec<PlanRemoval>, Vec<ForeignEntry>), PlannerError> {
    let unreadable = |detail: String| PlannerError::Destination {
        name: id.to_string(),
        target: id.to_string(),
        detail,
    };
    let children = match fs::read_dir(root) {
        Ok(children) => children,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok((Vec::new(), Vec::new())),
        Err(e) => return Err(unreadable(e.to_string())),
    };
    let (mut names, mut removals, mut foreign) = (Vec::<PathBuf>::new(), Vec::new(), Vec::new());
    for child in children {
        names.push(child.map_err(|e| unreadable(e.to_string()))?.path());
    }
    if names.len() > PACKAGE_BOUNDS.1 {
        return Err(PlannerError::BoundExceeded {
            scope: "target",
            bound: "entry count",
        });
    }
    names.sort();
    for path in names {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| display(&path));
        if desired.contains(name.as_str()) {
            continue;
        }
        match observe(policy, &path, &name, id)? {
            Observed::Absent => {}
            Observed::Owned { link, .. } => removals.push(PlanRemoval {
                name,
                target: id.to_string(),
                root: display(root),
                link,
            }),
            Observed::Unmanaged { kind } => foreign.push(ForeignEntry {
                name,
                target: id.to_string(),
                root: display(root),
                kind,
            }),
        }
    }
    Ok((removals, foreign))
}

fn drifted<T: PartialEq>(saved: &[T], current: &[T], what: &str) -> Option<String> {
    if saved.len() != current.len() {
        return Some(format!(
            "the {what} set changed from {} to {} entries",
            saved.len(),
            current.len()
        ));
    }
    saved
        .iter()
        .zip(current)
        .enumerate()
        .find(|(_, (was, now))| was != now)
        .map(|(index, _)| format!("{what} #{} changed", index + 1))
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;
    use walkdir::WalkDir;

    const MANIFEST: &str = concat!("version = 1\n[sources]\ncanonical = \"$XAVIER_SKILL_STORE\"\n",
        "[[targets]]\nid = \"t1\"\ntools = [\"codex\", \"claude\"]\nroot = \"$HOME/.claude/skills\"\nmode = \"symlink\"\n",
        "[[placements]]\nsource = \"canonical\"\npath = \"demo\"\nname = \"demo\"\ntargets = [\"t1\"]\n");

    /// `(tempdir, source root, target root, manifest, resolved roots)`
    fn fixture() -> (TempDir, PathBuf, PathBuf, Manifest, ResolvedRoots) {
        let tmp = TempDir::new().expect("tmp");
        let (source, target) = (tmp.path().join("store"), tmp.path().join("tools/skills"));
        fs::create_dir_all(&target).expect("mkdir target root");
        member(&source, "demo/SKILL.md", "---\nname: demo\n---\n\nBody.\n");
        member(&source, "demo/references/notes.md", "notes\n");
        member(
            &source,
            "retired/SKILL.md",
            "---\nname: retired\n---\n\nOld.\n",
        );
        let roots = ResolvedRoots {
            sources: BTreeMap::from([("canonical".to_string(), source.clone())]),
            targets: BTreeMap::from([("t1".to_string(), target.clone())]),
        };
        (
            tmp,
            source,
            target,
            Manifest::from_toml(MANIFEST).expect("manifest"),
            roots,
        )
    }
    fn policy_for(source: &Path, target: &Path) -> FsPolicy {
        FsPolicy::new(
            std::slice::from_ref(&source.to_path_buf()),
            std::slice::from_ref(&target.to_path_buf()),
        )
        .expect("policy")
    }
    fn member(dir: &Path, relative: &str, body: &str) {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, body).expect("write");
    }
    fn digest(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
    fn relative(root: &Path, path: &Path) -> String {
        path.strip_prefix(root)
            .unwrap_or(path)
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect::<Vec<_>>()
            .join("/")
    }
    /// Every path under `root` with its kind, link value or content digest.
    fn snapshot(root: &Path) -> Vec<(String, String)> {
        let kind_of = |entry: &walkdir::DirEntry| {
            let kind = entry.file_type();
            if kind.is_dir() {
                "dir".to_string()
            } else if kind.is_symlink() {
                format!(
                    "link:{}",
                    entry
                        .path()
                        .read_link()
                        .map(|p| display(&p))
                        .unwrap_or_default()
                )
            } else {
                format!(
                    "file:{}",
                    digest(&fs::read(entry.path()).unwrap_or_default())
                )
            }
        };
        let mut out: Vec<(String, String)> = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .map(|entry| (relative(root, entry.path()), kind_of(&entry)))
            .collect();
        out.sort();
        out
    }
    fn planned<'a>(plan: &'a ProjectionPlan, name: &str) -> &'a PlanEntry {
        plan.entries
            .iter()
            .find(|e| e.name == name)
            .expect("planned entry")
    }

    #[test]
    fn test_two_plans_over_unchanged_fixtures_are_byte_identical_and_write_nothing() {
        let (tmp, source, target, manifest, roots) = fixture();
        let (policy, before) = (policy_for(&source, &target), snapshot(tmp.path()));
        let first = build_plan(&manifest, &policy, &roots).expect("plan");
        let second = build_plan(&manifest, &policy, &roots).expect("plan");
        assert_eq!(
            to_json(&first).expect("json"),
            to_json(&second).expect("json"),
            "plans must be byte-identical"
        );
        assert_eq!(first, second);
        assert_eq!(
            before,
            snapshot(tmp.path()),
            "plan must not touch the filesystem"
        );
        let demo = planned(&first, "demo");
        assert_eq!(
            (
                demo.package.as_str(),
                demo.target.as_str(),
                demo.package_hash.len()
            ),
            ("demo", "t1", 64),
            "whole-package sha256 hex digest"
        );
        assert_eq!(
            (demo.action, &demo.observed),
            (PlanAction::Create, &DestinationState::Absent)
        );
        assert!(demo.root_present && demo.root.ends_with("tools/skills"));
        assert_eq!(
            demo.tools,
            vec![Tool::Claude, Tool::Codex],
            "tools are sorted, not manifest-ordered"
        );
        assert!(first.removals.is_empty() && first.foreign.is_empty());
    }

    #[test]
    fn test_changed_package_or_destination_identity_invalidates_the_saved_plan() {
        let (_tmp, source, target, manifest, roots) = fixture();
        let (policy, stale) = (policy_for(&source, &target), source.join("retired"));
        let plan = build_plan(&manifest, &policy, &roots).expect("plan");
        recheck(&plan, &manifest, &policy, &roots)
            .expect("an unchanged fixture keeps the plan valid");
        let mut misreported = plan.clone();
        misreported.entries[0].package_hash = "0".repeat(64);
        assert!(
            matches!(
                recheck(&misreported, &manifest, &policy, &roots),
                Err(PlannerError::Stale { .. })
            ),
            "a hash that no longer matches the package"
        );
        member(&source, "demo/references/extra.md", "extra\n");
        assert!(
            matches!(
                recheck(&plan, &manifest, &policy, &roots),
                Err(PlannerError::Stale { .. })
            ),
            "a changed package"
        );
        let projected = build_plan(&manifest, &policy, &roots).expect("plan");
        symlink(&stale, target.join("demo")).expect("link");
        assert!(
            matches!(
                recheck(&projected, &manifest, &policy, &roots),
                Err(PlannerError::Stale { .. })
            ),
            "a changed destination identity"
        );
        let refreshed = build_plan(&manifest, &policy, &roots).expect("plan");
        assert_eq!(
            (
                planned(&refreshed, "demo").action,
                planned(&refreshed, "demo").observed.clone()
            ),
            (
                PlanAction::Refresh,
                DestinationState::Symlink {
                    to: display(&stale)
                }
            )
        );
        fs::remove_file(target.join("demo")).expect("unlink");
        assert!(
            matches!(
                recheck(&refreshed, &manifest, &policy, &roots),
                Err(PlannerError::Stale { .. })
            ),
            "a vanished destination invalidates the plan that recorded it"
        );
    }

    #[test]
    fn test_missing_source_errors_without_proposing_deletions() {
        let (_tmp, source, target, manifest, roots) = fixture();
        let policy = policy_for(&source, &target);
        // An omitted link into the store: the only removal this manifest can ever propose.
        let owned = source.join("retired");
        symlink(&owned, target.join("old")).expect("link");
        let plan = build_plan(&manifest, &policy, &roots).expect("plan");
        assert_eq!(plan.removals.len(), 1);
        let removal = &plan.removals[0];
        assert_eq!(
            (
                removal.name.as_str(),
                removal.link.as_str(),
                removal.target.as_str()
            ),
            ("old", display(&owned).as_str(), "t1")
        );
        assert!(removal.root.ends_with("tools/skills"));
        fs::remove_dir_all(source.join("demo")).expect("remove the package");
        assert!(matches!(
            build_plan(&manifest, &policy, &roots),
            Err(PlannerError::SourceRejected { .. })
        ));
        assert!(
            matches!(
                recheck(&plan, &manifest, &policy, &roots),
                Err(PlannerError::SourceRejected { .. })
            ),
            "no plan, so no deletion"
        );
        fs::remove_dir_all(&source).expect("remove the source root");
        let error = build_plan(&manifest, &policy, &roots)
            .expect_err("a missing source aborts instead of deleting");
        assert!(
            matches!(error, PlannerError::SourceRejected { .. }),
            "{error:?}"
        );
        assert!(
            fs::symlink_metadata(target.join("old")).is_ok(),
            "the existing link is left intact"
        );
    }

    #[test]
    fn test_unmanaged_entries_are_conflicts_and_never_removals() {
        let (tmp, source, target, manifest, roots) = fixture();
        let policy = policy_for(&source, &target);
        member(
            &target,
            "hand-written/SKILL.md",
            "---\nname: hand-written\n---\n",
        );
        member(&target, "demo/SKILL.md", "---\nname: demo\n---\n");
        member(
            tmp.path(),
            "elsewhere/SKILL.md",
            "---\nname: foreign\n---\n",
        );
        symlink(tmp.path().join("elsewhere"), target.join("elsewhere")).expect("link");
        let plan = build_plan(&manifest, &policy, &roots).expect("plan");
        let demo = planned(&plan, "demo");
        assert_eq!(
            (demo.action, demo.observed.clone()),
            (
                PlanAction::Conflict,
                DestinationState::Unmanaged {
                    kind: ForeignKind::UnmanagedDirectory
                }
            ),
            "an unmanaged directory collides"
        );
        assert!(
            plan.removals.is_empty(),
            "no unmanaged entry is ever a removal"
        );
        let foreign: Vec<(&str, ForeignKind)> = plan
            .foreign
            .iter()
            .map(|f| (f.name.as_str(), f.kind))
            .collect();
        assert!(
            foreign.contains(&("hand-written", ForeignKind::UnmanagedDirectory))
                && foreign.contains(&("elsewhere", ForeignKind::LinkOutsideSources))
        );
    }
}
