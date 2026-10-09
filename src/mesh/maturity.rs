pub use crate::domain::cycle_breaks::w30_04::MeshMaturityReport;

use crate::mesh::acl::MeshAcl;

/// Inputs a maturity report may be derived from. Every percentage emitted by
/// [`MeshMaturityReport::from_checks`] is a 0/100 projection of one of these
/// flags — nothing is invented.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MeshMaturityChecks {
    /// True when the HTTP mesh routes are enabled by the live license settings.
    pub http_routes_enabled: bool,
    /// True when a non-empty ACL store was observed on disk.
    pub acl_loaded: bool,
    /// Tokenomics engine state. `None` = unchecked, so `measured` is false.
    pub tokenomics_engine_present: Option<bool>,
    /// On-chain governance state. `None` = unchecked, so `measured` is false.
    pub onchain_gov_present: Option<bool>,
}

/// True only when the enforcement loader accepts a non-empty ACL, without writes.
pub fn acl_store_loaded() -> bool {
    MeshAcl::load_read_only()
        .map(|acl| acl.has_entries())
        .unwrap_or(false)
}

impl MeshMaturityReport {
    /// Report with no measured values: every percent is 0, `measured` is false.
    pub fn unmeasured() -> Self {
        Self {
            http_transport: false,
            http_transport_percent: 0,
            libp2p: false,
            libp2p_percent: 0,
            acl: false,
            acl_percent: 0,
            tokenomics: false,
            tokenomics_percent: 0,
            onchain_gov: false,
            onchain_gov_percent: 0,
            measured: false,
        }
    }

    /// Derive a report from checks performed in this process. Only 0/100
    /// percentages are ever emitted. `measured` is true only when every input
    /// was actually checked (`None` means "not checked").
    pub fn from_checks(c: &MeshMaturityChecks) -> Self {
        let http = c.http_routes_enabled;
        // The libp2p transport is a deprecated stub (every op returns
        // Libp2pDeprecated): compiled-but-deprecated reports 0 / false.
        let acl = c.acl_loaded;
        let tokenomics = c.tokenomics_engine_present.unwrap_or(false);
        let onchain_gov = c.onchain_gov_present.unwrap_or(false);
        Self {
            http_transport: http,
            http_transport_percent: if http { 100 } else { 0 },
            libp2p: false,
            libp2p_percent: 0,
            acl,
            acl_percent: if acl { 100 } else { 0 },
            tokenomics,
            tokenomics_percent: if tokenomics { 100 } else { 0 },
            onchain_gov,
            onchain_gov_percent: if onchain_gov { 100 } else { 0 },
            measured: c.tokenomics_engine_present.is_some() && c.onchain_gov_present.is_some(),
        }
    }
}

impl Default for MeshMaturityReport {
    fn default() -> Self {
        Self::unmeasured()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ConfigDirGuard(Option<std::ffi::OsString>);

    impl ConfigDirGuard {
        fn set(path: &std::path::Path) -> Self {
            let previous = std::env::var_os("XAVIER_CONFIG_DIR");
            std::env::set_var("XAVIER_CONFIG_DIR", path);
            Self(previous)
        }
    }

    impl Drop for ConfigDirGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("XAVIER_CONFIG_DIR", value),
                None => std::env::remove_var("XAVIER_CONFIG_DIR"),
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn acl_maturity_rejects_malformed_missing_and_unreadable_store() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let _guard = ConfigDirGuard::set(&config);
        assert!(!acl_store_loaded());
        assert!(!config.exists(), "health probe must not create directories");
        std::fs::create_dir(&config).unwrap();
        let path = config.join("mesh_acl.json");
        std::fs::write(&path, r#"{"n1":1}"#).unwrap();
        assert!(!acl_store_loaded());
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(!acl_store_loaded());
    }

    #[test]
    #[serial_test::serial]
    fn acl_maturity_entry_removed_reports_false() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = ConfigDirGuard::set(dir.path());
        let mut acl = MeshAcl::load().unwrap();
        let node = crate::mesh::node::NodeId("n1".to_string());
        acl.set_entry(
            node.clone(),
            crate::mesh::acl::NodeAclEntry {
                role: crate::enterprise::rbac::Role::Viewer,
                clearance: crate::memory::schema::ClearanceLevel::Internal,
                namespaces: None,
                public_key_hex: String::new(),
                namespace_acl: None,
            },
        )
        .unwrap();
        assert!(acl_store_loaded());
        acl.remove_entry(&node).unwrap();
        assert!(!acl_store_loaded());
    }

