//! Filesystem authorization and native secret scanning for the skill controller.
//!
//! D5: owner-supplied source/target roots that repository content cannot widen.
//! D10: whole-package secret scan before publication, redacted findings (D10).
//! Never mutates: the projector owns the journalled write.

use regex::RegexSet;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use thiserror::Error;
use walkdir::WalkDir;

/// Bounding tuple for a package walk, in the order the guards read it:
/// `(max depth, max nodes, total bytes, bytes per member, max symlink hops)`.
pub const PACKAGE_BOUNDS: (usize, usize, u64, u64, u32) = (8, 512, 4 << 20, 1 << 20, 4);
#[rustfmt::skip]
const _: () = assert!(PACKAGE_BOUNDS.0 > 0 && PACKAGE_BOUNDS.1 > 0 && PACKAGE_BOUNDS.4 > 0 && PACKAGE_BOUNDS.3 > 0 && PACKAGE_BOUNDS.2 >= PACKAGE_BOUNDS.3);

/// High-signal secret classes mirroring `scripts/check-secrets.sh:23`. A finding
/// carries class and relative location only; matched bytes are never collected.
#[rustfmt::skip]
pub const SECRET_CLASSES: &[(&str, &str)] = &[
    ("openai_or_router_key", r"sk-or-v1-[A-Za-z0-9]{20,}"),
    ("github_pat", r"ghp_[A-Za-z0-9]{20,}"),
    ("aws_access_key_id", r"AKIA[0-9A-Z]{16}"),
    ("slack_token", r"xox[baprs]-[A-Za-z0-9-]{10,}"),
    ("google_api_key", r"AIza[0-9A-Za-z_-]{30,}"),
    ("private_key_block", r"BEGIN (RSA|OPENSSH|EC|DSA) PRIVATE KEY"),
    ("xavier_auth_token", r"XAVIER_TOKEN=[a-zA-Z0-9]{8,}"),
    ("xavier_supabase_key", r"XAVIER_SUPABASE_KEY=[a-zA-Z0-9]{8,}"),
];

/// Which owner-supplied allowlist a path is checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[rustfmt::skip]
pub enum RootKind { Source, Target }
#[rustfmt::skip]
impl RootKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Target => "target",
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
#[rustfmt::skip]
pub enum PolicyError {
    #[error("no {kind} roots are authorized by the host policy")] EmptyAllowlist { kind: &'static str },
    #[error("authorized {kind} root is not a readable directory: {detail}")] InvalidRoot { kind: &'static str, detail: String },
    #[error("secret patterns are invalid: {detail}")] InvalidPatterns { detail: String },
    #[error("{path} resolves outside every allowed {kind} root")] OutsideAllowlist { kind: &'static str, path: String },
    #[error("{path} contains a forbidden '..' component")] Traversal { path: String },
    #[error("{path} is reached through a symlinked component; links are not authorized here")] SymlinkComponent { path: String },
    #[error("{path} is missing or unreadable: {detail}")] Unreadable { path: String, detail: String },
    #[error("{path} was replaced while it was being checked")] SwappedPath { path: String },
    #[error("a stable object identity is unavailable for {path}; refusing the operation")] NoIdentity { path: String },
    #[error("special file is not publishable: {path}")] SpecialFile { path: String },
    #[error("package member has unexpected hardlinks: {path}")] Hardlinked { path: String },
    #[error("package exceeds the {bound} scan bound")] BoundExceeded { bound: &'static str },
    #[error("secret scan blocked publication: {summary}")] SecretsBlocked { summary: String },
}

/// Owner-supplied allowlists of exact source and target roots.
#[derive(Debug, Clone, Default)]
#[rustfmt::skip]
pub struct FsPolicy { sources: Vec<PathBuf>, targets: Vec<PathBuf> }

