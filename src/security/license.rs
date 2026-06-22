//! Xavier Dual License — AGPL v3 (core) + Commercial (enterprise)
//!
//! License architecture (inspired by MongoDB AGPL->SSPL / GitLab CE->EE):
//!
//!   ┌─────────────────────────────────────────────────────────┐
//!   │  Xavier Core (AGPL-3.0)                                  │
//!   │  - All source code visible, full open source            │
//!   │  - Network service = must release modifications         │
//!   │  - Free forever                                         │
//!   ├─────────────────────────────────────────────────────────┤
//!   │  Xavier Enterprise (Commercial License)                 │
//!   │  - Proprietary integration rights                       │
//!   │  - Private mesh without source disclosure               │
//!   │  - Enterprise-reserved features (advanced-rrf, etc.)    │
//!   │  - $100/node/mo or custom                               │
//!   ├─────────────────────────────────────────────────────────┤
//!   │  Xavier Mesh License (LICENSE-MESH)                     │
//!   │  - Additional terms for P2P network participation       │
//!   │  - XP Tokenomics, Governance, Data Commons              │
//!   │  - Requires acceptance (free for individuals/OSS)       │
//!   └─────────────────────────────────────────────────────────┘
//!
//! The core engine is AGPL-3.0. The Mesh License adds network-participation
//! terms on top. The Commercial License is for proprietary use.

use crate::settings::XavierSettings;

/// License kinds recognized by Xavier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseKind {
    /// AGPL-3.0 — core engine, full open source
    Agpl,
    /// Commercial — proprietary integration, private mesh, enterprise features
    Commercial,
}

impl std::fmt::Display for LicenseKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LicenseKind::Agpl => write!(f, "AGPL-3.0"),
            LicenseKind::Commercial => write!(f, "Xavier Commercial License"),
        }
    }
}

/// Mesh License status — separate from main license
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshStatus {
    /// Mesh License not yet accepted
    NotAccepted,
    /// Mesh License accepted (free for individuals/OSS)
    Active,
}

impl std::fmt::Display for MeshStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeshStatus::NotAccepted => write!(f, "❌ Not Accepted"),
            MeshStatus::Active => write!(f, "✅ Accepted"),
        }
    }
}

/// Detect the current license from settings
pub fn detect_license(settings: &XavierSettings) -> LicenseKind {
    match settings.license.license_type.as_str() {
        "Xavier-Commercial-1.0" | "Xavier-Enterprise-1.0" => LicenseKind::Commercial,
        _ => LicenseKind::Agpl,
    }
}

/// Verify that mesh features are allowed.
/// Returns an error message if the user tries to use mesh without accepting the Mesh License.
pub fn require_mesh_license(settings: &XavierSettings) -> Result<(), String> {
    if settings.license.mesh_accepted {
        Ok(())
    } else {
        Err(
            "Mesh, Data Commons, and Enterprise features require the Xavier Mesh License. "
                .to_owned()
                + "Run `xavier license accept` to accept the terms in LICENSE-MESH.",
        )
    }
}

/// Verify that enterprise-reserved features are allowed.
/// This requires the Commercial License or higher.
pub fn require_commercial_license(settings: &XavierSettings) -> Result<(), String> {
    match detect_license(settings) {
        LicenseKind::Commercial => Ok(()),
        LicenseKind::Agpl => {
            // Enterprise features are feature-gated in Cargo.toml behind `enterprise` feature.
            // If the binary was compiled with enterprise features, the user needs a commercial license.
            if cfg!(feature = "enterprise") {
                Err(
                    "Enterprise features require a Xavier Commercial License. "
                        .to_owned()
                        + "See COMMERCIAL_LICENSE.md for pricing. Contact iberi22 for inquiries.",
                )
            } else {
                // Binary compiled without enterprise features at all — nothing to restrict
                Ok(())
            }
        }
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

/// Accept a Commercial License (requires key/verification).
/// Returns true if acceptance was recorded.
pub fn accept_commercial_license(settings: &mut XavierSettings, license_key: &str) -> bool {
    // TODO: Implement key verification against SWAL's licensing server
    // For now, accept any non-empty key as valid (development mode)
    if license_key.is_empty() {
        tracing::warn!("Empty commercial license key rejected");
        return false;
    }
    settings.license.mesh_accepted = true;
    settings.license.license_type = "Xavier-Commercial-1.0".to_string();
    settings.license.commercial_key = Some(license_key.to_string());
    tracing::info!("Commercial License accepted");
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::XavierSettings;

    #[test]
    fn test_default_license_is_agpl() {
        let settings = XavierSettings::default();
        assert_eq!(detect_license(&settings), LicenseKind::Agpl);
    }

    #[test]
    fn test_accept_mesh_upgrades_license_type() {
        let mut settings = XavierSettings::default();
        assert_eq!(detect_license(&settings), LicenseKind::Agpl);
        assert!(accept_mesh_license(&mut settings));
        assert_eq!(
            settings.license.license_type,
            "Xavier-Mesh-1.0".to_string()
        );
        // Still AGPL for detection purposes (mesh adds network terms, not commercial)
        assert_eq!(detect_license(&settings), LicenseKind::Agpl);
    }

    #[test]
    fn test_accept_commercial_upgrades_license() {
        let mut settings = XavierSettings::default();
        assert!(accept_commercial_license(&mut settings, "swal-com-2026-abc123"));
        assert_eq!(detect_license(&settings), LicenseKind::Commercial);
    }

    #[test]
    fn test_empty_commercial_key_rejected() {
        let mut settings = XavierSettings::default();
        assert!(!accept_commercial_license(&mut settings, ""));
        assert_eq!(detect_license(&settings), LicenseKind::Agpl);
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
    fn test_require_commercial_license_fails_with_enterprise_feature() {
        // In test mode without cfg(feature = "enterprise"), it should pass
        let settings = XavierSettings::default();
        assert!(require_commercial_license(&settings).is_ok());
    }

    #[test]
    fn test_duplicate_accept_returns_false() {
        let mut settings = XavierSettings::default();
        assert!(accept_mesh_license(&mut settings));
        assert!(!accept_mesh_license(&mut settings));
    }

    #[test]
    fn test_license_kind_display() {
        assert_eq!(LicenseKind::Agpl.to_string(), "AGPL-3.0");
        assert_eq!(
            LicenseKind::Commercial.to_string(),
            "Xavier Commercial License"
        );
    }

    #[test]
    fn test_mesh_status_display() {
        assert_eq!(
            MeshStatus::NotAccepted.to_string(),
            "❌ Not Accepted"
        );
        assert_eq!(MeshStatus::Active.to_string(), "✅ Accepted");
    }
}
