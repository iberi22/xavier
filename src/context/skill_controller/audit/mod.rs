//! D01/D13: read-only drift audit of skill packages and their projections.
//!
//! Reuses the projector's package hash (`super::plan::hash_package`) and J02's
//! scanner (`super::policy::scan_package`), and renders a reviewable proposal as
//! a hand-built unified diff. The audit never writes and never probes the
//! network: it opens members read-only through one shared policy-checked reader
//! and reports a rule ID, a package-relative `file:line` and a detail built
//! from counts or digests instead of untrusted text (D10/D13). A secret finding
//! carries class and location only; a proposal carries its own added line and
//! nothing the member already held. Callers on a Tokio worker must wrap these
//! calls in `tokio::task::spawn_blocking`.

pub mod member;
pub mod render;
pub mod rules;
#[cfg(test)]
mod tests;

use super::manifest::{parse_frontmatter_name, PlacementRecord};
use super::plan::{hash_package, ResolvedRoots};
use super::policy::{scan_package, AuthorizedDir, FsPolicy, PolicyError, RootKind, SecretFinding};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use thiserror::Error;

pub use member::{frontmatter_field, frontmatter_start, read_member_text, DESCRIPTION_MAX};
pub use render::{proposal_diff, redact_url};
pub use rules::{Rule, Severity, AUDIT_BOUNDS};

use member::{http_hits, link_targets};
use rules::Patterns;

/// One redacted finding. `location` is a package-relative `file:line`; a finding
/// about the whole package uses `.`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule: Rule,
    pub severity: Severity,
    pub package: String,
    pub location: String,
    pub detail: String,
}

/// A reviewable unified diff for one finding, built from structured data instead
/// of the member's own bytes. Never applied by the audit; it never covers a
/// secret, safety or approval rule (D13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub rule: Rule,
    pub package: String,
    /// Package-relative path the diff applies to.
    pub path: String,
    pub diff: String,
}

/// Everything one audit run reports; an empty finding list is a clean audit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditReport {
    pub findings: Vec<Finding>,
    pub proposals: Vec<Proposal>,
    pub packages_audited: usize,
    pub bytes_scanned: u64,
    /// A bound stopped the audit early, so the result is partial (D13).
    pub truncated: bool,
}

/// An escape out of the owner-supplied allowlist is refused, never reported as a
/// finding: the caller must not continue against an unauthorised path.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuditError {
    #[error("no source root resolves '{0}' in the host policy")]
    UnresolvedSource(String),
    #[error("a declared package path escapes the authorized source root")]
    OutsideAllowlist,
    #[error("audit patterns are invalid: {detail}")]
    InvalidPatterns { detail: String },
    #[error("the audit exceeded the {bound} bound")]
    BoundExceeded { bound: &'static str },
}

/// One declared package: its placement plus the package hash recorded by the last
/// verified projection (`None` = never verified).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditedPackage {
    pub placement: PlacementRecord,
    pub recorded_hash: Option<String>,
}

/// Audit declared packages against their authorized sources: metadata, local
/// references, offline endpoint evidence, secrets, projection hash drift and
/// declared conflicts across the whole set. Read-only by construction.
pub fn audit_packages(
    policy: &FsPolicy,
    roots: &ResolvedRoots,
    packages: &[AuditedPackage],
) -> Result<AuditReport, AuditError> {
    if packages.len() > AUDIT_BOUNDS.0 {
        return Err(AuditError::BoundExceeded {
            bound: "package count",
        });
    }
    for audited in packages {
        if !roots.sources.contains_key(&audited.placement.source) {
            return Err(AuditError::UnresolvedSource(
                audited.placement.source.clone(),
            ));
        }
    }
    let mut run = AuditRun {
        policy,
        roots,
        patterns: Patterns::compile()?,
        report: AuditReport::default(),
        versions: BTreeMap::new(),
        in_package: 0,
        package: String::new(),
        secret_found: false,
    };
    for audited in packages {
        run.audit_package(audited)?;
        run.report.packages_audited += 1;
    }
    declared_conflicts(packages, &run.versions, &mut run.report.findings);
    run.report.findings.sort_by(|a, b| {
        a.package
            .cmp(&b.package)
            .then(a.rule.cmp(&b.rule))
            .then(a.location.cmp(&b.location))
    });
    Ok(run.report)
}

