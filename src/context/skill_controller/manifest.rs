use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Codex,
    Claude,
    Opencode,
    Gemini,
    Agy,
    Hermes,
    Openclaw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetMode {
    Symlink,
    Copy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRecord {
    pub id: String,
    pub tools: Vec<Tool>,
    pub root: String,
    pub mode: TargetMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementRecord {
    pub source: String,
    pub path: String,
    pub name: String,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EphemeralConfig {
    pub enabled: bool,
    pub max_tokens: usize,
    pub ttl_seconds: u64,
}
impl Default for EphemeralConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_tokens: 4000,
            ttl_seconds: 86400,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub retention_days: u32,
}
impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u64,
    pub sources: HashMap<String, String>,
    #[serde(default)]
    pub targets: Vec<TargetRecord>,
    #[serde(default)]
    pub placements: Vec<PlacementRecord>,
    #[serde(default)]
    pub ephemeral: Option<EphemeralConfig>,
    #[serde(default)]
    pub telemetry: Option<TelemetryConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetConflict {
    pub target: String,
    pub name: String,
    pub src1: String,
    pub p1: String,
    pub src2: String,
    pub p2: String,
}
impl std::fmt::Display for TargetConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "'{}' in target '{}' ('{}/{}' vs '{}/{}')",
            self.name, self.target, self.src1, self.p1, self.src2, self.p2
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageConflict {
    pub name: String,
    pub src1: String,
    pub p1: String,
    pub src2: String,
    pub p2: String,
}
impl std::fmt::Display for PackageConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "'{}' across sources ('{}/{}' vs '{}/{}')",
            self.name, self.src1, self.p1, self.src2, self.p2
        )
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("failed to parse TOML manifest: {0}")]
    TomlParse(String),
    #[error("unsupported manifest version {0}, expected 1")]
    UnsupportedVersion(u64),
    #[error("invalid slug '{0}': must be 1-64 lowercase alphanumeric or -/_")]
    InvalidSlug(String),
    #[error("sources table cannot be empty")]
    EmptySources,
    #[error("source directory for '{0}' cannot be empty")]
    EmptySourceDir(String),
    #[error("targets list cannot be empty")]
    EmptyTargets,
    #[error("duplicate target ID '{0}'")]
    DuplicateTargetId(String),
    #[error("target '{0}' must have at least one tool")]
    EmptyTargetTools(String),
    #[error("target root for '{0}' cannot be empty")]
    EmptyTargetRoot(String),
    #[error("unknown source '{0}' in placement '{1}'")]
    UnknownSource(String, String),
    #[error("unknown target '{0}' in placement '{1}'")]
    UnknownTarget(String, String),
    #[error("placement '{0}' must specify at least one target")]
    EmptyPlacementTargets(String),
    #[error("package path '{0}' cannot be empty")]
    EmptyPackagePath(String),
    #[error("package path '{0}' must be relative, not absolute")]
    AbsolutePath(String),
    #[error("package path '{0}' contains forbidden path traversal ('..')")]
    PathTraversal(String),
    #[error("package path '{0}' contains forbidden glob pattern")]
    GlobPattern(String),
    #[error("placement name '{placement}' does not match package folder '{folder}'")]
    PlacementNameMismatch { placement: String, folder: String },
    #[error("conflicting target name: {0}")]
    ConflictingTargetName(Box<TargetConflict>),
    #[error("conflicting package name: {0}")]
    ConflictingPackageName(Box<PackageConflict>),
    #[error("frontmatter name '{fm}' does not match placement name '{placement}'")]
    FrontmatterNameMismatch { fm: String, placement: String },
    #[error("package '{0}' is missing YAML frontmatter or 'name' field in SKILL.md")]
    MissingFrontmatterName(String),
    #[error("invalid ephemeral configuration: {0}")]
    InvalidEphemeral(String),
    #[error("invalid telemetry configuration: {0}")]
    InvalidTelemetry(String),
}

fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn contains_glob(p: &str) -> bool {
    p.chars()
        .any(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}'))
}
fn has_path_traversal(p: &str) -> bool {
    p.split(['/', '\\']).any(|s| s == "..")
}
fn is_absolute_path(p: &str) -> bool {
    p.starts_with('/') || p.starts_with('\\') || p.starts_with('~') || Path::new(p).is_absolute()
}

fn frontmatter_name_value(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let u = raw
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| raw.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(raw)
        .trim();
    (!u.is_empty()).then(|| u.to_string())
}