impl FsPolicy {
    /// Canonicalize each root once; it must already be a real readable directory.
    pub fn new(sources: &[PathBuf], targets: &[PathBuf]) -> Result<Self, PolicyError> {
        let sources = canonical_roots(RootKind::Source, sources)?;
        let targets = canonical_roots(RootKind::Target, targets)?;
        Ok(Self { sources, targets })
    }

    #[rustfmt::skip]
    fn roots(&self, kind: RootKind) -> &[PathBuf] { match kind { RootKind::Source => &self.sources, RootKind::Target => &self.targets } }

    /// Reject `..`, reject a symlinked parent component, require the resolved path
    /// inside an allowed root, then pin the resolved directory by open handle.
    #[rustfmt::skip]
    pub fn authorize(&self, kind: RootKind, path: &Path) -> Result<AuthorizedDir, PolicyError> {
        let roots = self.roots(kind);
        if roots.is_empty() { return Err(PolicyError::EmptyAllowlist { kind: kind.as_str() }); }
        if path.components().any(|c| matches!(c, Component::ParentDir)) { return Err(PolicyError::Traversal { path: display(path) }); }
        let resolved = fs::canonicalize(path).map_err(|e| unreadable(display(path), e))?;
        let escaped = || PolicyError::OutsideAllowlist { kind: kind.as_str(), path: display(&resolved) };
        let root = roots.iter().find(|r| resolved.starts_with(r)).ok_or_else(escaped)?;
        // No-follow walk of the *literal* chain: `canonicalize` hides a link, so a
        // parent swapped in after the allowlist was built is inspected by name. A
        // link is tolerated only when it lands inside the allowlisted root.
        let mut cursor = PathBuf::new();
        for component in path.components() {
            cursor.push(component);
            let entry = meta(&cursor, false).map_err(|e| unreadable(display(&cursor), e))?;
            if entry.file_type().is_symlink() {
                let lands = fs::canonicalize(&cursor).map(|c| root.starts_with(c.as_path())).unwrap_or(false);
                if !lands { return Err(PolicyError::SymlinkComponent { path: display(&cursor) }); }
                continue;
            }
            if !entry.is_dir() { return Err(PolicyError::SpecialFile { path: display(&cursor) }); }
        }
        if !resolved.is_dir() { return Err(PolicyError::SpecialFile { path: display(&resolved) }); }
        let (file, id) = open_dir(&resolved)?;
        Ok(AuthorizedDir { path: resolved, file, id })
    }
}

/// A directory that passed containment checks and is pinned by an open handle.
#[derive(Debug)]
#[rustfmt::skip]
pub struct AuthorizedDir { path: PathBuf, file: File, id: DirId }

impl AuthorizedDir {
    /// The canonical path inside the allowlist.
    #[rustfmt::skip]
    pub fn path(&self) -> &Path { &self.path }

    /// Confirm the handle and the path still name the same object, catching a
    /// directory swapped after authorization.
    #[rustfmt::skip]
    pub fn revalidate(&self) -> Result<(), PolicyError> { if open_dir(&self.path)?.1 != self.id { return Err(PolicyError::SwappedPath { path: display(&self.path) }); } Ok(()) }
}

/// One redacted finding: the secret class plus a package-relative location.
#[derive(Debug, Clone, PartialEq, Eq)]
#[rustfmt::skip]
pub struct SecretFinding { pub class: &'static str, pub location: String }

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[rustfmt::skip]
pub struct PackageScan { pub findings: Vec<SecretFinding>, pub files_scanned: usize, pub bytes_scanned: u64 }

impl fmt::Display for PackageScan {
    /// Redacted by construction: `<class> at <relative path>` pairs only.
    #[rustfmt::skip]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, finding) in self.findings.iter().enumerate() { if i > 0 { f.write_str("; ")?; } write!(f, "{} at {}", finding.class, finding.location)?; }
        Ok(())
    }
}

/// Publication is blocked on any finding (D10).
#[rustfmt::skip]
impl PackageScan { pub fn blocks_publication(&self) -> bool { !self.findings.is_empty() } }

