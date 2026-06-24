//! Cargo Feature Gate Tests for License Enforcement
//!
//! Verifies that require_commercial_license() behaves correctly depending
//! on the "enterprise" cargo feature.

use xavier::settings::XavierSettings;
use xavier::security::license::require_commercial_license;

#[test]
fn test_commercial_license_requirement_gated_by_feature() {
    let settings = XavierSettings::default();
    let result = require_commercial_license(&settings);

    if cfg!(feature = "enterprise") {
        // If enterprise feature is on, it must be blocked without commercial license
        assert!(result.is_err(), "Should be blocked when 'enterprise' feature is enabled");
        assert!(result.unwrap_err().contains("require a Xavier Commercial License"));
    } else {
        // If enterprise feature is off, it should be allowed (no-op)
        assert!(result.is_ok(), "Should NOT be blocked when 'enterprise' feature is disabled");
    }
}
