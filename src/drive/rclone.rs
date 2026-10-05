//! Detect and reuse an existing rclone remote before doing any OAuth.
//!
//! # The rule: do not disturb what already works
//!
//! `gdrive:` and `gdrive-xavier-crypt:` are configured and validated, and two
//! timers depend on them (`xavier-backup.timer` at 03:17,
//! `xavier-backup-verify.timer` at 04:40). The native Drive connection is a
//! **fallback for a machine that has no remote**, not a replacement.
//!
//! [`detect_rclone_remote`] reads `rclone.conf` directly — no subprocess, no
//! `rclone config dump` — so asking the question has no side effects and cannot
//! block the backup path. The native OAuth flow is entered only when the answer
//! is "no usable remote".
//!
//! # What is *not* done here
//!
//! This module never writes `rclone.conf`. Rewriting a config that two working
//! timers depend on, in order to store a credential that can live in Xavier's own
//! sealed store, trades a known-good state for no security gain.

use std::path::{Path, PathBuf};

/// How a remote was configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// Plain Google Drive (no client-side encryption).
    PlainDrive,
    /// Google Drive wrapped by rclone's `crypt` layer.
    CryptDrive,
    /// Anything else (S3, WebDAV, …).
    Other,
}

/// A usable rclone remote found on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RcloneRemote {
    /// Remote name without the trailing colon, e.g. `gdrive-xavier-crypt`.
    pub name: String,
    pub kind: TransportKind,
    /// Whether a `crypt` remote wraps it.
    pub wrapped: bool,
}

impl RcloneRemote {
    /// rclone-style remote string, e.g. `gdrive-xavier-crypt:`.
    pub fn remote_string(&self) -> String {
        format!("{}:", self.name)
    }

    /// Whether this remote is suitable for an encrypted backup.
    ///
    /// A plain remote does not protect the data at rest on Google's side, which
    /// is exactly why the historical `master.key` files ended up readable there.
    pub fn suitable_for_encrypted_backup(&self) -> bool {
        matches!(self.kind, TransportKind::CryptDrive)
    }
}

/// Path of the rclone config, from `$RCLONE_CONFIG` else the default.
pub fn rclone_config_path() -> PathBuf {
    std::env::var("RCLONE_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
                .join("rclone")
                .join("rclone.conf")
        })
}

/// Remote names in preference order: crypt-wrapped first, because an encrypted
/// remote is the only one appropriate for a memory vault.
pub const PREFERRED_REMOTE_ORDER: &[&str] = &["gdrive-xavier-crypt", "gdrive"];

/// Look for a usable remote in `rclone.conf`.
///
/// Pure read: parses `[name]` / `type = ...` pairs without executing rclone.
/// Returns `None` when no remote is present or none matches a preference, which
/// is the signal to run the native OAuth flow instead.
pub fn detect_rclone_remote() -> Option<RcloneRemote> {
    detect_rclone_remote_at(&rclone_config_path())
}

/// [`detect_rclone_remote`] against an explicit config path (tests use a tempdir).
pub fn detect_rclone_remote_at(config: &Path) -> Option<RcloneRemote> {
    let content = std::fs::read_to_string(config).ok()?;
    let remotes = parse_remotes(&content);

    for preferred in PREFERRED_REMOTE_ORDER {
        if let Some(found) = remotes.iter().find(|r| r.name == *preferred) {
            return Some(found.clone());
        }
    }
    // Nothing preferred exists: any Drive remote beats starting an OAuth flow,
    // but it is returned so the caller can warn that it is not encrypted.
    remotes
        .iter()
        .find(|r| {
            matches!(
                r.kind,
                TransportKind::PlainDrive | TransportKind::CryptDrive
            )
        })
        .cloned()
        .or_else(|| remotes.first().cloned())
}

/// Parse `rclone.conf` into remotes.
///
/// Minimal INI: section headers and `key = value`. Comments (`#`, `;`) and
/// blank lines are skipped. Keys before the first section are ignored.
pub fn parse_remotes(content: &str) -> Vec<RcloneRemote> {
    let mut remotes: Vec<RcloneRemote> = Vec::new();
    let mut current: Option<(String, RemoteFields)> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if let Some((name, fields)) = current.take() {
                remotes.push(build_remote(name, fields));
            }
            current = Some((name.trim().to_string(), RemoteFields::default()));
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if let Some((_, ref mut fields)) = current {
                // `type` and `remote` are DIFFERENT fields with different
                // meanings and must be captured separately:
                //   type   = crypt          -> "this remote encrypts"
                //   remote = gdrive:...     -> "this is what it wraps"
                // Conflating them (storing `type` in the `remote` slot) makes a
                // crypt-over-Drive remote classify as `Other`, which hides the
                // one remote a vault backup actually needs.
                match key.trim() {
                    "type" => fields.kind = Some(value.trim().to_string()),
                    "remote" => fields.underlying = Some(value.trim().to_string()),
                    _ => {}
                }
            }
        }
    }
    if let Some((name, fields)) = current.take() {
        remotes.push(build_remote(name, fields));
    }
    remotes
}