/// A validated publication intent. `dry_run` is the default: it reports the plan
/// and touches nothing, even when the scan found secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
#[rustfmt::skip]
pub struct PublicationPlan { pub package: PathBuf, pub targets: Vec<PathBuf>, pub dry_run: bool, pub scan: PackageScan }

/// Authorize the package and every target against their allowlist, then scan the
/// whole package. A dry run returns the plan as-is; a secret otherwise aborts.
#[rustfmt::skip]
pub fn plan_publication(policy: &FsPolicy, package_dir: &Path, target_dirs: &[PathBuf], dry_run: bool) -> Result<PublicationPlan, PolicyError> {
    let package = policy.authorize(RootKind::Source, package_dir)?;
    let mut targets = Vec::with_capacity(target_dirs.len());
    for target in target_dirs { targets.push(policy.authorize(RootKind::Target, target)?.path().to_path_buf()); }
    let scan = scan_package(&package)?;
    if !dry_run && scan.blocks_publication() { return Err(PolicyError::SecretsBlocked { summary: scan.to_string() }); }
    Ok(PublicationPlan { package: package.path().to_path_buf(), targets, dry_run, scan })
}

/// Scan every regular file of an authorized package, bounded by depth, nodes and
/// bytes. Special files, escaped or cyclic symlinks, hardlinks and unreadable
/// members are rejected, never skipped: an unscannable package is not publishable.
#[rustfmt::skip]
pub fn scan_package(dir: &AuthorizedDir) -> Result<PackageScan, PolicyError> {
    dir.revalidate()?;
    let set = secret_set()?;
    let mut scan = PackageScan::default();
    let mut nodes = 0usize;
    let walk = WalkDir::new(dir.path()).follow_links(false).max_depth(PACKAGE_BOUNDS.0 + 1);
    for entry in walk {
        let entry = entry.map_err(|e| PolicyError::Unreadable { path: display(dir.path()), detail: e.to_string() })?;
        nodes += 1;
        if nodes > PACKAGE_BOUNDS.1 { return Err(PolicyError::BoundExceeded { bound: "node count" }); }
        if entry.depth() > PACKAGE_BOUNDS.0 { return Err(PolicyError::BoundExceeded { bound: "directory depth" }); }
        let relative = relative_to(dir.path(), entry.path());
        let entry_meta = member_meta(dir.path(), entry.path(), &relative)?;
        if entry_meta.is_dir() { continue; }
        if !entry_meta.is_file() { return Err(PolicyError::SpecialFile { path: relative }); }
        if link_count(&entry_meta) > 1 { return Err(PolicyError::Hardlinked { path: relative }); }
        let bytes = read_member(entry.path(), &relative)?;
        scan.bytes_scanned += bytes.len() as u64;
        if scan.bytes_scanned > PACKAGE_BOUNDS.2 { return Err(PolicyError::BoundExceeded { bound: "total bytes" }); }
        scan.files_scanned += 1;
        // `RegexSet::matches` yields each matching pattern index at most once.
        let text = String::from_utf8_lossy(&bytes);
        for index in set.matches(text.as_ref()) {
            scan.findings.push(SecretFinding { class: SECRET_CLASSES[index].0, location: relative.clone() });
        }
    }
    dir.revalidate()?;
    Ok(scan)
}

type DirId = (u64, u64);

#[rustfmt::skip]
fn secret_set() -> Result<&'static RegexSet, PolicyError> {
    static SET: OnceLock<Result<RegexSet, String>> = OnceLock::new();
    let built = SET.get_or_init(|| RegexSet::new(SECRET_CLASSES.iter().map(|(_, p)| *p)).map_err(|e| e.to_string()));
    built.as_ref().map_err(|d| PolicyError::InvalidPatterns { detail: d.clone() })
}

