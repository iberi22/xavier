//! Redaction Engine - PII and Sensitive Data Redaction
//!
//! Provides features to detect and mask PII patterns (emails, phones, SSNs, addresses)
//! with configurable redaction rules.

use crate::security::clearance::ClearanceLevel;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

/// A section within a segmented document with its own clearance level requirement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocSection {
    /// Section ID or index key
    pub id: String,
    /// Title of the section (e.g. "Public Summary" or "Secret Ops")
    pub title: String,
    /// Clearance level required to view this section
    pub clearance_level: ClearanceLevel,
    /// Raw content of the section
    pub content: String,
}

/// A document structured into multiple clearance-level sections.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SegmentedDoc {
    /// Document title
    pub title: String,
    /// List of document sections
    pub sections: Vec<DocSection>,
}

impl SegmentedDoc {
    /// Render document for a requester with a specific clearance level.
    /// Sections requiring higher clearance than `requester_level` will be rendered as `[REDACTED: <title>]`.
    /// Accessible sections will also have PII / sensitive patterns masked with `RedactionEngine::default()`.
    pub fn render_for_clearance(&self, requester_level: ClearanceLevel) -> String {
        let default_engine = RedactionEngine::default();
        self.render_for_clearance_with_engine(requester_level, &default_engine)
    }

    /// Render document for a requester with a specific clearance level and a custom `RedactionEngine`.
    pub fn render_for_clearance_with_engine(
        &self,
        requester_level: ClearanceLevel,
        engine: &RedactionEngine,
    ) -> String {
        let mut rendered = Vec::new();

        if !self.title.is_empty() {
            rendered.push(format!("# {}\n", self.title));
        }

        for section in &self.sections {
            if requester_level >= section.clearance_level {
                let clean_content =
                    engine.redact_nested_markdown(&section.content, requester_level);
                if !section.title.is_empty() {
                    rendered.push(format!("## {}\n{}", section.title, clean_content));
                } else {
                    rendered.push(clean_content);
                }
            } else {
                let section_title = if section.title.is_empty() {
                    "Untitled Section"
                } else {
                    &section.title
                };
                rendered.push(format!("[REDACTED: {}]", section_title));
            }
        }

        rendered.join("\n\n")
    }
}

/// Parse a Markdown string containing clearance markers (e.g., `## [CLEARANCE:3] Title` or `## Title [CLEARANCE:INTERNAL]`)
/// into a structured `SegmentedDoc`.
pub fn parse_segmented(markdown: &str) -> SegmentedDoc {
    let mut doc = SegmentedDoc::default();
    let mut current_section: Option<DocSection> = None;
    let mut title_found = false;

    let clearance_regex = Regex::new(r"(?i)\[CLEARANCE:\s*([A-Z0-9_]+)\]").unwrap();

    for line in markdown.lines() {
        if line.starts_with("# ") && !title_found {
            doc.title = line.trim_start_matches("# ").trim().to_string();
            title_found = true;
            continue;
        }

        if line.starts_with("## ") {
            if let Some(sec) = current_section.take() {
                doc.sections.push(sec);
            }

            let header_text = line.trim_start_matches("## ").trim();
            let (clearance_level, clean_title) =
                if let Some(captures) = clearance_regex.captures(header_text) {
                    let level_str = captures.get(1).unwrap().as_str();
                    let parsed_level = if let Ok(num) = level_str.parse::<u8>() {
                        ClearanceLevel::from(num)
                    } else {
                        ClearanceLevel::from(level_str)
                    };
                    let clean = clearance_regex.replace(header_text, "").trim().to_string();
                    (parsed_level, clean)
                } else {
                    (ClearanceLevel::Unclassified, header_text.to_string())
                };

            let sec_id = format!("sec-{}", doc.sections.len() + 1);
            current_section = Some(DocSection {
                id: sec_id,
                title: clean_title,
                clearance_level,
                content: String::new(),
            });
            continue;
        }

        if let Some(ref mut sec) = current_section {
            if !sec.content.is_empty() {
                sec.content.push('\n');
            }
            sec.content.push_str(line);
        } else if !title_found && doc.title.is_empty() && !line.trim().is_empty() {
            // Content prior to any heading or section
            current_section = Some(DocSection {
                id: "sec-1".to_string(),
                title: "Introduction".to_string(),
                clearance_level: ClearanceLevel::Unclassified,
                content: line.to_string(),
            });
        }
    }

    if let Some(sec) = current_section {
        doc.sections.push(sec);
    }

    doc
}

