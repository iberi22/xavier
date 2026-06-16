//! Xavier Dual License — MIT (standalone) + Mesh License (network participation)
//!
//! This module handles license detection, acceptance, and feature gating.
//! The core logic: if mesh features are enabled but Mesh License not accepted,
//! the startup logs a warning and mesh commands are unavailable.

use crate::settings::XavierSettings;

/// License kinds recognized by Xavier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseKind {
    /// MIT — standalone use only, no mesh features
    Mit,
    /// Mesh License — full network participation
    Mesh,
}

impl std::fmt::Display for LicenseKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LicenseKind::Mit => write!(f, "MIT"),
            LicenseKind::Mesh => write!(f, "Xavier Mesh License v1.0"),
        }
    }
}

/// Check the current license from settings and return the resolved kind.
pub fn detect_license(settings: &XavierSettings) -> LicenseKind {
    if settings.license.mesh_accepted {
        LicenseKind::Mesh
    } else {
        LicenseKind::Mit
    }
}

/// Verify that mesh features are allowed under the active license.
/// Returns an error message if the user tries to use mesh without accepting the Mesh License.
pub fn require_mesh_license(settings: &XavierSettings) -> Result<(), String> {
    if settings.license.mesh_accepted {
        Ok(())
    } else {
        Err(
            "Mesh features require the Xavier Mesh License. "
                .to_owned()
                .to_owned()
                + "Run `xavier license accept` to accept the terms in LICENSE-MESH.",
        )
    }
}

/// Accept the Mesh License. Returns true if acceptance was recorded.
pub fn accept_mesh_license(settings: &mut XavierSettings) -> bool {
    if settings.license.mesh_accepted {
        tracing::info!("Mesh License already accepted");
        return false;
    }
    settings.license.mesh_accepted = true;
    settings.license.license_type = "Xavier-Mesh-1.0".to_string();
    tracing::info!("Mesh License accepted");
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::XavierSettings;

    #[test]
    fn test_default_license_is_mit() {
        let settings = XavierSettings::default();
        assert_eq!(detect_license(&settings), LicenseKind::Mit);
    }

    #[test]
    fn test_accept_upgrades_to_mesh() {
        let mut settings = XavierSettings::default();
        assert_eq!(detect_license(&settings), LicenseKind::Mit);
        assert!(accept_mesh_license(&mut settings));
        assert_eq!(detect_license(&settings), LicenseKind::Mesh);
    }

    #[test]
    fn test_require_mesh_license_fails_without_acceptance() {
        let settings = XavierSettings::default();
        assert!(require_mesh_license(&settings).is_err());
    }

    #[test]
    fn test_require_mesh_license_passes_with_acceptance() {
        let mut settings = XavierSettings::default();
        accept_mesh_license(&mut settings);
        assert!(require_mesh_license(&settings).is_ok());
    }

    #[test]
    fn test_duplicate_accept_returns_false() {
        let mut settings = XavierSettings::default();
        accept_mesh_license(&mut settings);
        assert!(!accept_mesh_license(&mut settings));
    }
}