pub fn parse_frontmatter_name(skill_md: &str) -> Option<String> {
    let (first, rest) = skill_md
        .trim_start()
        .strip_prefix("---")?
        .split_once('\n')?;
    if !first.trim().is_empty() {
        return None;
    }
    let mut found = None;
    for line in rest.lines() {
        if line.trim() == "---" {
            return found;
        }
        if found.is_none() {
            if let Some(val) = line.strip_prefix("name:") {
                let stripped = val
                    .split_once(" #")
                    .or_else(|| val.split_once("\t#"))
                    .map_or(val, |(b, _)| b);
                found = frontmatter_name_value(stripped);
            }
        }
    }
    None
}

pub fn validate_package_frontmatter(
    p: &PlacementRecord,
    skill_md: &str,
) -> Result<(), ManifestError> {
    let fm = parse_frontmatter_name(skill_md)
        .ok_or_else(|| ManifestError::MissingFrontmatterName(p.path.clone()))?;
    if fm != p.name {
        return Err(ManifestError::FrontmatterNameMismatch {
            fm,
            placement: p.name.clone(),
        });
    }
    Ok(())
}

impl Manifest {
    pub fn from_toml(content: &str) -> Result<Self, ManifestError> {
        let manifest: Self =
            toml::from_str(content).map_err(|e| ManifestError::TomlParse(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.version != 1 {
            return Err(ManifestError::UnsupportedVersion(self.version));
        }
        if self.sources.is_empty() {
            return Err(ManifestError::EmptySources);
        }
        for (sid, dir) in &self.sources {
            if !is_valid_slug(sid) {
                return Err(ManifestError::InvalidSlug(sid.clone()));
            }
            if dir.trim().is_empty() {
                return Err(ManifestError::EmptySourceDir(sid.clone()));
            }
        }
        if self.targets.is_empty() {
            return Err(ManifestError::EmptyTargets);
        }
        let mut target_ids = HashSet::new();
        for t in &self.targets {
            let id = t.id.clone();
            if !is_valid_slug(&t.id) {
                return Err(ManifestError::InvalidSlug(id));
            }
            if !target_ids.insert(id.clone()) {
                return Err(ManifestError::DuplicateTargetId(id));
            }
            if t.tools.is_empty() {
                return Err(ManifestError::EmptyTargetTools(id));
            }
            if t.root.trim().is_empty() {
                return Err(ManifestError::EmptyTargetRoot(id));
            }
            if t.root.starts_with('~') {
                return Err(ManifestError::AbsolutePath(t.root.clone()));
            }
        }

        let mut pkgs = HashMap::new();
        let mut tgts = HashMap::new();
        for p in &self.placements {
            let name = p.name.clone();
            if !self.sources.contains_key(&p.source) {
                return Err(ManifestError::UnknownSource(p.source.clone(), name));
            }
            if p.targets.is_empty() {
                return Err(ManifestError::EmptyPlacementTargets(name));
            }
            for tid in &p.targets {
                if !target_ids.contains(tid) {
                    return Err(ManifestError::UnknownTarget(tid.clone(), name));
                }
            }
            if p.path.trim().is_empty() {
                return Err(ManifestError::EmptyPackagePath(p.path.clone()));
            }
            if is_absolute_path(&p.path) {
                return Err(ManifestError::AbsolutePath(p.path.clone()));
            }
            if has_path_traversal(&p.path) {
                return Err(ManifestError::PathTraversal(p.path.clone()));
            }
            if contains_glob(&p.path) {
                return Err(ManifestError::GlobPattern(p.path.clone()));
            }
            if !is_valid_slug(&p.name) {
                return Err(ManifestError::InvalidSlug(name.clone()));
            }

            let folder = Path::new(&p.path)
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| ManifestError::EmptyPackagePath(p.path.clone()))?;
            if folder != p.name {
                return Err(ManifestError::PlacementNameMismatch {
                    placement: name,
                    folder: folder.into(),
                });
            }
            let src = (p.source.as_str(), p.path.as_str());
            for tid in &p.targets {
                if let Some(prev) = tgts.insert((tid.as_str(), p.name.as_str()), src) {
                    if prev != src {
                        let c = TargetConflict {
                            target: tid.clone(),
                            name: p.name.clone(),
                            src1: prev.0.into(),
                            p1: prev.1.into(),
                            src2: src.0.into(),
                            p2: src.1.into(),
                        };
                        return Err(ManifestError::ConflictingTargetName(Box::new(c)));
                    }
                }
            }
            if let Some(prev) = pkgs.insert(p.name.as_str(), src) {
                if prev != src {
                    let c = PackageConflict {
                        name: p.name.clone(),
                        src1: prev.0.into(),
                        p1: prev.1.into(),
                        src2: src.0.into(),
                        p2: src.1.into(),
                    };
                    return Err(ManifestError::ConflictingPackageName(Box::new(c)));
                }
            }
        }

        if let Some(ref e) = self.ephemeral {
            if e.max_tokens == 0 || e.max_tokens > 1_000_000 || e.ttl_seconds == 0 {
                let msg = if e.ttl_seconds == 0 {
                    "ttl_seconds must be > 0"
                } else {
                    "max_tokens 1..=1000000"
                };
                return Err(ManifestError::InvalidEphemeral(msg.into()));
            }
        }
        if let Some(ref t) = self.telemetry {
            if t.retention_days == 0 {
                return Err(ManifestError::InvalidTelemetry(
                    "retention_days must be > 0".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accepts_minimal_valid_manifest() {
        let s = "version = 1\n[sources]\ns = \"s\"\n[[targets]]\nid = \"t\"\ntools = [\"codex\"]\nroot = \"$R\"\nmode = \"symlink\"\n[[placements]]\nsource = \"s\"\npath = \"p\"\nname = \"p\"\ntargets = [\"t\"]\n";
        assert_eq!((Manifest::from_toml(s).unwrap().version, 1), (1, 1));
        assert!(Manifest::from_toml(&format!(
            "{s}[[placements]]\nsource = \"s\"\npath = \"p\"\nname = \"p\"\ntargets = [\"t\"]\n"
        ))
        .is_ok());
    }

    #[test]
    fn test_accepts_full_valid_manifest() {
        let s = concat!(
            "version = 1\n[sources]\ns1 = \"$S\"\ns2 = \"$P\"\n",
            "[[targets]]\nid = \"t1\"\ntools = [\"codex\", \"opencode\", \"gemini\", \"agy\"]\nroot = \"$R\"\nmode = \"symlink\"\n",
            "[[targets]]\nid = \"t2\"\ntools = [\"claude\"]\nroot = \"$R\"\nmode = \"symlink\"\n",
            "[[placements]]\nsource = \"s2\"\npath = \"p\"\nname = \"p\"\ntargets = [\"t1\", \"t2\"]\n",
            "[ephemeral]\nenabled = true\nmax_tokens = 4000\nttl_seconds = 86400\n[telemetry]\nenabled = true\nretention_days = 30\n"
        );
        let m = Manifest::from_toml(s).unwrap();
        assert!(m.version == 1 && m.ephemeral.unwrap().enabled && m.telemetry.unwrap().enabled);
    }

    #[test]
    fn test_frontmatter_matching() {
        let p = PlacementRecord {
            source: "s".into(),
            path: "r".into(),
            name: "r".into(),
            targets: vec!["t".into()],
        };
        for md in [
            "---\nname: r\n---\n",
            "---\nname: \"r\" # c\n---\n",
            "---\nname: 'r' # c\n---\n",
            "---\nn:\n  name: x\nname: r\n---\n",
        ] {
            assert!(validate_package_frontmatter(&p, md).is_ok(), "md: {md}");
        }
        for md in [
            "---\nname: w\n---\n",
            "---\nn:\n  name: r\n---\n",
            "---\nname: \"\"\n---\n",
            "# no fm",
        ] {
            assert!(validate_package_frontmatter(&p, md).is_err(), "md: {md}");
        }
    }

    #[test]
    fn test_manifest_rejections() {
        let b = "version = 1\n[sources]\ns = \"s\"\n[[targets]]\nid = \"t\"\ntools = [\"codex\"]\nroot = \"r\"\nmode = \"symlink\"\n";
        type Case = (&'static str, String, fn(&ManifestError) -> bool);
        let cases: &[Case] = &[
            ("unknown version", "version = 2\n[sources]\ns = \"s\"\n".into(), |e| matches!(e, ManifestError::UnsupportedVersion(2))),
            ("unknown fields", format!("{b}bad = true\n"), |e| matches!(e, ManifestError::TomlParse(_))),
            ("empty sources", "version = 1\n[sources]\n".into(), |e| matches!(e, ManifestError::EmptySources)),
            ("invalid source slug", "version = 1\n[sources]\n\"Bad!\" = \"s\"\n".into(), |e| matches!(e, ManifestError::InvalidSlug(_))),
            ("empty source dir", "version = 1\n[sources]\ns = \" \"\n".into(), |e| matches!(e, ManifestError::EmptySourceDir(_))),
            ("missing targets", "version = 1\n[sources]\ns = \"s\"\n".into(), |e| matches!(e, ManifestError::EmptyTargets)),
            ("empty targets list", "version = 1\ntargets = []\n[sources]\ns = \"s\"\n".into(), |e| matches!(e, ManifestError::EmptyTargets)),
            ("invalid target slug", "version = 1\n[sources]\ns = \"s\"\n[[targets]]\nid = \"Bad!\"\ntools = [\"codex\"]\nroot = \"r\"\nmode = \"symlink\"\n".into(), |e| matches!(e, ManifestError::InvalidSlug(_))),
            ("duplicate target id", format!("{b}[[targets]]\nid = \"t\"\ntools = [\"codex\"]\nroot = \"r\"\nmode = \"symlink\"\n"), |e| matches!(e, ManifestError::DuplicateTargetId(id) if id == "t")),
            ("empty target tools", "version = 1\n[sources]\ns = \"s\"\n[[targets]]\nid = \"t\"\ntools = []\nroot = \"r\"\nmode = \"symlink\"\n".into(), |e| matches!(e, ManifestError::EmptyTargetTools(_))),
            ("empty target root", "version = 1\n[sources]\ns = \"s\"\n[[targets]]\nid = \"t\"\ntools = [\"codex\"]\nroot = \" \"\nmode = \"symlink\"\n".into(), |e| matches!(e, ManifestError::EmptyTargetRoot(_))),
            ("target root tilde", "version = 1\n[sources]\ns = \"s\"\n[[targets]]\nid = \"t\"\ntools = [\"codex\"]\nroot = \"~/r\"\nmode = \"symlink\"\n".into(), |e| matches!(e, ManifestError::AbsolutePath(p) if p == "~/r")),
            ("placement path tilde", format!("{b}[[placements]]\nsource = \"s\"\npath = \"~/p\"\nname = \"p\"\ntargets = [\"t\"]\n"), |e| matches!(e, ManifestError::AbsolutePath(_))),
            ("placement path absolute", format!("{b}[[placements]]\nsource = \"s\"\npath = \"/p\"\nname = \"p\"\ntargets = [\"t\"]\n"), |e| matches!(e, ManifestError::AbsolutePath(_))),
            ("placement path traversal", format!("{b}[[placements]]\nsource = \"s\"\npath = \"../p\"\nname = \"p\"\ntargets = [\"t\"]\n"), |e| matches!(e, ManifestError::PathTraversal(_))),
            ("placement path glob", format!("{b}[[placements]]\nsource = \"s\"\npath = \"p/*\"\nname = \"p\"\ntargets = [\"t\"]\n"), |e| matches!(e, ManifestError::GlobPattern(_))),
            ("unknown source", format!("{b}[[placements]]\nsource = \"unknown\"\npath = \"p\"\nname = \"p\"\ntargets = [\"t\"]\n"), |e| matches!(e, ManifestError::UnknownSource(_, _))),
            ("unknown target", format!("{b}[[placements]]\nsource = \"s\"\npath = \"p\"\nname = \"p\"\ntargets = [\"unknown\"]\n"), |e| matches!(e, ManifestError::UnknownTarget(_, _))),
            ("empty placement targets", format!("{b}[[placements]]\nsource = \"s\"\npath = \"p\"\nname = \"p\"\ntargets = []\n"), |e| matches!(e, ManifestError::EmptyPlacementTargets(_))),
            ("folder mismatch", format!("{b}[[placements]]\nsource = \"s\"\npath = \"p\"\nname = \"other\"\ntargets = [\"t\"]\n"), |e| matches!(e, ManifestError::PlacementNameMismatch { .. })),
            ("conflicting target name", format!("{b}[[placements]]\nsource = \"s\"\npath = \"p\"\nname = \"p\"\ntargets = [\"t\"]\n[[placements]]\nsource = \"s\"\npath = \"sub/p\"\nname = \"p\"\ntargets = [\"t\"]\n"), |e| matches!(e, ManifestError::ConflictingTargetName(_))),
            ("conflicting package name", "version = 1\n[sources]\ns = \"s\"\n[[targets]]\nid = \"t1\"\ntools = [\"codex\"]\nroot = \"r\"\nmode = \"symlink\"\n[[targets]]\nid = \"t2\"\ntools = [\"codex\"]\nroot = \"r\"\nmode = \"symlink\"\n[[placements]]\nsource = \"s\"\npath = \"p\"\nname = \"p\"\ntargets = [\"t1\"]\n[[placements]]\nsource = \"s\"\npath = \"sub/p\"\nname = \"p\"\ntargets = [\"t2\"]\n".into(), |e| matches!(e, ManifestError::ConflictingPackageName(_))),
            ("invalid ephemeral tokens", format!("{b}[ephemeral]\nmax_tokens = 0\n"), |e| matches!(e, ManifestError::InvalidEphemeral(_))),
            ("invalid ephemeral ttl", format!("{b}[ephemeral]\nttl_seconds = 0\n"), |e| matches!(e, ManifestError::InvalidEphemeral(_))),
            ("invalid telemetry retention", format!("{b}[telemetry]\nretention_days = 0\n"), |e| matches!(e, ManifestError::InvalidTelemetry(_))),
        ];
        for (label, toml_str, matcher) in cases {
            match Manifest::from_toml(toml_str) {
                Err(ref e) if matcher(e) => {}
                other => panic!("case '{label}' failed: expected matching error, got {other:?}"),
            }
        }
    }
}
