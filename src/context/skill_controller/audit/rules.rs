//! Rule IDs, severities and the pattern table the audit compiles once per run.

use regex::{Regex, RegexSet};

use super::super::audit::AuditError;
use super::super::policy::{PolicyError, SECRET_CLASSES};

/// Audit bounds: `(max packages, max findings per package, max endpoints noted
/// per package, max bytes of one rendered proposal)`.
pub const AUDIT_BOUNDS: (usize, usize, usize, usize) = (512, 64, 16, 8 << 10);
const _: () =
    assert!(AUDIT_BOUNDS.0 > 0 && AUDIT_BOUNDS.1 > 0 && AUDIT_BOUNDS.2 > 0 && AUDIT_BOUNDS.3 > 0);

/// The severity a rule always reports; it orders a report without a tie-breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// One stable rule ID per deterministic check, with the severity it always
/// carries (D10/D13: the audit reports and proposes, it never auto-fixes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rule {
    pub id: &'static str,
    pub severity: Severity,
}
impl Rule {
    const fn warn(id: &'static str) -> Self {
        Self {
            id,
            severity: Severity::Warning,
        }
    }
    const fn err(id: &'static str) -> Self {
        Self {
            id,
            severity: Severity::Error,
        }
    }
    const fn info(id: &'static str) -> Self {
        Self {
            id,
            severity: Severity::Info,
        }
    }
}

pub const DESC_MISSING: Rule = Rule::warn("skill.metadata.description_missing");
pub const DESC_TOO_LONG: Rule = Rule::warn("skill.metadata.description_too_long");
pub const REF_BROKEN: Rule = Rule::warn("skill.local_reference.broken");
pub const REF_ESCAPES: Rule = Rule::warn("skill.local_reference.escapes_package");
pub const NAME_MISMATCH: Rule = Rule::err("skill.metadata.name_mismatch");
pub const SKILL_MD_MISSING: Rule = Rule::err("skill.metadata.skill_md_missing");
pub const DECL_CONFLICT: Rule = Rule::warn("skill.declared.conflict");
pub const DECL_VERSION: Rule = Rule::warn("skill.declared.version_conflict");
pub const SECRET: Rule = Rule::err("skill.secret.found");
pub const ENDPOINT: Rule = Rule::info("skill.endpoint.unverified");
pub const HASH_DRIFT: Rule = Rule::warn("skill.projection.hash_drift");
pub const PKG_MISSING: Rule = Rule::warn("skill.package.unavailable");
pub const PKG_UNSCANNABLE: Rule = Rule::warn("skill.package.unscannable");
pub const PROPOSAL_SUPPRESSED: Rule = Rule::warn("proposal.suppressed");

/// Secret shapes the audit refuses to render even though J02's inherited table
/// does not classify them: PKCS#8 and OpenSSH key headers, and `KEY=VALUE`
/// environment-style secrets (`*_SECRET*`, `*_TOKEN*`, `*_KEY*`, `PASSWORD*`).
const EXTRA_SECRET_PATTERNS: &[&str] = &[
    r"BEGIN (OPENSSH )?PRIVATE KEY",
    r"(?:[A-Za-z0-9_]*_(?:SECRET|TOKEN|KEY)|PASSWORD)[A-Za-z0-9_]*=\S",
];

/// Patterns compiled once per run, never inside a loop, so a broken pattern is an
/// audit error instead of a per-line `expect`.
pub(super) struct Patterns {
    pub link: Regex,
    pub url: Regex,
    /// The policy scanner's table (D10), one regex per class, so a finding can
    /// re-locate a match without keeping the matched bytes.
    pub secrets: Vec<(&'static str, Regex)>,
    /// Everything a rendered proposal may never contain.
    sensitive: RegexSet,
}

impl Patterns {
    pub fn compile() -> Result<Self, AuditError> {
        let secrets = SECRET_CLASSES
            .iter()
            .map(|(class, source)| compile(source).map(|regex| (*class, regex)))
            .collect::<Result<Vec<_>, AuditError>>()?;
        let mut sensitive: Vec<&str> = SECRET_CLASSES.iter().map(|(_, p)| *p).collect();
        sensitive.extend(EXTRA_SECRET_PATTERNS.iter().copied());
        sensitive.push(r"https?://\S");
        let sensitive = RegexSet::new(sensitive).map_err(|e| AuditError::InvalidPatterns {
            detail: e.to_string(),
        })?;
        Ok(Self {
            link: compile(r"\]\(([^)\s]+)")?,
            url: compile(r#"https?://[^\s\)\]>"']+"#)?,
            secrets,
            sensitive,
        })
    }

    /// True when `text` matches a secret class, an extra secret shape or an http
    /// literal, so the caller must not surface it.
    pub fn sensitive(&self, text: &str) -> bool {
        self.sensitive.is_match(text)
    }
}

fn compile(source: &str) -> Result<Regex, AuditError> {
    Regex::new(source).map_err(|e| AuditError::InvalidPatterns {
        detail: e.to_string(),
    })
}

pub use super::super::policy::RootKind;

/// True when a policy error means the declared path left the allowlist, which the
/// audit refuses instead of reporting.
pub fn is_escape(error: &PolicyError) -> bool {
    matches!(
        error,
        PolicyError::OutsideAllowlist { .. }
            | PolicyError::Traversal { .. }
            | PolicyError::SymlinkComponent { .. }
    )
}
