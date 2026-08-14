//! Redaction Engine - PII and Sensitive Data Redaction
//!
//! Provides features to detect and mask PII patterns (emails, phones, SSNs, addresses)
//! with configurable redaction rules.

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

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
}
