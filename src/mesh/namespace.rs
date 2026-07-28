//! SWAL namespace helpers — instance isolation for mesh data plane.
//!
//! Canonical mesh namespace: `swal/{appId}/{instanceId}` (NODE_PRO_AND_INSTANCES §3).
//! Xavier memory namespace: `app/{appId}/instance/{instanceId}`.
//! Two installs of the same app MUST NOT share namespaces by default.

use anyhow::{bail, Result};

/// Build the edge-mesh / P2P data-plane namespace.
pub fn swal_namespace(app_id: &str, instance_id: &str) -> Result<String> {
    let app = normalize_segment(app_id, "appId")?;
    let instance = normalize_segment(instance_id, "instanceId")?;
    Ok(format!("swal/{app}/{instance}"))
}

/// Build the Xavier memory namespace for the same install.
pub fn xavier_memory_namespace(app_id: &str, instance_id: &str) -> Result<String> {
    let app = normalize_segment(app_id, "appId")?;
    let instance = normalize_segment(instance_id, "instanceId")?;
    Ok(format!("app/{app}/instance/{instance}"))
}

/// Parse `swal/{appId}/{instanceId}` → `(appId, instanceId)`.
pub fn parse_swal_namespace(ns: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = ns.split('/').collect();
    if parts.len() != 3 || parts[0] != "swal" {
        return None;
    }
    if parts[1].is_empty() || parts[2].is_empty() {
        return None;
    }
    Some((parts[1].to_string(), parts[2].to_string()))
}

/// True when two namespaces belong to different instances (must not mix data).
pub fn namespaces_are_isolated(a: &str, b: &str) -> bool {
    if a == b {
        return false;
    }
    match (parse_swal_namespace(a), parse_swal_namespace(b)) {
        (Some((app_a, inst_a)), Some((app_b, inst_b))) => app_a != app_b || inst_a != inst_b,
        _ => a != b,
    }
}

fn normalize_segment(raw: &str, label: &str) -> Result<String> {
    let s = raw.trim();
    if s.is_empty() {
        bail!("{label} must not be empty");
    }
    if s.contains('/') || s.contains('\\') {
        bail!("{label} must not contain path separators");
    }
    Ok(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swal_namespace_format() {
        assert_eq!(
            swal_namespace("worldexams", "inst-aaa").unwrap(),
            "swal/worldexams/inst-aaa"
        );
    }

    #[test]
    fn two_instances_do_not_share_namespace() {
        let a = swal_namespace("app", "i1").unwrap();
        let b = swal_namespace("app", "i2").unwrap();
        assert_ne!(a, b);
        assert!(namespaces_are_isolated(&a, &b));
        assert!(!namespaces_are_isolated(&a, &a));
    }

    #[test]
    fn reject_empty_or_slash() {
        assert!(swal_namespace("", "x").is_err());
        assert!(swal_namespace("a/b", "x").is_err());
    }
}
