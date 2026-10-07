//! Ingestion guard: path denylist and content redaction for ingested material.
//!
//! Every route that reads a local file (or accepts caller content) and hands it
//! to the memory/indexing layer must screen it first. Detected secret values are
//! replaced by a fixed marker and are never logged or persisted in clear text.

use std::path::Path;

use crate::security::scanner::entropy::SecretDetector;

/// Prefix of the fixed marker substituted for a detected secret.
pub const REDACTION_MARKER_PREFIX: &str = "«redacted:";

/// Marker used when the detected pattern has no name.
pub const REDACTION_MARKER: &str = "«redacted»";

/// File names that are always denied, matched case-insensitively.
const DENIED_FILE_NAMES: &[&str] = &[".env", ".netrc", ".npmrc", ".pypirc"];

/// File-name suffixes that are always denied.
const DENIED_SUFFIXES: &[&str] = &[
    ".env",
    ".key",
    ".pem",
    ".p12",
    ".pfx",
    ".credentials",
    ".kdbx",
];

/// File-name prefixes that are always denied.
const DENIED_PREFIXES: &[&str] = &["id_rsa", "id_ed25519"];

/// Verdict for one ingestion candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestDecision {
    /// The path matches the secret denylist; nothing is read.
    RejectSecretPath,
    /// The payload is not valid UTF-8 text and cannot be redacted.
    RejectNonUtf8,
    /// Safe to ingest; `content` is the redacted text.
    Allow { content: String, redacted: usize },
}

impl IngestDecision {
    /// True when the candidate may be ingested.
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }

    /// Redacted content, or `None` when the candidate was rejected.
    pub fn content(&self) -> Option<&str> {
        match self {
            Self::Allow { content, .. } => Some(content),
            _ => None,
        }
    }

    /// Number of redactions applied, or `None` when the candidate was rejected.
    pub fn redacted(&self) -> Option<usize> {
        match self {
            Self::Allow { redacted, .. } => Some(*redacted),
            _ => None,
        }
    }
}

/// True when `path` names a credential or key store that must never be ingested.
///
/// Matching is by file name only (case-insensitive): `.env`, `*.env`, `.env.*`,
/// `*.key`, `*.pem`, `*.p12`, `*.pfx`, `id_rsa*`, `id_ed25519*`, `*_rsa`,
/// `*_ed25519`, `credentials*.json`, `*.credentials`, `.netrc`, `.npmrc`,
/// `.pypirc`, `secrets.{yaml,yml,json,toml}`, `*.kdbx`.
pub fn is_secret_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();

    if DENIED_FILE_NAMES.contains(&name.as_str()) {
        return true;
    }
    if DENIED_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
        return true;
    }
    if DENIED_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        return true;
    }
    if name.starts_with(".env") {
        return true;
    }
    if name.starts_with("credentials") && name.ends_with(".json") {
        return true;
    }
    if name.ends_with("_rsa") || name.ends_with("_ed25519") {
        return true;
    }
    matches!(
        name.as_str(),
        "secrets.yaml" | "secrets.yml" | "secrets.json" | "secrets.toml"
    )
}

/// Replace every value detected by [`SecretDetector::extract_secrets`] with a
/// fixed marker. Returns the redacted text and the number of replacements.
pub fn redact_secrets(text: &str) -> (String, usize) {
    let mut matches = SecretDetector::extract_secrets(text);
    if matches.is_empty() {
        return (text.to_string(), 0);
    }
    // Regex matches arrive grouped per pattern; order them and drop overlaps so
    // offsets stay valid while building the output in one pass.
    matches.sort_by_key(|m| (m.start, m.end));
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    let mut redacted = 0usize;
    for m in matches {
        if m.start < cursor || m.start >= m.end || m.end > text.len() {
            continue;
        }
        out.push_str(&text[cursor..m.start]);
        out.push_str(REDACTION_MARKER_PREFIX);
        out.push_str(&m.pattern_name);
        out.push('»');
        cursor = m.end;
        redacted += 1;
    }
    out.push_str(&text[cursor..]);
    (out, redacted)
}

/// Screen a path plus already-decoded UTF-8 text.
pub fn screen_ingest(path: &Path, content: &str) -> IngestDecision {
    if is_secret_path(path) {
        return IngestDecision::RejectSecretPath;
    }
    let (content, redacted) = redact_secrets(content);
    IngestDecision::Allow { content, redacted }
}

/// Screen a path plus raw bytes, rejecting non-UTF-8 payloads.
pub fn screen_ingest_bytes(path: &Path, bytes: &[u8]) -> IngestDecision {
    if is_secret_path(path) {
        return IngestDecision::RejectSecretPath;
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => {
            let (content, redacted) = redact_secrets(text);
            IngestDecision::Allow { content, redacted }
        }
        Err(_) => IngestDecision::RejectNonUtf8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fake values only: shaped to match the existing detector patterns.
    const FAKE_API: &str = "FAKEkey0123456789ABCDEFGH";

    #[test]
    fn guard_detects_all_fixture_names() {
        for denied in [
            ".env",
            ".env.local",
            "record.key",
            "master.key",
            "id_rsa",
            "server.pem",
            "credentials.json",
        ] {
            assert!(is_secret_path(Path::new(denied)), "{denied} must be denied");
        }
        for allowed in ["leaky.rs", "NOTES.md", "in.md", "notes.txt"] {
            assert!(
                !is_secret_path(Path::new(allowed)),
                "{allowed} must be allowed"
            );
        }
    }

    #[test]
    fn redact_secrets_leaves_no_fake_value() {
        let text = format!("API_KEY={FAKE_API}\nnote\n");
        let (redacted, count) = redact_secrets(&text);
        assert_eq!(count, 1, "{redacted}");
        assert!(!redacted.contains(FAKE_API), "{redacted}");
        assert!(redacted.contains(REDACTION_MARKER_PREFIX), "{redacted}");
    }

    #[test]
    fn screen_ingest_rejects_denied_path_and_non_utf8() {
        assert_eq!(
            screen_ingest(Path::new(".env"), "x"),
            IngestDecision::RejectSecretPath
        );
        assert_eq!(
            screen_ingest_bytes(Path::new("notes.md"), &[0xff, 0xfe]),
            IngestDecision::RejectNonUtf8
        );
        let ok = screen_ingest(Path::new("notes.md"), &format!("API_KEY={FAKE_API}"));
        assert!(ok.is_allowed());
        assert_eq!(ok.redacted(), Some(1));
        assert!(!ok.content().unwrap().contains(FAKE_API));
    }
}