struct AuditRun<'a> {
    policy: &'a FsPolicy,
    roots: &'a ResolvedRoots,
    patterns: Patterns,
    report: AuditReport,
    /// Declared `version` per package name, for the declared-conflict rule.
    versions: BTreeMap<String, BTreeSet<String>>,
    in_package: usize,
    package: String,
    /// The current package already produced a secret finding, so it earns no
    /// proposal at all (D13).
    secret_found: bool,
}

impl AuditRun<'_> {
    /// Append one finding, capped per package so a rotting skill cannot flood the
    /// report with unbounded lines.
    fn add(&mut self, rule: Rule, location: String, detail: &str) {
        self.secret_found |= rule == rules::SECRET;
        if self.in_package >= AUDIT_BOUNDS.1 {
            self.report.truncated = true;
            return;
        }
        self.in_package += 1;
        self.report.findings.push(Finding {
            rule,
            severity: rule.severity,
            package: self.package.clone(),
            location,
            detail: detail.into(),
        });
    }

    /// A finding about the whole package instead of one member.
    fn note(&mut self, rule: Rule, detail: &str) {
        self.add(rule, ".".into(), detail);
    }

    fn audit_package(&mut self, audited: &AuditedPackage) -> Result<(), AuditError> {
        self.package.clone_from(&audited.placement.name);
        self.in_package = 0;
        self.secret_found = false;
        let root = self
            .roots
            .sources
            .get(&audited.placement.source)
            .ok_or_else(|| AuditError::UnresolvedSource(audited.placement.source.clone()))?;
        let dir = match self
            .policy
            .authorize(RootKind::Source, &root.join(&audited.placement.path))
        {
            Ok(dir) => dir,
            Err(escape) if rules::is_escape(&escape) => return Err(AuditError::OutsideAllowlist),
            Err(_) => {
                self.note(rules::PKG_MISSING, "declared package missing");
                return Ok(());
            }
        };
        // Secret findings from J02's scanner: class and relative location only.
        // An unscannable package is also an unreadable one, so the audit stops
        // here instead of opening members the scanner already refused.
        match scan_package(&dir) {
            Ok(scan) => {
                self.report.bytes_scanned += scan.bytes_scanned;
                for finding in &scan.findings {
                    let line = self.secret_line(&dir, finding);
                    let location = format!("{}:{}", finding.location, line);
                    let detail = format!("the {} pattern matched", finding.class);
                    self.add(rules::SECRET, location, &detail);
                }
            }
            Err(_) => {
                self.note(rules::PKG_UNSCANNABLE, "not scannable by the policy");
                return Ok(());
            }
        }
        let Some(text) = read_member_text(&dir, "SKILL.md") else {
            self.add(
                rules::SKILL_MD_MISSING,
                "SKILL.md:1".into(),
                "SKILL.md unreadable",
            );
            return Ok(());
        };
        // Metadata: the Agent Skills spec frontmatter rules (04 §4).
        let start = frontmatter_start(&text);
        let name = frontmatter_field(&text, "name");
        let location = format!("SKILL.md:{}", name.as_ref().map_or(start, |(l, _)| *l));
        if let Some(found) = parse_frontmatter_name(&text) {
            if found != audited.placement.name {
                self.add(
                    rules::NAME_MISMATCH,
                    location,
                    "name differs from the placement",
                );
            }
        }
        let description = frontmatter_field(&text, "description");
        let location = format!(
            "SKILL.md:{}",
            description.as_ref().map_or(start, |(l, _)| *l)
        );
        match &description {
            None => self.add(rules::DESC_MISSING, location, "no description field"),
            Some((_, value)) if value.chars().count() > DESCRIPTION_MAX => {
                let detail = format!("{} chars, limit {DESCRIPTION_MAX}", value.chars().count());
                self.add(rules::DESC_TOO_LONG, location, &detail);
            }
            Some(_) => {}
        }
        if let Some((_, version)) = frontmatter_field(&text, "version") {
            self.versions
                .entry(self.package.clone())
                .or_default()
                .insert(version);
        }
        // Local references: every inline markdown link to a package-relative member.
        for (line, target) in link_targets(&text, &self.patterns.link) {
            let location = format!("SKILL.md:{line}");
            if target.split(['/', '\\']).any(|part| part == "..") {
                self.add(rules::REF_ESCAPES, location, "link escapes the package");
                continue;
            }
            let present = member::member_path(dir.path(), &target)
                .and_then(|path| fs::symlink_metadata(path).ok())
                .is_some();
            if !present {
                let detail = format!("link {} missing", render::target_digest(&target));
                self.add(rules::REF_BROKEN, location, &detail);
            }
        }
        // Offline endpoint evidence: noted as unverified, never probed (D13).
        let (endpoints, capped) = http_hits(&text, &self.patterns.url);
        self.report.truncated |= capped;
        for (line, literal) in endpoints {
            let detail = format!("not probed: {}", redact_url(&literal));
            self.add(rules::ENDPOINT, format!("SKILL.md:{line}"), &detail);
        }
        // Projection drift: the recorded hash no longer describes the package.
        let observed = hash_package(&dir, &audited.placement).map(|identity| identity.hash);
        match (&observed, &audited.recorded_hash) {
            (Ok(hash), Some(recorded)) if recorded != hash => {
                self.note(rules::HASH_DRIFT, "projection hash drift");
            }
            (Err(_), Some(_)) => self.note(rules::PKG_UNSCANNABLE, "unhashable"),
            _ => {}
        }
        if description.is_none() {
            self.propose_description(&text);
        }
        Ok(())
    }

    /// The 1-based line of the first match for the finding's class, or 0 when the
    /// member cannot be re-read.
    fn secret_line(&self, dir: &AuthorizedDir, finding: &SecretFinding) -> usize {
        let Some(text) = read_member_text(dir, &finding.location) else {
            return 0;
        };
        for (class, regex) in &self.patterns.secrets {
            if *class == finding.class {
                if let Some(hit) = regex.find(&text) {
                    return 1 + text.as_bytes()[..hit.start()]
                        .iter()
                        .filter(|byte| **byte == b'\n')
                        .count();
                }
            }
        }
        0
    }

    /// Render the only safe deterministic repair (D13): a placeholder
    /// `description` for a package whose frontmatter lacks one. Nothing on disk is
    /// rewritten and no member byte is copied: the diff is assembled from the
    /// insertion point plus the one line the audit itself composes.
    fn propose_description(&mut self, text: &str) {
        if self.secret_found {
            return;
        }
        let (at, added) = render::description_insertion(text, &self.package);
        let diff = proposal_diff(&added, at);
        if diff.len() > AUDIT_BOUNDS.3 {
            self.report.truncated = true;
            return;
        }
        if self.patterns.sensitive(&diff) {
            self.note(
                rules::PROPOSAL_SUPPRESSED,
                "proposal withheld: it matched a sensitive pattern",
            );
            return;
        }
        self.report.proposals.push(Proposal {
            rule: rules::DESC_MISSING,
            package: self.package.clone(),
            path: "SKILL.md".into(),
            diff,
        });
    }
}

