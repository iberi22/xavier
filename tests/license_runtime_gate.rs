//! Runtime Gate Tests for License Enforcement
//!
//! Verifies that core gating functions (require_mesh_license, require_commercial_license)
//! behave correctly based on settings.

use xavier::settings::XavierSettings;
use xavier::security::license::{require_mesh_license, require_commercial_license, accept_mesh_license, accept_commercial_license};

#[test]
fn test_runtime_gate_blocks_mesh_without_acceptance() {
    let settings = XavierSettings::default();
    let result = require_mesh_license(&settings);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("require the Xavier Mesh License"));
}

#[test]
fn test_runtime_gate_allows_mesh_after_acceptance() {
    let mut settings = XavierSettings::default();
    accept_mesh_license(&mut settings);
    assert!(require_mesh_license(&settings).is_ok());
}

#[test]
fn test_runtime_gate_blocks_commercial_without_license() {
    // Note: Behavior depends on whether "enterprise" feature is enabled.
    // If NOT enabled, require_commercial_license should return Ok (no-op).
    // If ENABLED, it should return Err.

    let settings = XavierSettings::default();
    let result = require_commercial_license(&settings);

    if cfg!(feature = "enterprise") {
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("require a Xavier Commercial License"));
    } else {
        assert!(result.is_ok());
    }
}

#[test]
fn test_runtime_gate_allows_commercial_with_license() {
    let mut settings = XavierSettings::default();
    accept_commercial_license(&mut settings, "valid-key");
    assert!(require_commercial_license(&settings).is_ok());
}

#[test]
fn test_accept_commercial_license_updates_settings() {
    let mut settings = XavierSettings::default();
    assert!(accept_commercial_license(&mut settings, "test-key-123"));
    assert!(settings.license.mesh_accepted);
    assert_eq!(settings.license.license_type, "Xavier-Commercial-1.0");
    assert_eq!(settings.license.commercial_key, Some("test-key-123".to_string()));
}