    #[test]
    fn default_maturity_is_unmeasured() {
        let r = MeshMaturityReport::default();
        assert!(!r.measured);
        assert_eq!(r.http_transport_percent, 0);
        assert_eq!(r.libp2p_percent, 0);
        assert_eq!(r.acl_percent, 0);
        assert_eq!(r.tokenomics_percent, 0);
        assert_eq!(r.onchain_gov_percent, 0);
        assert!(!r.http_transport);
        assert!(!r.libp2p);
        assert!(!r.acl);
        assert!(!r.tokenomics);
        assert!(!r.onchain_gov);
    }

    #[test]
    fn from_checks_maps_literal_inputs_to_literal_outputs() {
        // Literal in, literal out: no cfg! re-derivation.
        let c = MeshMaturityChecks {
            http_routes_enabled: true,
            acl_loaded: true,
            tokenomics_engine_present: Some(true),
            onchain_gov_present: Some(false),
        };
        let r = MeshMaturityReport::from_checks(&c);
        assert!(r.measured);
        assert!(r.http_transport);
        assert_eq!(r.http_transport_percent, 100);
        // Compiled-but-deprecated libp2p still reports 0 / false.
        assert!(!r.libp2p);
        assert_eq!(r.libp2p_percent, 0);
        assert!(r.acl);
        assert_eq!(r.acl_percent, 100);
        assert!(r.tokenomics);
        assert_eq!(r.tokenomics_percent, 100);
        assert!(!r.onchain_gov);
        assert_eq!(r.onchain_gov_percent, 0);
    }

    #[test]
    fn from_checks_unmeasured_when_any_input_unchecked() {
        let c = MeshMaturityChecks {
            http_routes_enabled: true,
            acl_loaded: true,
            tokenomics_engine_present: None,
            onchain_gov_present: Some(false),
        };
        let r = MeshMaturityReport::from_checks(&c);
        assert!(!r.measured);
        assert!(r.http_transport);
        assert_eq!(r.http_transport_percent, 100);
        assert!(!r.tokenomics);
        assert_eq!(r.tokenomics_percent, 0);
    }

    #[test]
    fn from_checks_never_invents_intermediate_percentages() {
        for bits in 0..16u32 {
            let opt = |b: u32| {
                if bits & b != 0 {
                    Some(true)
                } else {
                    Some(false)
                }
            };
            let c = MeshMaturityChecks {
                http_routes_enabled: bits & 1 != 0,
                acl_loaded: bits & 2 != 0,
                tokenomics_engine_present: opt(4),
                onchain_gov_present: opt(8),
            };
            let r = MeshMaturityReport::from_checks(&c);
            for p in [
                r.http_transport_percent,
                r.libp2p_percent,
                r.acl_percent,
                r.tokenomics_percent,
                r.onchain_gov_percent,
            ] {
                assert!(matches!(p, 0 | 100), "invented percentage: {p}");
            }
        }
    }

    #[test]
    fn default_checks_report_unmeasured_zeros() {
        let r = MeshMaturityReport::from_checks(&MeshMaturityChecks::default());
        assert!(!r.measured);
        assert_eq!(r.http_transport_percent, 0);
        assert_eq!(r.libp2p_percent, 0);
        assert_eq!(r.acl_percent, 0);
        assert_eq!(r.tokenomics_percent, 0);
        assert_eq!(r.onchain_gov_percent, 0);
    }

    #[test]
    fn unmeasured_report_serializes_as_measured_false() {
        let r = MeshMaturityReport::unmeasured();
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["measured"], false);
        assert_eq!(v["acl_percent"], 0);
        // Payloads without the key still deserialize, defaulting to false.
        let raw = serde_json::json!({
            "http_transport": false, "http_transport_percent": 0,
            "libp2p": false, "libp2p_percent": 0,
            "acl": false, "acl_percent": 0,
            "tokenomics": false, "tokenomics_percent": 0,
            "onchain_gov": false, "onchain_gov_percent": 0
        });
        let back: MeshMaturityReport = serde_json::from_value(raw).unwrap();
        assert!(!back.measured);
    }
}