/// Duplicate names across the audited union of placements (01 §4): one manifest
/// already rejects them, so the audit reports them for the whole union instead.
fn declared_conflicts(
    packages: &[AuditedPackage],
    versions: &BTreeMap<String, BTreeSet<String>>,
    findings: &mut Vec<Finding>,
) {
    let mut grouped: BTreeMap<&str, BTreeSet<(&str, &str)>> = BTreeMap::new();
    for audited in packages {
        let placement = &audited.placement;
        grouped
            .entry(placement.name.as_str())
            .or_default()
            .insert((placement.source.as_str(), placement.path.as_str()));
    }
    for (name, origins) in grouped {
        if origins.len() > 1 {
            let detail = format!("{} placements declare this name", origins.len());
            findings.push(finding(rules::DECL_CONFLICT, name, detail));
        }
        if let Some(v) = versions.get(name).filter(|v| v.len() > 1) {
            let detail = format!("{} declared versions for this name", v.len());
            findings.push(finding(rules::DECL_VERSION, name, detail));
        }
    }
}

/// A whole-package finding: same shape for every declared-data rule.
fn finding(rule: Rule, package: &str, detail: String) -> Finding {
    Finding {
        rule,
        severity: rule.severity,
        package: package.into(),
        location: ".".into(),
        detail,
    }
}