/// The two `rclone.conf` keys that matter for classification.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct RemoteFields {
    /// Value of `type =` (`drive`, `crypt`, `s3`, …).
    kind: Option<String>,
    /// Value of `remote =` for a `crypt` remote: the remote it wraps.
    underlying: Option<String>,
}

/// Build a remote from its `type` and, for `crypt`, its underlying `remote`.
///
/// A crypt remote is classified as [`TransportKind::CryptDrive`] when it
/// declares `type = crypt` **and** wraps a Drive remote. Both facts are
/// required: the name alone is not proof (`s3crypt` is not a Drive backup), and
/// `type = crypt` alone does not say what is being encrypted.
fn build_remote(name: String, fields: RemoteFields) -> RcloneRemote {
    let kind = fields.kind.as_deref().unwrap_or_default();
    let underlying = fields.underlying.as_deref().unwrap_or_default();

    if kind == "crypt" {
        let wraps_drive = underlying.contains("drive");
        return RcloneRemote {
            kind: if wraps_drive {
                TransportKind::CryptDrive
            } else {
                TransportKind::Other
            },
            name,
            wrapped: true,
        };
    }

    // Not a crypt remote: classify by declared type. A name that merely looks
    // encrypted ("gdrive-xavier-crypt") is still whatever its `type` says, so a
    // mislabelled section cannot be reported as encrypted.
    RcloneRemote {
        kind: classify(kind),
        name,
        wrapped: false,
    }
}

fn classify(value: &str) -> TransportKind {
    match value {
        "drive" => TransportKind::PlainDrive,
        _ => TransportKind::Other,
    }
}

