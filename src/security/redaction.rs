use crate::security::acl::ClearanceLevel;
use serde::{Deserialize, Serialize};

/// Represents a segmented document that has multiple sections,
/// each with its own clearance level classification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentedDocument {
    pub id: String,
    pub sections: Vec<DocSection>,
}

/// A specific section of a segmented document, containing its identifier,
/// title, clearance requirement, and content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocSection {
    pub id: String,
    pub title: String,
    pub clearance: ClearanceLevel,
    pub content: String,
}

/// Represents a redacted document where sections requiring clearance higher than
/// the requester's clearance level have been obfuscated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedactedDocument {
    pub id: String,
    pub sections: Vec<RedactedDocSection>,
}

/// A section within a redacted document, containing its identifier,
/// title, clearance level, and potentially redacted content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedactedDocSection {
    pub id: String,
    pub title: String,
    pub clearance: ClearanceLevel,
    pub content: String,
}

/// Redacts a segmented document by replacing section content with `[REDACTED: {title}]`
/// for any section whose clearance requirement is greater than the requester's clearance level.
///
/// Sections requiring clearance less than or equal to the requester's level are kept unmodified.
pub fn redact(doc: &SegmentedDocument, requester_clearance: ClearanceLevel) -> RedactedDocument {
    let sections = doc
        .sections
        .iter()
        .map(|sec| {
            let content = if sec.clearance > requester_clearance {
                format!("[REDACTED: {}]", sec.title)
            } else {
                sec.content.clone()
            };
            RedactedDocSection {
                id: sec.id.clone(),
                title: sec.title.clone(),
                clearance: sec.clearance,
                content,
            }
        })
        .collect();

    RedactedDocument {
        id: doc.id.clone(),
        sections,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topsecret_hidden_from_public_reader() {
        // Test: sección TOPSECRET oculta a lector UNCLASSIFIED (Public)
        let doc = SegmentedDocument {
            id: "doc-1".to_string(),
            sections: vec![
                DocSection {
                    id: "sec-intro".to_string(),
                    title: "Introducción".to_string(),
                    clearance: ClearanceLevel::Public,
                    content: "This is a public intro.".to_string(),
                },
                DocSection {
                    id: "sec-exploit".to_string(),
                    title: "Vulnerabilidad Crítica".to_string(),
                    clearance: ClearanceLevel::TopSecret,
                    content: "Explotación del desbordamiento de búfer...".to_string(),
                },
            ],
        };

        let redacted = redact(&doc, ClearanceLevel::Public);

        assert_eq!(redacted.sections.len(), 2);
        assert_eq!(redacted.sections[0].content, "This is a public intro.");
        assert_eq!(
            redacted.sections[1].content,
            "[REDACTED: Vulnerabilidad Crítica]"
        );
    }

    #[test]
    fn test_topsecret_visible_to_topsecret_reader() {
        let doc = SegmentedDocument {
            id: "doc-1".to_string(),
            sections: vec![DocSection {
                id: "sec-exploit".to_string(),
                title: "Vulnerabilidad Crítica".to_string(),
                clearance: ClearanceLevel::TopSecret,
                content: "Explotación del desbordamiento de búfer...".to_string(),
            }],
        };

        let redacted = redact(&doc, ClearanceLevel::TopSecret);

        assert_eq!(redacted.sections.len(), 1);
        assert_eq!(
            redacted.sections[0].content,
            "Explotación del desbordamiento de búfer..."
        );
    }

    #[test]
    fn test_public_content_visible_to_all() {
        let doc = SegmentedDocument {
            id: "doc-1".to_string(),
            sections: vec![DocSection {
                id: "sec-1".to_string(),
                title: "General".to_string(),
                clearance: ClearanceLevel::Public,
                content: "Hello World".to_string(),
            }],
        };

        let redacted_public = redact(&doc, ClearanceLevel::Public);
        let redacted_internal = redact(&doc, ClearanceLevel::Internal);
        let redacted_topsecret = redact(&doc, ClearanceLevel::TopSecret);

        assert_eq!(redacted_public.sections[0].content, "Hello World");
        assert_eq!(redacted_internal.sections[0].content, "Hello World");
        assert_eq!(redacted_topsecret.sections[0].content, "Hello World");
    }

    #[test]
    fn test_segmented_document_serialization_deserialization() {
        let doc = SegmentedDocument {
            id: "doc-123".to_string(),
            sections: vec![DocSection {
                id: "sec-1".to_string(),
                title: "Internal Plan".to_string(),
                clearance: ClearanceLevel::Internal,
                content: "Internal operations guidelines.".to_string(),
            }],
        };

        let json_str = serde_json::to_string(&doc).expect("Failed to serialize SegmentedDocument");
        assert!(json_str.contains("\"id\":\"doc-123\""));
        assert!(json_str.contains("\"sections\""));
        assert!(json_str.contains("\"clearance\":\"internal\""));

        let deserialized: SegmentedDocument =
            serde_json::from_str(&json_str).expect("Failed to deserialize SegmentedDocument");
        assert_eq!(deserialized, doc);
    }

    #[test]
    fn test_redacted_document_serialization_deserialization() {
        let redacted_doc = RedactedDocument {
            id: "doc-123".to_string(),
            sections: vec![RedactedDocSection {
                id: "sec-1".to_string(),
                title: "Secret Details".to_string(),
                clearance: ClearanceLevel::Secret,
                content: "[REDACTED: Secret Details]".to_string(),
            }],
        };

        let json_str =
            serde_json::to_string(&redacted_doc).expect("Failed to serialize RedactedDocument");
        assert!(json_str.contains("\"id\":\"doc-123\""));
        assert!(json_str.contains("\"content\":\"[REDACTED: Secret Details]\""));

        let deserialized: RedactedDocument =
            serde_json::from_str(&json_str).expect("Failed to deserialize RedactedDocument");
        assert_eq!(deserialized, redacted_doc);
    }

    #[test]
    fn test_empty_document_redaction() {
        let doc = SegmentedDocument {
            id: "doc-empty".to_string(),
            sections: vec![],
        };

        let redacted = redact(&doc, ClearanceLevel::TopSecret);
        assert_eq!(redacted.id, "doc-empty");
        assert!(redacted.sections.is_empty());
    }

    #[test]
    fn test_exact_redaction_boundary_clearances() {
        let doc = SegmentedDocument {
            id: "doc-boundaries".to_string(),
            sections: vec![
                DocSection {
                    id: "sec-confidential".to_string(),
                    title: "Confidential Section".to_string(),
                    clearance: ClearanceLevel::Confidential,
                    content: "Confidential Content".to_string(),
                },
                DocSection {
                    id: "sec-secret".to_string(),
                    title: "Secret Section".to_string(),
                    clearance: ClearanceLevel::Secret,
                    content: "Secret Content".to_string(),
                },
            ],
        };

        // 1. Request with Confidential clearance (Secret should be redacted, Confidential visible)
        let redacted_conf = redact(&doc, ClearanceLevel::Confidential);
        assert_eq!(redacted_conf.sections[0].content, "Confidential Content");
        assert_eq!(
            redacted_conf.sections[1].content,
            "[REDACTED: Secret Section]"
        );

        // 2. Request with Internal clearance (both redacted)
        let redacted_int = redact(&doc, ClearanceLevel::Internal);
        assert_eq!(
            redacted_int.sections[0].content,
            "[REDACTED: Confidential Section]"
        );
        assert_eq!(
            redacted_int.sections[1].content,
            "[REDACTED: Secret Section]"
        );

        // 3. Request with Secret clearance (both visible)
        let redacted_sec = redact(&doc, ClearanceLevel::Secret);
        assert_eq!(redacted_sec.sections[0].content, "Confidential Content");
        assert_eq!(redacted_sec.sections[1].content, "Secret Content");
    }

    #[test]
    fn test_preserves_section_metadata_and_order() {
        let doc = SegmentedDocument {
            id: "doc-meta".to_string(),
            sections: vec![
                DocSection {
                    id: "sec-a".to_string(),
                    title: "Section A".to_string(),
                    clearance: ClearanceLevel::Secret,
                    content: "Content A".to_string(),
                },
                DocSection {
                    id: "sec-b".to_string(),
                    title: "Section B".to_string(),
                    clearance: ClearanceLevel::Public,
                    content: "Content B".to_string(),
                },
            ],
        };

        let redacted = redact(&doc, ClearanceLevel::Public);

        assert_eq!(redacted.sections[0].id, "sec-a");
        assert_eq!(redacted.sections[0].title, "Section A");
        assert_eq!(redacted.sections[0].clearance, ClearanceLevel::Secret);

        assert_eq!(redacted.sections[1].id, "sec-b");
        assert_eq!(redacted.sections[1].title, "Section B");
        assert_eq!(redacted.sections[1].clearance, ClearanceLevel::Public);
    }
}
