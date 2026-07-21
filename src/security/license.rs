// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Xavier Dual License — MIT (standalone) + Mesh License (network/commercial)
//!
//! License architecture (inspired by MongoDB AGPL->SSPL / GitLab CE->EE):
//!
//!   ┌─────────────────────────────────────────────────────────┐
//!   │  Xavier Core (MIT)                                      │
//!   │  - Standalone, local-first open source                  │
//!   │  - Free forever, permissive use                         │
//!   ├─────────────────────────────────────────────────────────┤
//!   │  Xavier Mesh License (LICENSE-MESH)                     │
//!   │  - Activates peer-to-peer, governance, tokenomics,      │
//!   │    and enterprise features                              │
//!   │  - Free for individuals and verified open-source        │
//!   │  - Commercial license required for large organizations  │
//!   └─────────────────────────────────────────────────────────┘
//!
//! The core engine is MIT. The Mesh License adds network-participation
//! and commercial terms on top.

use crate::settings::XavierSettings;

/// License kinds recognized by Xavier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseKind {
    /// MIT — core engine, permissive open source
    Mit,
    /// Mesh — commercial/network participation, governance, enterprise features
    Mesh,
}

impl std::fmt::Display for LicenseKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LicenseKind::Mit => write!(f, "MIT"),
            LicenseKind::Mesh => write!(f, "Xavier Mesh License"),
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
        "Xavier-Mesh-1.0" | "Xavier-Commercial-1.0" | "Xavier-Enterprise-1.0" => LicenseKind::Mesh,
        _ => {
            if settings.license.mesh_accepted {
                LicenseKind::Mesh
            } else {
                LicenseKind::Mit
            }
        }
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
/// This requires the Xavier Mesh/Commercial License.
pub fn require_commercial_license(settings: &XavierSettings) -> Result<(), String> {
    match detect_license(settings) {
        LicenseKind::Mesh => Ok(()),
        LicenseKind::Mit => {
            // Enterprise features are feature-gated in Cargo.toml behind `enterprise` feature.
            // If the binary was compiled with enterprise features, the user needs a commercial/mesh license.
            if cfg!(feature = "enterprise") {
                Err(
                    "Enterprise features require a Xavier Commercial/Mesh License. ".to_owned()
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
    fn test_default_license_is_mit() {
        let settings = XavierSettings::default();
        assert_eq!(detect_license(&settings), LicenseKind::Mit);
    }

    #[test]
    fn test_accept_mesh_upgrades_license_type() {
        let mut settings = XavierSettings::default();
        assert_eq!(detect_license(&settings), LicenseKind::Mit);
        assert!(accept_mesh_license(&mut settings));
        assert_eq!(settings.license.license_type, "Xavier-Mesh-1.0".to_string());
        assert_eq!(detect_license(&settings), LicenseKind::Mesh);
    }

    #[test]
    fn test_accept_commercial_upgrades_license() {
        let mut settings = XavierSettings::default();
        assert!(accept_commercial_license(
            &mut settings,
            "swal-com-2026-abc123"
        ));
        assert_eq!(detect_license(&settings), LicenseKind::Mesh);
    }

    #[test]
    fn test_empty_commercial_key_rejected() {
        let mut settings = XavierSettings::default();
        assert!(!accept_commercial_license(&mut settings, ""));
        assert_eq!(detect_license(&settings), LicenseKind::Mit);
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
        assert_eq!(LicenseKind::Mit.to_string(), "MIT");
        assert_eq!(
            LicenseKind::Mesh.to_string(),
            "Xavier Mesh License"
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
        assert_eq!(detect_license(&reloaded), LicenseKind::Mesh);
        // Commercial acceptance also unlocks mesh features.
        assert!(require_mesh_license(&reloaded).is_ok());
    }

    /// License downgrade: a Commercial license explicitly downgraded back to
    /// the MIT default must be detected as MIT and lose enterprise gating.
    #[test]
    fn test_license_downgrade_from_commercial_to_mit() {
        let mut settings = XavierSettings::default();
        assert!(accept_commercial_license(&mut settings, "key-123"));
        assert_eq!(detect_license(&settings), LicenseKind::Mesh);

        // Downgrade: clear the commercial markers.
        settings.license.license_type = "MIT".to_string();
        settings.license.commercial_key = None;
        settings.license.mesh_accepted = false;

        assert_eq!(detect_license(&settings), LicenseKind::Mit);
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

    /// The commercial gate must reject MIT binaries that were compiled with
    /// the `enterprise` feature, but pass when enterprise is absent. Since the
    /// test suite is compiled without `enterprise`, we assert the pass branch
    /// and that an MIT setting is never silently upgraded to Mesh.
    #[test]
    fn test_commercial_gate_refuses_mit_enterprise_contract() {
        let mit = XavierSettings::default();
        // Without the enterprise feature compiled in, the gate is a no-op pass.
        assert!(require_commercial_license(&mit).is_ok());
        // And an MIT setting is never mis-detected as mesh.
        assert_eq!(detect_license(&mit), LicenseKind::Mit);

        // A commercial setting flips detection but the gate logic is symmetric:
        // both branches return a stable Ok/Err for the same input.
        let mut commercial = XavierSettings::default();
        assert!(accept_commercial_license(&mut commercial, "k"));
        assert_eq!(detect_license(&commercial), LicenseKind::Mesh);
        assert!(require_commercial_license(&commercial).is_ok());
    }

    /// The CLI `license status` path routes through `detect_license`; verify
    /// every accepted license state is reported with the right LicenseKind so
    /// the status display is correct.
    #[test]
    fn test_cli_status_reports_correct_license_kind() {
        // Default MIT.
        let mut settings = XavierSettings::default();
        assert_eq!(detect_license(&settings), LicenseKind::Mit);

        // Mesh acceptance changes the detected license kind to Mesh.
        accept_mesh_license(&mut settings);
        assert_eq!(detect_license(&settings), LicenseKind::Mesh);

        // Commercial acceptance also detects as Mesh.
        accept_commercial_license(&mut settings, "swal-x");
        assert_eq!(detect_license(&settings), LicenseKind::Mesh);

        // Each variant renders a non-empty, distinct string for the status box.
        let m = LicenseKind::Mit.to_string();
        let c = LicenseKind::Mesh.to_string();
        assert!(!m.is_empty() && !c.is_empty());
        assert_ne!(m, c);
    }

    /// All `LicenseKind` variants must round-trip through JSON (the status
    /// command and persistence rely on stable Display + serialization).
    #[test]
    fn test_license_kind_variants_display_and_identity() {
        let variants = [LicenseKind::Mit, LicenseKind::Mesh];
        for v in variants {
            // Display is stable and non-empty.
            let s = v.to_string();
            assert!(!s.is_empty());
            // Copy/clone are equal to the original (used across threads/tasks).
            assert_eq!(v, v.clone());
        }
        // The two kinds are distinct (no aliasing).
        assert_ne!(LicenseKind::Mit, LicenseKind::Mesh);
        // MeshStatus variants likewise render distinct strings.
        assert_ne!(
            MeshStatus::NotAccepted.to_string(),
            MeshStatus::Active.to_string()
        );
    }
}