/// A rule defining a sensitive pattern and its mask replacement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionRule {
    /// Name or type of the rule (e.g., "email", "phone", "ssn", "address")
    pub name: String,
    /// Regex pattern to detect the sensitive data
    pub pattern: String,
    /// Mask replacement string (e.g., "[EMAIL]")
    pub mask: String,
}

/// Engine that performs content redaction based on a list of rules.
#[derive(Debug, Clone)]
pub struct RedactionEngine {
    /// Collection of redaction rules configured in this engine
    pub rules: Vec<RedactionRule>,
}

impl Default for RedactionEngine {
    fn default() -> Self {
        Self {
            rules: vec![
                RedactionRule {
                    name: "email".to_string(),
                    pattern: r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}".to_string(),
                    mask: "[EMAIL]".to_string(),
                },
                RedactionRule {
                    name: "iban".to_string(),
                    pattern: r"\b[a-zA-Z]{2}\d{2}[a-zA-Z0-9]{11,30}\b|\b[a-zA-Z]{2}\d{2}(?:\s?[a-zA-Z0-9]{4}){3,7}(?:\s?[a-zA-Z0-9]{1,4})?\b".to_string(),
                    mask: "[IBAN]".to_string(),
                },
                RedactionRule {
                    name: "ipv6".to_string(),
                    pattern: r"\b(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}\b|\b(?:[0-9a-fA-F]{1,4}:){1,7}:|:(?::[0-9a-fA-F]{1,4}){1,7}\b|\b(?:[0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}\b".to_string(),
                    mask: "[IPV6]".to_string(),
                },
                RedactionRule {
                    name: "ipv4".to_string(),
                    pattern: r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b".to_string(),
                    mask: "[IPV4]".to_string(),
                },
                RedactionRule {
                    name: "gps".to_string(),
                    pattern: r"[-+]?\b(?:90(?:\.0+)?|[1-8]?\d(?:\.\d+)?),\s*[-+]?(?:180(?:\.0+)?|1[0-7]\d(?:\.\d+)?|[1-9]?\d(?:\.\d+)?)\b".to_string(),
                    mask: "[GPS]".to_string(),
                },
                RedactionRule {
                    name: "cedula".to_string(),
                    pattern: r"\b(?:C\.?C\.?|Cédula|cedula)\s*[:#]?\s*\d{6,10}\b|\b\d{6,10}\b".to_string(),
                    mask: "[CEDULA]".to_string(),
                },
                RedactionRule {
                    name: "phone".to_string(),
                    pattern: r"(?:\+?\d{1,3}[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}".to_string(),
                    mask: "[PHONE]".to_string(),
                },
                RedactionRule {
                    name: "ssn".to_string(),
                    pattern: r"\b\d{3}-\d{2}-\d{4}\b".to_string(),
                    mask: "[SSN]".to_string(),
                },
                RedactionRule {
                    name: "address".to_string(),
                    pattern: r"\b\d{1,5}\s+[A-Za-z\s#\-]{2,30}\s+(?:Street|St|Avenue|Ave|Road|Rd|Boulevard|Blvd|Drive|Dr|Lane|Ln|Court|Ct|Circle|Cir|Way|Apt|Suite|Plaza|Pl)\b".to_string(),
                    mask: "[ADDRESS]".to_string(),
                },
            ],
        }
    }
}