#[cfg(unix)]
#[rustfmt::skip]
fn identity(meta: &fs::Metadata) -> Option<DirId> { use std::os::unix::fs::MetadataExt; Some((meta.dev(), meta.ino())) }
#[cfg(unix)]
#[rustfmt::skip]
fn link_count(meta: &fs::Metadata) -> u64 { use std::os::unix::fs::MetadataExt; meta.nlink() }
/// No stable identity without a unix `dev`/`ino`: refuse rather than guess.
#[cfg(not(unix))]
#[rustfmt::skip]
fn identity(_meta: &fs::Metadata) -> Option<DirId> { None }
#[cfg(not(unix))]
#[rustfmt::skip]
fn link_count(_meta: &fs::Metadata) -> u64 { 1 }

/// Pin a directory `(dev, ino)` and confirm its name still resolves to that object.
#[rustfmt::skip]
fn open_dir(path: &Path) -> Result<(File, DirId), PolicyError> {
    let file = File::open(path).map_err(|e| unreadable(display(path), e))?;
    let id = identity(&file.metadata().map_err(|e| unreadable(display(path), e))?).ok_or_else(|| PolicyError::NoIdentity { path: display(path) })?;
    if identity(&meta(path, true).map_err(|e| unreadable(display(path), e))?) != Some(id) { return Err(PolicyError::SwappedPath { path: display(path) }); }
    Ok((file, id))
}

/// Resolve a member without following links out of the package.
#[rustfmt::skip]
fn member_meta(root: &Path, path: &Path, relative: &str) -> Result<fs::Metadata, PolicyError> {
    let mut cursor = path.to_path_buf();
    for hop in 0..=PACKAGE_BOUNDS.4 {
        let entry = meta(&cursor, false).map_err(|e| unreadable(relative.into(), e))?;
        if !entry.file_type().is_symlink() { return Ok(entry); }
        if hop == PACKAGE_BOUNDS.4 { break; }
        let target = fs::canonicalize(&cursor).map_err(|e| unreadable(relative.into(), e))?;
        if !target.starts_with(root) { return Err(PolicyError::OutsideAllowlist { kind: "package", path: relative.into() }); }
        cursor = target;
    }
    Err(PolicyError::BoundExceeded { bound: "symlink hops" })
}

/// Read a member by handle, refusing non-regular files and proving the opened
/// object is still the one its name resolves to.
#[rustfmt::skip]
fn read_member(path: &Path, relative: &str) -> Result<Vec<u8>, PolicyError> {
    let file = File::open(path).map_err(|e| unreadable(relative.into(), e))?;
    let opened = file.metadata().map_err(|e| unreadable(relative.into(), e))?;
    if !opened.is_file() { return Err(PolicyError::SpecialFile { path: relative.into() }); }
    let held = identity(&opened).ok_or_else(|| PolicyError::NoIdentity { path: relative.into() })?;
    if identity(&meta(path, true).map_err(|e| unreadable(relative.into(), e))?) != Some(held) { return Err(PolicyError::SwappedPath { path: relative.into() }); }
    if opened.len() > PACKAGE_BOUNDS.3 { return Err(PolicyError::BoundExceeded { bound: "member size" }); }
    let mut buffer = Vec::new();
    file.take(PACKAGE_BOUNDS.3 + 1).read_to_end(&mut buffer).map_err(|e| unreadable(relative.into(), e))?;
    Ok(buffer)
}

#[rustfmt::skip]
fn meta(path: &Path, follow: bool) -> Result<fs::Metadata, PolicyError> {
    let read = if follow { fs::metadata(path) } else { fs::symlink_metadata(path) };
    read.map_err(|e| unreadable(display(path), e))
}
#[rustfmt::skip]
fn unreadable(path: String, e: impl fmt::Display) -> PolicyError { PolicyError::Unreadable { path, detail: e.to_string() } }
#[rustfmt::skip]
fn display(path: &Path) -> String { path.to_string_lossy().into_owned() }
#[rustfmt::skip]
fn relative_to(root: &Path, path: &Path) -> String { let parts: Vec<&str> = path.strip_prefix(root).unwrap_or(path).components().filter_map(|c| c.as_os_str().to_str()).collect(); if parts.is_empty() { ".".into() } else { parts.join("/") } }