/// Whether an OAuth flow is needed: true only when no remote is usable.
pub fn needs_oauth(config: &Path) -> bool {
    detect_rclone_remote_at(config).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_CONFIG: &str = r#"
# Xavier backups
[gdrive]
type = drive
scope = drive
token = {"access_token":"ya29.x"}

[gdrive-xavier-crypt]
type = crypt
remote = gdrive:
filename_encryption = standard
password = should-never-be-logged
"#;

    /// AC: an existing valid remote means no OAuth, so the timers stay intact.
    #[test]
    fn existing_crypt_remote_wins_and_needs_no_oauth() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("rclone.conf");
        std::fs::write(&config, REAL_CONFIG).unwrap();

        let remote = detect_rclone_remote_at(&config).unwrap();
        assert_eq!(remote.name, "gdrive-xavier-crypt");
        assert_eq!(remote.remote_string(), "gdrive-xavier-crypt:");
        assert!(
            !needs_oauth(&config),
            "an existing remote must not trigger OAuth"
        );
    }

    /// A plain remote is returned but flagged as not suitable for a vault backup.
    #[test]
    fn plain_remote_is_flagged_as_unencrypted() {
        let remotes = parse_remotes(REAL_CONFIG);
        let gdrive = remotes.iter().find(|r| r.name == "gdrive").unwrap();
        assert_eq!(gdrive.kind, TransportKind::PlainDrive);
        assert!(!gdrive.suitable_for_encrypted_backup());
    }

    /// AC: a fresh machine with no config is the only case that needs OAuth.
    #[test]
    fn missing_config_needs_oauth() {
        let dir = tempfile::tempdir().unwrap();
        let absent = dir.path().join("nope.conf");
        assert!(needs_oauth(&absent));

        std::fs::write(&absent, "").unwrap();
        assert!(needs_oauth(&absent), "an empty config has no usable remote");
    }

    #[test]
    fn parser_handles_comments_and_blank_lines() {
        let remotes =
            parse_remotes("\n# comment\n; another\n[alpha]\ntype = drive\n\n[beta]\ntype = s3\n");
        assert_eq!(remotes.len(), 2);
        assert_eq!(remotes[0].name, "alpha");
        assert_eq!(remotes[1].kind, TransportKind::Other);
    }

    /// Regression: `type` and `remote` are different fields. Conflating them made
    /// every crypt remote classify as `Other`, hiding the encrypted remote that a
    /// vault backup requires.
    #[test]
    fn crypt_remote_is_classified_as_encrypted_drive() {
        let remotes = parse_remotes(REAL_CONFIG);
        let crypt = remotes
            .iter()
            .find(|r| r.name == "gdrive-xavier-crypt")
            .unwrap();
        assert_eq!(crypt.kind, TransportKind::CryptDrive);
        assert!(crypt.wrapped);
        assert!(
            crypt.suitable_for_encrypted_backup(),
            "a crypt-over-drive remote is exactly what a vault backup needs"
        );
    }

    /// The production config uses a deep path as the underlying remote, so the
    /// classifier must not require it to be exactly `gdrive:`.
    #[test]
    fn crypt_remote_with_a_deep_underlying_path_is_recognised() {
        let remotes = parse_remotes(
            "[gdrive-xavier-crypt]\ntype = crypt\nremote = gdrive:SWAL/backups/xavier-crypt\n",
        );
        assert_eq!(remotes[0].kind, TransportKind::CryptDrive);
        assert!(remotes[0].suitable_for_encrypted_backup());
    }

    /// A remote merely *named* like a crypt is still whatever its type declares.
    #[test]
    fn a_drive_remote_named_like_crypt_is_not_reported_as_encrypted() {
        let remotes = parse_remotes("[gdrive-xavier-crypt]\ntype = drive\n");
        assert_eq!(remotes[0].kind, TransportKind::PlainDrive);
        assert!(
            !remotes[0].suitable_for_encrypted_backup(),
            "the declared type wins over the name"
        );
    }

    /// `type = crypt` with no underlying remote cannot be called a Drive backup.
    #[test]
    fn crypt_without_an_underlying_remote_is_not_a_drive_backup() {
        let remotes = parse_remotes("[mystery-crypt]\ntype = crypt\n");
        assert_eq!(remotes[0].kind, TransportKind::Other);
        assert!(!remotes[0].suitable_for_encrypted_backup());
    }

    /// A crypt remote that does not wrap Drive must not be called a Drive backup.
    #[test]
    fn crypt_over_non_drive_is_not_a_drive_backup() {
        let remotes = parse_remotes("[s3crypt]\ntype = crypt\nremote = my-s3:\n");
        let crypt = &remotes[0];
        assert_eq!(crypt.kind, TransportKind::Other);
        assert!(!crypt.suitable_for_encrypted_backup());
    }

    /// The real production config keeps the encrypted remote as the choice.
    #[test]
    fn production_style_config_prefers_the_encrypted_remote() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("rclone.conf");
        std::fs::write(&config, REAL_CONFIG).unwrap();
        let remote = detect_rclone_remote_at(&config).unwrap();
        assert_eq!(remote.name, "gdrive-xavier-crypt");
        assert!(remote.suitable_for_encrypted_backup());
    }

    /// Non-Drive remotes still surface, so the caller can report rather than
    /// silently starting an OAuth flow it did not need to.
    #[test]
    fn non_drive_remote_is_reported_rather_than_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("rclone.conf");
        std::fs::write(&config, "[my-s3]\ntype = s3\n").unwrap();

        let remote = detect_rclone_remote_at(&config).unwrap();
        assert_eq!(remote.name, "my-s3");
        assert_eq!(remote.kind, TransportKind::Other);
    }

    /// Detection must be a pure read: no file is written or touched.
    #[test]
    fn detection_does_not_modify_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("rclone.conf");
        std::fs::write(&config, REAL_CONFIG).unwrap();
        let before = std::fs::read(&config).unwrap();

        detect_rclone_remote_at(&config).unwrap();
        assert_eq!(std::fs::read(&config).unwrap(), before);
    }

    /// The config path honours `$RCLONE_CONFIG`.
    #[test]
    fn config_path_honours_env() {
        let saved = std::env::var("RCLONE_CONFIG").ok();
        std::env::set_var("RCLONE_CONFIG", "/tmp/custom-rclone.conf");
        assert_eq!(
            rclone_config_path(),
            PathBuf::from("/tmp/custom-rclone.conf")
        );

        match saved {
            Some(v) => std::env::set_var("RCLONE_CONFIG", v),
            None => std::env::remove_var("RCLONE_CONFIG"),
        }
    }
}