impl RedactionEngine {
    /// Creates a new `RedactionEngine` with a custom set of rules.
    pub fn new(rules: Vec<RedactionRule>) -> Self {
        Self { rules }
    }

    /// Adds a rule to the redaction engine.
    pub fn add_rule(&mut self, rule: RedactionRule) {
        self.rules.push(rule);
    }

    /// Redacts all sensitive patterns from the input text using the current rules.
    pub fn redact(&self, input: &str) -> String {
        let mut redacted = input.to_string();
        for rule in &self.rules {
            if let Ok(re) = RegexBuilder::new(&rule.pattern)
                .case_insensitive(true)
                .build()
            {
                redacted = re.replace_all(&redacted, &rule.mask).to_string();
            }
        }
        redacted
    }

    /// Redacts nested segmented markdown blocks (`:::redact[level]` ... `:::`) based on `requester_level`
    /// and applies PII redaction rules to accessible content.
    pub fn redact_nested_markdown(&self, input: &str, requester_level: ClearanceLevel) -> String {
        let nodes = parse_nested_doc(input);
        render_nodes(&nodes, requester_level, self)
    }
}

/// A node in a parsed nested markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownNode {
    /// Raw markdown text line(s)
    Text(String),
    /// A clearance-gated container block defined by `:::redact[level]` ... `:::`
    RedactBlock {
        /// Raw level string specified in tag (e.g. "CONFIDENTIAL" or "3")
        level_str: String,
        /// Parsed clearance level required to view this block
        clearance_level: ClearanceLevel,
        /// Inner nodes nested within this block
        children: Vec<MarkdownNode>,
    },
}

/// Redact nested segmented markdown text (`:::redact[level]`) for a given requester clearance level using default redaction rules.
pub fn redact_nested_markdown(input: &str, requester_level: ClearanceLevel) -> String {
    let engine = RedactionEngine::default();
    engine.redact_nested_markdown(input, requester_level)
}

/// Parse nested markdown string containing `:::redact[level]` blocks into a list of `MarkdownNode`s.
pub fn parse_nested_doc(markdown: &str) -> Vec<MarkdownNode> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut all_nodes = Vec::new();
    let mut idx = 0;
    while idx < lines.len() {
        let (mut nodes, next_idx) = parse_markdown_nodes(&lines, idx);
        all_nodes.append(&mut nodes);
        if next_idx == idx {
            idx += 1;
        } else {
            idx = next_idx;
        }
    }
    all_nodes
}

fn parse_markdown_nodes(lines: &[&str], start_idx: usize) -> (Vec<MarkdownNode>, usize) {
    let mut nodes = Vec::new();
    let mut current_text = Vec::new();
    let mut i = start_idx;

    let open_re = Regex::new(r"(?i)^\s*:::\s*redact\s*\[\s*([^\]]+)\s*\]\s*$").unwrap();
    let close_re = Regex::new(r"^\s*:::\s*$").unwrap();

    while i < lines.len() {
        let line = lines[i];

        if let Some(caps) = open_re.captures(line) {
            if !current_text.is_empty() {
                nodes.push(MarkdownNode::Text(current_text.join("\n")));
                current_text.clear();
            }

            let level_str = caps.get(1).unwrap().as_str().trim().to_string();
            let clearance_level = if let Ok(num) = level_str.parse::<u8>() {
                ClearanceLevel::from(num)
            } else {
                ClearanceLevel::from(level_str.as_str())
            };

            let (children, next_idx) = parse_markdown_nodes(lines, i + 1);
            nodes.push(MarkdownNode::RedactBlock {
                level_str,
                clearance_level,
                children,
            });
            i = next_idx;
        } else if close_re.is_match(line) {
            if !current_text.is_empty() {
                nodes.push(MarkdownNode::Text(current_text.join("\n")));
                current_text.clear();
            }
            return (nodes, i + 1);
        } else {
            current_text.push(line);
            i += 1;
        }
    }

    if !current_text.is_empty() {
        nodes.push(MarkdownNode::Text(current_text.join("\n")));
    }

    (nodes, i)
}

