//! Exact Google Drive scope profiles for recovery commands.

use crate::drive::DriveScopes;

/// Drive scope profile for recovery commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DriveScopeProfile {
    /// Default: narrow `drive.file` scope.
    #[default]
    Backup,
    /// Opt-in: full `drive` scope.
    Full,
}

impl DriveScopeProfile {
    /// Resolve the profile from the CLI opt-in flag.
    pub fn from_cli(full_scope: bool) -> Self {
        if full_scope {
            Self::Full
        } else {
            Self::Backup
        }
    }

    /// The single scope string this profile requests.
    pub fn requested_scope(&self) -> &'static str {
        match self {
            Self::Backup => DriveScopes::BACKUP,
            Self::Full => DriveScopes::FULL,
        }
    }

    /// Whether a set of granted scopes satisfies this profile.
    pub fn is_satisfied_by(&self, granted: &[String]) -> bool {
        match self {
            Self::Backup => granted.iter().any(|s| s == DriveScopes::BACKUP),
            Self::Full => granted.iter().any(|s| s == DriveScopes::FULL),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_is_backup() {
        assert_eq!(DriveScopeProfile::default(), DriveScopeProfile::Backup);
    }

    #[test]
    fn resolve_from_cli_respects_flag() {
        assert_eq!(
            DriveScopeProfile::from_cli(false),
            DriveScopeProfile::Backup
        );
        assert_eq!(DriveScopeProfile::from_cli(true), DriveScopeProfile::Full);
    }

    #[test]
    fn mapping_to_scopes_is_correct() {
        assert_eq!(
            DriveScopeProfile::Backup.requested_scope(),
            DriveScopes::BACKUP
        );
        assert_eq!(DriveScopeProfile::Full.requested_scope(), DriveScopes::FULL);
    }

    #[test]
    fn full_scope_remains_unrequested_by_default() {
        // Contract explicitly requires that DriveScopes::is_requested returns false for FULL
        // meaning FULL is an explicit opt-in override for one-off CLI flows only.
        assert!(!DriveScopes::is_requested(DriveScopes::FULL));
        assert!(DriveScopes::is_requested(DriveScopes::BACKUP));
    }

    #[test]
    fn backup_requires_exact_scope() {
        let profile = DriveScopeProfile::Backup;
        assert!(profile.is_satisfied_by(&[DriveScopes::BACKUP.to_string()]));
        assert!(!profile.is_satisfied_by(&["something/drive.file".to_string()]));
        assert!(!profile.is_satisfied_by(&[DriveScopes::FULL.to_string()]));
    }

    #[test]
    fn full_requires_exact_scope() {
        let profile = DriveScopeProfile::Full;
        assert!(profile.is_satisfied_by(&[DriveScopes::FULL.to_string()]));
        assert!(!profile.is_satisfied_by(&["something/auth/drive".to_string()]));
        assert!(!profile.is_satisfied_by(&[DriveScopes::BACKUP.to_string()]));
    }

    #[test]
    fn disconnect_removes_stored_credential_and_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::drive::DriveCredentialStore::at(dir.path());

        assert!(!store.exists(), "store should be empty initially");

        // A direct mock of `drive_disconnect` behavior: calling `remove` when file does not exist
        // is idempotent and succeeds.
        let res = store.remove();
        assert!(
            res.is_ok(),
            "disconnect is idempotent and returns Ok when file does not exist"
        );

        // Provide a valid fake credential file
        let creds = crate::drive::DriveCredentials {
            expires_at: 1_700_000_000,
            scopes: vec![DriveScopes::BACKUP.to_string()],
            refresh_token: "fake-refresh-token".to_string(),
        };
        let master_key = [0u8; 32];
        store.store(&creds, &master_key).unwrap();

        assert!(store.exists(), "store should exist after storing");

        let res = store.remove();
        assert!(res.is_ok(), "disconnect should succeed when file exists");
        assert!(!store.exists(), "store should be empty after disconnect");
    }
}
