use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

pub const PROTOCOL_VERSION: &str = "xavier-core/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub protocol_version: String,
    pub message_id: Uuid,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u32,
    pub minor: u32,
}

impl FromStr for ProtocolVersion {
    type Err = ProtocolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 2 || parts[0] != "xavier-core" {
            return Err(ProtocolError::Malformed);
        }

        let ver_parts: Vec<&str> = parts[1].split('.').collect();
        let major = ver_parts
            .first()
            .and_then(|m| m.parse().ok())
            .ok_or(ProtocolError::Malformed)?;

        let minor = if ver_parts.len() > 1 {
            ver_parts[1].parse().map_err(|_| ProtocolError::Malformed)?
        } else {
            0
        };

        Ok(ProtocolVersion { major, minor })
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.minor == 0 {
            write!(f, "xavier-core/{}", self.major)
        } else {
            write!(f, "xavier-core/{}.{}", self.major, self.minor)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_version: String,
    pub sidecar_id: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub negotiated_version: String,
    #[serde(default)]
    pub core_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    VersionSkew,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reject {
    pub reason: RejectReason,
    pub min_supported: String,
    pub max_supported: String,
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("protocol version skew")]
    VersionSkew,
    #[error("frame too large")]
    FrameTooLarge,
    #[error("malformed frame")]
    Malformed,
    #[error("io error")]
    Io,
    #[error("timeout")]
    Timeout,
}

pub fn negotiate(core: &str, sidecar: &str) -> Result<ProtocolVersion, ProtocolError> {
    let core_ver = ProtocolVersion::from_str(core)?;
    let sidecar_ver = ProtocolVersion::from_str(sidecar)?;

    if core_ver.major != sidecar_ver.major {
        return Err(ProtocolError::VersionSkew);
    }

    Ok(ProtocolVersion {
        major: core_ver.major,
        minor: std::cmp::max(core_ver.minor, sidecar_ver.minor),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_frame_roundtrip() {
        let frame = Frame {
            protocol_version: PROTOCOL_VERSION.to_string(),
            message_id: Uuid::new_v4(),
            kind: "codegraph.parse.request".to_string(),
            payload: json!({ "path": "src/main.rs" }),
        };

        let serialized = serde_json::to_string(&frame).unwrap();
        let deserialized: Frame = serde_json::from_str(&serialized).unwrap();
        assert_eq!(frame, deserialized);
    }

    #[test]
    fn test_unknown_fields_tolerated() {
        let json_str = r#"{
            "protocol_version": "xavier-core/1",
            "message_id": "00000000-0000-0000-0000-000000000000",
            "kind": "test",
            "payload": {},
            "unknown_extra_field": "tolerated"
        }"#;

        let deserialized: Frame = serde_json::from_str(json_str).unwrap();
        assert_eq!(deserialized.protocol_version, "xavier-core/1");
        assert_eq!(deserialized.kind, "test");
    }

    #[test]
    fn test_hello_roundtrip() {
        let hello = Hello {
            protocol_version: PROTOCOL_VERSION.to_string(),
            sidecar_id: "test-sidecar".to_string(),
            capabilities: vec!["c1".to_string()],
        };

        let serialized = serde_json::to_string(&hello).unwrap();
        let deserialized: Hello = serde_json::from_str(&serialized).unwrap();
        assert_eq!(hello, deserialized);
    }

    #[test]
    fn test_welcome_roundtrip() {
        let welcome = Welcome {
            negotiated_version: PROTOCOL_VERSION.to_string(),
            core_capabilities: vec![],
        };

        let serialized = serde_json::to_string(&welcome).unwrap();
        let deserialized: Welcome = serde_json::from_str(&serialized).unwrap();
        assert_eq!(welcome, deserialized);
    }

    #[test]
    fn test_reject_roundtrip() {
        let reject = Reject {
            reason: RejectReason::VersionSkew,
            min_supported: "xavier-core/1".to_string(),
            max_supported: "xavier-core/1".to_string(),
        };

        let serialized = serde_json::to_string(&reject).unwrap();
        let deserialized: Reject = serde_json::from_str(&serialized).unwrap();
        assert_eq!(reject, deserialized);
    }

    #[test]
    fn test_negotiate_same_version() {
        assert_eq!(
            negotiate("xavier-core/1", "xavier-core/1"),
            Ok(ProtocolVersion { major: 1, minor: 0 })
        );
    }

    #[test]
    fn test_negotiate_higher_minor_ok() {
        assert_eq!(
            negotiate("xavier-core/1.0", "xavier-core/1.1"),
            Ok(ProtocolVersion { major: 1, minor: 1 })
        );
        assert_eq!(
            negotiate("xavier-core/1.2", "xavier-core/1.1"),
            Ok(ProtocolVersion { major: 1, minor: 2 })
        );
    }

    #[test]
    fn test_negotiate_higher_major_rejected() {
        assert_eq!(
            negotiate("xavier-core/1", "xavier-core/2"),
            Err(ProtocolError::VersionSkew)
        );
    }

    #[test]
    fn test_negotiate_lower_major_rejected() {
        assert_eq!(
            negotiate("xavier-core/2", "xavier-core/1"),
            Err(ProtocolError::VersionSkew)
        );
    }

    #[test]
    fn test_negotiate_malformed() {
        assert_eq!(
            negotiate("xavier-core/1", "bad-format/1"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            negotiate("xavier-core/1", "xavier-core/abc"),
            Err(ProtocolError::Malformed)
        );
    }
}