fn render_nodes(
    nodes: &[MarkdownNode],
    requester_level: ClearanceLevel,
    engine: &RedactionEngine,
) -> String {
    let rendered_parts: Vec<String> = nodes
        .iter()
        .map(|node| render_single_node(node, requester_level, engine))
        .collect();
    rendered_parts.join("\n")
}

fn render_single_node(
    node: &MarkdownNode,
    requester_level: ClearanceLevel,
    engine: &RedactionEngine,
) -> String {
    match node {
        MarkdownNode::Text(text) => engine.redact(text),
        MarkdownNode::RedactBlock {
            level_str,
            clearance_level,
            children,
        } => {
            if requester_level >= *clearance_level {
                render_nodes(children, requester_level, engine)
            } else {
                format!("[REDACTED: {}]", level_str)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_emails() {
        let engine = RedactionEngine::default();
        let input = "Contact me at john.doe@example.com or support@company.org.";
        let redacted = engine.redact(input);
        assert_eq!(redacted, "Contact me at [EMAIL] or [EMAIL].");
    }

    #[test]
    fn test_redact_phones() {
        let engine = RedactionEngine::default();
        let inputs = vec![
            ("Call me at 123-456-7890.", "Call me at [PHONE]."),
            ("My number is +1-555-123-4567.", "My number is [PHONE]."),
            ("Reach us at (555) 123-4567.", "Reach us at [PHONE]."),
        ];
        for (input, expected) in inputs {
            assert_eq!(engine.redact(input), expected);
        }
    }

    #[test]
    fn test_redact_ssn() {
        let engine = RedactionEngine::default();
        let input = "My SSN is ***-**-**** or 123-45-6789.";
        let redacted = engine.redact(input);
        assert_eq!(redacted, "My SSN is ***-**-**** or [SSN].");
    }

    #[test]
    fn test_redact_address() {
        let engine = RedactionEngine::default();
        let inputs = vec![
            ("I live at 123 Main St.", "I live at [ADDRESS]."),
            (
                "Meet at 1600 Amphitheatre Pkwy.",
                "Meet at 1600 Amphitheatre Pkwy.",
            ), // no standard street suffix
            ("Send mail to 456 Oak Avenue.", "Send mail to [ADDRESS]."),
        ];
        for (input, expected) in inputs {
            assert_eq!(engine.redact(input), expected);
        }
    }

    #[test]
    fn test_custom_rules() {
        let mut engine = RedactionEngine::new(vec![]);
        engine.add_rule(RedactionRule {
            name: "secret_id".to_string(),
            pattern: r"SEC-\d{4}".to_string(),
            mask: "[SECRET]".to_string(),
        });
        let input = "Code: SEC-1234 and SEC-5678.";
        let redacted = engine.redact(input);
        assert_eq!(redacted, "Code: [SECRET] and [SECRET].");
    }

    #[test]
    fn test_redact_cedula() {
        let engine = RedactionEngine::default();
        let inputs = vec![
            ("Mi Cédula #1012345678 es esta.", "Mi [CEDULA] es esta."),
            ("CC 523456789 es el documento.", "[CEDULA] es el documento."),
            ("cedula: 123456", "[CEDULA]"),
            ("El codigo es 123.", "El codigo es 123."), // Too short (< 6 digits) -> no match
        ];
        for (input, expected) in inputs {
            assert_eq!(engine.redact(input), expected);
        }
    }

    #[test]
    fn test_redact_ipv4() {
        let engine = RedactionEngine::default();
        let inputs = vec![
            (
                "Server IP is 192.168.1.1 or 10.0.0.254.",
                "Server IP is [IPV4] or [IPV4].",
            ),
            (
                "Invalid IP 999.999.999.999 should stay.",
                "Invalid IP 999.999.999.999 should stay.",
            ),
        ];
        for (input, expected) in inputs {
            assert_eq!(engine.redact(input), expected);
        }
    }

    #[test]
    fn test_redact_ipv6() {
        let engine = RedactionEngine::default();
        let inputs = vec![
            (
                "Address is 2001:0db8:85a3:0000:0000:8a2e:0370:7334.",
                "Address is [IPV6].",
            ),
            ("Loopback is ::1.", "Loopback is [IPV6]."),
        ];
        for (input, expected) in inputs {
            assert_eq!(engine.redact(input), expected);
        }
    }

    #[test]
    fn test_redact_gps() {
        let engine = RedactionEngine::default();
        let inputs = vec![
            ("Coordinates: 4.60971, -74.08175.", "Coordinates: [GPS]."),
            ("Location -12.04637, 77.04279 ok", "Location [GPS] ok"),
            ("Not GPS 999.999, 123.45", "Not GPS 999.999, 123.45"),
        ];
        for (input, expected) in inputs {
            assert_eq!(engine.redact(input), expected);
        }
    }

    #[test]
    fn test_redact_iban() {
        let engine = RedactionEngine::default();
        let inputs = vec![
            ("Transfer to GB33BUKB20201555555555.", "Transfer to [IBAN]."),
            ("IBAN ES91 2100 0418 4502 0005 1332", "IBAN [IBAN]"),
            ("Short text US12", "Short text US12"),
        ];
        for (input, expected) in inputs {
            assert_eq!(engine.redact(input), expected);
        }
    }

    #[test]
    fn test_segmented_doc_render_for_clearance() {
        let doc = SegmentedDoc {
            title: "Project Alpha Plan".to_string(),
            sections: vec![
                DocSection {
                    id: "sec-1".to_string(),
                    title: "Public Overview".to_string(),
                    clearance_level: ClearanceLevel::Internal, // Level 1
                    content:
                        "This project aims to optimize response times. Contact boss@company.org."
                            .to_string(),
                },
                DocSection {
                    id: "sec-2".to_string(),
                    title: "Secret Infrastructure".to_string(),
                    clearance_level: ClearanceLevel::Confidential, // Level 3
                    content: "Database credentials are stored in Vault cluster alpha-9."
                        .to_string(),
                },
            ],
        };

        // Level 1 requester: Section 2 must be redacted
        let rendered_lvl1 = doc.render_for_clearance(ClearanceLevel::Internal);
        assert!(rendered_lvl1.contains("## Public Overview"));
        assert!(rendered_lvl1.contains("[EMAIL]")); // PII redacted
        assert!(rendered_lvl1.contains("[REDACTED: Secret Infrastructure]"));
        assert!(!rendered_lvl1.contains("Vault cluster alpha-9"));

        // Level 3 requester: Section 2 must be visible
        let rendered_lvl3 = doc.render_for_clearance(ClearanceLevel::Confidential);
        assert!(rendered_lvl3.contains("## Public Overview"));
        assert!(rendered_lvl3.contains("## Secret Infrastructure"));
        assert!(rendered_lvl3.contains("Vault cluster alpha-9"));
        assert!(!rendered_lvl3.contains("[REDACTED: Secret Infrastructure]"));
    }

    #[test]
    fn test_parse_segmented_markdown() {
        let markdown = r#"# Classified Mission Document

## [CLEARANCE:1] Public Briefing
Welcome team. Contact admin@swal.io for info.

## Strategic Targets [CLEARANCE:3]
Target sector 7 contains sensitive assets.

## [CLEARANCE:TOPSECRET] Vault Key
The nuclear launch codes are 000000.
"#;

        let parsed = parse_segmented(markdown);
        assert_eq!(parsed.title, "Classified Mission Document");
        assert_eq!(parsed.sections.len(), 3);

        assert_eq!(parsed.sections[0].title, "Public Briefing");
        assert_eq!(parsed.sections[0].clearance_level, ClearanceLevel::Internal);

        assert_eq!(parsed.sections[1].title, "Strategic Targets");
        assert_eq!(
            parsed.sections[1].clearance_level,
            ClearanceLevel::Confidential
        );

        assert_eq!(parsed.sections[2].title, "Vault Key");
        assert_eq!(
            parsed.sections[2].clearance_level,
            ClearanceLevel::TopSecret
        );
    }

    #[test]
    fn test_nested_segmented_markdown_redaction() {
        let input = r#"# Main Title

Public intro line. Contact help@example.com.

:::redact[INTERNAL]
Internal notes line.
:::redact[CONFIDENTIAL]
Confidential ops details. Call 123-456-7890.
:::redact[TOPSECRET]
Ultra secret nuclear codes 0000.
:::
:::
:::

Ending public note."#;

        // Requester Clearance: Unclassified (0)
        let redacted_unclass = redact_nested_markdown(input, ClearanceLevel::Unclassified);
        assert!(redacted_unclass.contains("Public intro line. Contact [EMAIL]."));
        assert!(redacted_unclass.contains("[REDACTED: INTERNAL]"));
        assert!(!redacted_unclass.contains("Internal notes line."));
        assert!(!redacted_unclass.contains("Confidential ops details."));
        assert!(!redacted_unclass.contains("Ultra secret nuclear codes"));
        assert!(redacted_unclass.contains("Ending public note."));

        // Requester Clearance: Internal (1)
        let redacted_internal = redact_nested_markdown(input, ClearanceLevel::Internal);
        assert!(redacted_internal.contains("Internal notes line."));
        assert!(redacted_internal.contains("[REDACTED: CONFIDENTIAL]"));
        assert!(!redacted_internal.contains("Confidential ops details."));
        assert!(!redacted_internal.contains("Ultra secret nuclear codes"));

        // Requester Clearance: Confidential (3)
        let redacted_confidential = redact_nested_markdown(input, ClearanceLevel::Confidential);
        assert!(redacted_confidential.contains("Internal notes line."));
        assert!(redacted_confidential.contains("Confidential ops details. Call [PHONE]."));
        assert!(redacted_confidential.contains("[REDACTED: TOPSECRET]"));
        assert!(!redacted_confidential.contains("Ultra secret nuclear codes"));

        // Requester Clearance: TopSecret (5)
        let redacted_topsecret = redact_nested_markdown(input, ClearanceLevel::TopSecret);
        assert!(redacted_topsecret.contains("Internal notes line."));
        assert!(redacted_topsecret.contains("Confidential ops details. Call [PHONE]."));
        assert!(redacted_topsecret.contains("Ultra secret nuclear codes 0000."));
        assert!(!redacted_topsecret.contains("[REDACTED:"));
    }

    #[test]
    fn test_nested_segmented_markdown_sibling_blocks() {
        let input = r#":::redact[1]
Level 1 content with email user@domain.com
:::

:::redact[4]
Level 4 secret info
:::"#;

        let red_lvl0 = redact_nested_markdown(input, ClearanceLevel::Unclassified);
        assert!(red_lvl0.contains("[REDACTED: 1]"));
        assert!(red_lvl0.contains("[REDACTED: 4]"));

        let red_lvl2 = redact_nested_markdown(input, ClearanceLevel::Restricted);
        assert!(red_lvl2.contains("Level 1 content with email [EMAIL]"));
        assert!(red_lvl2.contains("[REDACTED: 4]"));

        let red_lvl5 = redact_nested_markdown(input, ClearanceLevel::TopSecret);
        assert!(red_lvl5.contains("Level 1 content with email [EMAIL]"));
        assert!(red_lvl5.contains("Level 4 secret info"));
    }
}
