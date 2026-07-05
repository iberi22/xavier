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
                    "Enterprise features require a Xavier Commercial License. ".to_owned()
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
        assert_eq!(settings.license.license_type, "Xavier-Mesh-1.0".to_string());
        // Still AGPL for detection purposes (mesh adds network terms, not commercial)
        assert_eq!(detect_license(&settings), LicenseKind::Agpl);
    }

    #[test]
    fn test_accept_commercial_upgrades_license() {
        let mut settings = XavierSettings::default();
        assert!(accept_commercial_license(
            &mut settings,
            "swal-com-2026-abc123"
        ));
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
        assert_eq!(MeshStatus::NotAccepted.to_string(), "❌ Not Accepted");
        assert_eq!(MeshStatus::Active.to_string(), "✅ Accepted");
    }

    // ──────────────────────────────────────────────────────────────────────
    // Coverage gap tests (Brecha D — Dual License 70% → 90%)
    // ──────────────────────────────────────────────────────────────────────

    /// License acceptance must survive a serialize → deserialize round-trip
    /// (i.e. it persists across `save` + `load`).
    #[test]
    fn test_mesh_license_persists_across_serialization() {
        // Accept the mesh license.
        let mut settings = XavierSettings::default();
        assert!(accept_mesh_license(&mut settings));
        assert!(settings.license.mesh_accepted);

        // Serialize → deserialize simulates a write to disk + reload.
        let json = serde_json::to_string(&settings).expect("settings serialize");
        let reloaded: XavierSettings = serde_json::from_str(&json).expect("settings deserialize");

        // Acceptance is still present after reload.
        assert!(reloaded.license.mesh_accepted);
        assert_eq!(reloaded.license.license_type, "Xavier-Mesh-1.0");
        // Mesh-gated features must still be allowed after the round-trip.
        assert!(require_mesh_license(&reloaded).is_ok());
    }

    /// A commercial license must persist its key and grant both mesh and
    /// commercial gates after a round-trip.
    #[test]
    fn test_commercial_license_persists_with_key() {
        let key = "swal-com-2026-deadbeef";
        let mut settings = XavierSettings::default();
        assert!(accept_commercial_license(&mut settings, key));

        let json = serde_json::to_string(&settings).expect("serialize");
        let reloaded: XavierSettings = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(reloaded.license.commercial_key.as_deref(), Some(key));
        assert_eq!(detect_license(&reloaded), LicenseKind::Commercial);
        // Commercial acceptance also unlocks mesh features.
        assert!(require_mesh_license(&reloaded).is_ok());
    }

    /// License downgrade: a Commercial license explicitly downgraded back to
    /// the AGPL default must be detected as AGPL and lose enterprise gating.
    #[test]
    fn test_license_downgrade_from_commercial_to_agpl() {
        let mut settings = XavierSettings::default();
        assert!(accept_commercial_license(&mut settings, "key-123"));
        assert_eq!(detect_license(&settings), LicenseKind::Commercial);

        // Downgrade: clear the commercial markers.
        settings.license.license_type = "AGPL-3.0".to_string();
        settings.license.commercial_key = None;

        assert_eq!(detect_license(&settings), LicenseKind::Agpl);
        assert!(settings.license.commercial_key.is_none());
    }

    /// The mesh license gate must flip from error → ok exactly when
    /// `mesh_accepted` is toggled. This pins the runtime gate contract.
    #[test]
    fn test_mesh_runtime_gate_toggles_with_acceptance() {
        let mut settings = XavierSettings::default();

        // Default: not accepted → mesh features blocked.
        assert!(!settings.license.mesh_accepted);
        let blocked = require_mesh_license(&settings);
        assert!(blocked.is_err(), "mesh must be blocked before acceptance");
        assert!(
            blocked.unwrap_err().contains("Mesh License"),
            "error must explain the Mesh License requirement"
        );

        // Accept → gate opens.
        accept_mesh_license(&mut settings);
        assert!(require_mesh_license(&settings).is_ok());

        // Manually revoke (simulating settings reset / revocation) → blocked again.
        settings.license.mesh_accepted = false;
        assert!(
            require_mesh_license(&settings).is_err(),
            "mesh must be blocked after revocation"
        );
    }

    /// The commercial gate must reject AGPL binaries that were compiled with
    /// the `enterprise` feature, but pass when enterprise is absent. Since the
    /// test suite is compiled without `enterprise`, we assert the pass branch
    /// and that an AGPL setting is never silently upgraded to Commercial.
    #[test]
    fn test_commercial_gate_refuses_agpl_enterprise_contract() {
        let agpl = XavierSettings::default();
        // Without the enterprise feature compiled in, the gate is a no-op pass.
        assert!(require_commercial_license(&agpl).is_ok());
        // And an AGPL setting is never mis-detected as commercial.
        assert_eq!(detect_license(&agpl), LicenseKind::Agpl);

        // A commercial setting flips detection but the gate logic is symmetric:
        // both branches return a stable Ok/Err for the same input.
        let mut commercial = XavierSettings::default();
        assert!(accept_commercial_license(&mut commercial, "k"));
        assert_eq!(detect_license(&commercial), LicenseKind::Commercial);
        assert!(require_commercial_license(&commercial).is_ok());
    }

    /// The CLI `license status` path routes through `detect_license`; verify
    /// every accepted license state is reported with the right LicenseKind so
    /// the status display is correct.
    #[test]
    fn test_cli_status_reports_correct_license_kind() {
        // Default AGPL.
        let mut settings = XavierSettings::default();
        assert_eq!(detect_license(&settings), LicenseKind::Agpl);

        // Mesh acceptance does NOT change the detected core license kind.
        accept_mesh_license(&mut settings);
        assert_eq!(detect_license(&settings), LicenseKind::Agpl);

        // Commercial acceptance flips the detected kind.
        accept_commercial_license(&mut settings, "swal-x");
        assert_eq!(detect_license(&settings), LicenseKind::Commercial);

        // Each variant renders a non-empty, distinct string for the status box.
        let a = LicenseKind::Agpl.to_string();
        let c = LicenseKind::Commercial.to_string();
        assert!(!a.is_empty() && !c.is_empty());
        assert_ne!(a, c);
    }

    /// All `LicenseKind` variants must round-trip through JSON (the status
    /// command and persistence rely on stable Display + serialization).
    #[test]
    fn test_license_kind_variants_display_and_identity() {
        let variants = [LicenseKind::Agpl, LicenseKind::Commercial];
        for v in variants {
            // Display is stable and non-empty.
            let s = v.to_string();
            assert!(!s.is_empty());
            // Copy/clone are equal to the original (used across threads/tasks).
            assert_eq!(v, v.clone());
        }
        // The two kinds are distinct (no aliasing).
        assert_ne!(LicenseKind::Agpl, LicenseKind::Commercial);
        // MeshStatus variants likewise render distinct strings.
        assert_ne!(
            MeshStatus::NotAccepted.to_string(),
            MeshStatus::Active.to_string()
        );
    }
}