#[rustfmt::skip]
fn canonical_roots(kind: RootKind, roots: &[PathBuf]) -> Result<Vec<PathBuf>, PolicyError> {
    let invalid = |detail: String| PolicyError::InvalidRoot { kind: kind.as_str(), detail };
    let mut out = Vec::with_capacity(roots.len());
    for root in roots {
        let canonical = fs::canonicalize(root).map_err(|e| invalid(format!("{}: {e}", display(root))))?;
        if !canonical.is_dir() { return Err(invalid(format!("{} is not a directory", display(root)))); }
        out.push(canonical);
    }
    Ok(out)
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    /// `(tempdir, source root, target root, clean package dir)`
    fn fixture() -> (TempDir, PathBuf, PathBuf, PathBuf) {
        let tmp = TempDir::new().expect("tmp");
        let source = tmp.path().join("store");
        let target = tmp.path().join("tools/skills");
        let package = source.join("demo");
        fs::create_dir_all(target.join("demo")).expect("mkdir target entry");
        member(&package, "SKILL.md", "---\nname: demo\n---\n\nBody.\n");
        member(&package, "references/notes.md", "plain notes\n");
        (tmp, source, target, package)
    }
    fn member(dir: &Path, relative: &str, body: &str) -> PathBuf { let path = dir.join(relative); fs::create_dir_all(path.parent().expect("parent")).expect("mkdir"); fs::write(&path, body).expect("write"); path }
    /// A fake credential assembled at runtime, so no fixture holds secret bytes.
    fn fake_google_key() -> String {
        let mut key = String::from("AIza");
        for _ in 0..10 { key.push_str("SyD"); }
        key
    }
    fn plan_err(policy: &FsPolicy, package: &Path, targets: &[PathBuf]) -> PolicyError {
        match plan_publication(policy, package, targets, true) { Err(e) => e, Ok(p) => panic!("expected a policy error, got plan {p:?}") }
    }
    #[test]
    fn test_authorize_accepts_package_and_targets_under_roots() {
        let (_tmp, source, target, package) = fixture();
        let policy = FsPolicy::new(&[source], std::slice::from_ref(&target)).expect("policy");
        let plan = plan_publication(&policy, &package, &[target.join("demo")], true).expect("plan");
        assert!(plan.dry_run && !plan.scan.blocks_publication() && plan.scan.files_scanned == 2);
        assert!(plan.package.ends_with("store/demo") && plan.targets[0].ends_with("tools/skills/demo"));
    }
    #[test]
    fn test_rejects_root_escape_traversal_and_package_member_escape() {
        let (tmp, source, target, package) = fixture();
        let outside = tmp.path().join("outside/demo");
        member(&outside, "SKILL.md", "---\nname: demo\n---\n");
        let policy = FsPolicy::new(std::slice::from_ref(&source), &[target]).expect("policy");
        symlink(&outside, source.join("linked")).expect("symlink");
        assert!(matches!(policy.authorize(RootKind::Source, &source.join("linked")), Err(PolicyError::OutsideAllowlist { kind: "source", .. })));
        assert!(matches!(plan_err(&policy, &outside, &[]), PolicyError::OutsideAllowlist { .. }));
        assert!(matches!(plan_err(&policy, &source.join("..").join("outside/demo"), &[]), PolicyError::Traversal { .. }));
        member(tmp.path(), "outside.md", "outside\n");
        symlink(tmp.path().join("outside.md"), package.join("references/link.md")).expect("symlink");
        assert!(matches!(plan_err(&policy, &package, &[]), PolicyError::OutsideAllowlist { kind: "package", ref path } if path == "references/link.md"));
    }
    #[test]
    fn test_rejects_symlinked_parent_inside_root() {
        let (_tmp, source, target, package) = fixture();
        let real = source.join("demo-real");
        fs::rename(&package, &real).expect("rename");
        symlink(&real, &package).expect("symlink");
        let policy = FsPolicy::new(&[source], &[target]).expect("policy");
        assert!(matches!(policy.authorize(RootKind::Source, &package), Err(PolicyError::SymlinkComponent { .. })));
    }
    #[test]
    fn test_rejects_special_hardlinked_and_unreadable_members() {
        let (tmp, source, target, package) = fixture();
        let policy = FsPolicy::new(&[source], &[target]).expect("policy");
        let socket = package.join("references/pipe");
        std::os::unix::net::UnixListener::bind(&socket).expect("bind");
        assert!(matches!(plan_err(&policy, &package, &[]), PolicyError::SpecialFile { ref path } if path == "references/pipe"));
        fs::remove_file(&socket).expect("cleanup");
        member(tmp.path(), "other.md", "linked\n");
        fs::hard_link(tmp.path().join("other.md"), package.join("references/linked.md")).expect("hard link");
        assert!(matches!(plan_err(&policy, &package, &[]), PolicyError::Hardlinked { ref path } if path == "references/linked.md"));
        fs::remove_file(package.join("references/linked.md")).expect("cleanup");
        symlink(package.join("references/missing.md"), package.join("references/gone.md")).expect("symlink");
        assert!(matches!(plan_err(&policy, &package, &[]), PolicyError::Unreadable { ref path, .. } if path == "references/gone.md"));
    }
    #[test]
    fn test_secret_in_references_blocks_publication_with_class_and_location_only() {
        let (_tmp, source, target, package) = fixture();
        let key = fake_google_key();
        member(&package, "references/notes.md", &format!("key = {key}\n"));
        let policy = FsPolicy::new(std::slice::from_ref(&source), std::slice::from_ref(&target)).expect("policy");
        let error = match plan_publication(&policy, &package, &[target.join("demo")], false) { Err(e) => e, Ok(p) => panic!("publication must be blocked, got plan {p:?}") };
        let PolicyError::SecretsBlocked { ref summary } = error else { panic!("expected SecretsBlocked, got {error:?}") };
        assert_eq!(summary, "google_api_key at references/notes.md");
        // No secret bytes and no absolute source path in the redacted report.
        assert!(!summary.contains(&key) && !error.to_string().contains(&key));
        assert!(!error.to_string().contains(&source.to_string_lossy().into_owned()));
        assert_eq!(fs::read_dir(target.join("demo")).expect("read").count(), 0, "nothing was published");
    }
    #[test]
    fn test_dry_run_is_default_and_reports_without_touching_disk() {
        let (_tmp, source, target, package) = fixture();
        let key = fake_google_key();
        member(&package, "scripts/run.sh", &format!("export K={key}\n"));
        let policy = FsPolicy::new(&[source], std::slice::from_ref(&target)).expect("policy");
        let destination = target.join("demo");
        let plan = plan_publication(&policy, &package, std::slice::from_ref(&destination), true).expect("dry run");
        assert!(plan.dry_run && plan.scan.blocks_publication());
        assert_eq!(plan.scan.findings, vec![SecretFinding { class: "google_api_key", location: "scripts/run.sh".into() }]);
        assert_eq!(plan.scan.to_string(), "google_api_key at scripts/run.sh");
        assert_eq!(fs::read_dir(&destination).expect("read").count(), 0, "dry run must not create anything");
    }
    #[test]
    fn test_rejects_empty_allowlist_and_unauthorized_target() {
        let (_tmp, source, _target, package) = fixture();
        let policy = FsPolicy::new(&[source], &[]).expect("policy");
        assert!(matches!(policy.authorize(RootKind::Target, &package), Err(PolicyError::EmptyAllowlist { kind: "target" })));
        assert!(matches!(plan_err(&policy, &package, std::slice::from_ref(&package)), PolicyError::EmptyAllowlist { kind: "target" }));
    }
}
