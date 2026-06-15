//! Data Commons Data Sanitizer
//!
//! Provides rule-based sanitization of arbitrary serializable payloads.
//! Each rule consists of a regex field pattern and an action (redact, hash,
//! mask, or drop). Default rules are provided for common PII fields such
//! as emails, passwords, IP addresses, credit cards, and phone numbers.
//!
//! # Example
//!
//! ```ignore
//! let sanitizer = DataSanitizer::new();
//! sanitizer.add_default_rules();
//!
//! let clean = sanitizer.sanitize(&payload).unwrap();
//! ```

use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use hex;

// ---------------------------------------------------------------------------
// SanitizationAction
// ---------------------------------------------------------------------------

/// The action to take when a field matches a sanitization rule.
#[derive(Debug, Clone)]
pub enum SanitizationAction {
    /// Replace the field value with a static replacement string.
    Redact { replacement: String },
    /// Replace the field value with its SHA-256 hash.
    Hash,
    /// Keep only the first N characters visible; replace the rest with asterisks.
    Mask { prefix_visible: usize },
    /// Remove the field entirely from the payload.
    Drop,
}

// ---------------------------------------------------------------------------
// SanitizationRule
// ---------------------------------------------------------------------------

/// A single sanitization rule: a regex pattern matching field names, and the
/// action to apply to matching fields.
#[derive(Debug, Clone)]
pub struct SanitizationRule {
    /// Regex pattern to match against field names.
    pub field_pattern: String,
    /// The action to apply to matching fields.
    pub action: SanitizationAction,
}

// ---------------------------------------------------------------------------
// DataSanitizer
// ---------------------------------------------------------------------------

/// Applies rule-based sanitization to serializable payloads.
pub struct DataSanitizer {
    /// Ordered list of sanitization rules. Rules are applied in order; the
    /// first matching rule wins.
    rules: Vec<SanitizationRule>,
}

impl DataSanitizer {
    /// Create a new `DataSanitizer` with no rules.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a sanitization rule.
    pub fn add_rule(&mut self, field_pattern: &str, action: SanitizationAction) {
        self.rules.push(SanitizationRule {
            field_pattern: field_pattern.to_string(),
            action,
        });
    }

    /// Sanitize a serializable value by applying all matching rules to every
    /// field in the JSON tree.
    pub fn sanitize<T: Serialize>(&self, value: &T) -> Result<Value, serde_json::Error> {
        let json = serde_json::to_value(value)?;
        Ok(self.sanitize_value(&json))
    }

    /// Add default rules for common PII fields.
    ///
    /// The following rules are added (in order):
    /// - `email` → Mask(3) — shows first 3 chars, e.g. "joh***"
    /// - `password` → Redact("***")
    /// - `ip_address` → Mask(3)
    /// - `credit_card` → Drop (remove entirely)
    /// - `phone` → Mask(4)
    pub fn add_default_rules(&mut self) {
        self.rules.push(SanitizationRule {
            field_pattern: "email".to_string(),
            action: SanitizationAction::Mask { prefix_visible: 3 },
        });
        self.rules.push(SanitizationRule {
            field_pattern: "password".to_string(),
            action: SanitizationAction::Redact {
                replacement: "***".to_string(),
            },
        });
        self.rules.push(SanitizationRule {
            field_pattern: "ip_address".to_string(),
            action: SanitizationAction::Mask { prefix_visible: 3 },
        });
        self.rules.push(SanitizationRule {
            field_pattern: "credit_card".to_string(),
            action: SanitizationAction::Drop,
        });
        self.rules.push(SanitizationRule {
            field_pattern: "phone".to_string(),
            action: SanitizationAction::Mask { prefix_visible: 4 },
        });
    }

    /// Apply sanitization rules recursively to a JSON value.
    fn sanitize_value(&self, value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut cleaned = Map::new();
                for (key, val) in map {
                    let action = self.match_field(key);
                    match action {
                        Some(SanitizationAction::Drop) => {
                            // Skip this field entirely
                        }
                        Some(SanitizationAction::Redact { replacement }) => {
                            cleaned.insert(key.clone(), Value::String(replacement));
                        }
                        Some(SanitizationAction::Hash) => {
                            let hashed = self.hash_value(val);
                            cleaned.insert(key.clone(), Value::String(hashed));
                        }
                        Some(SanitizationAction::Mask { prefix_visible }) => {
                            let masked = match val {
                                Value::String(s) => mask_string(s, prefix_visible),
                                _ => Value::String(format!("[MASKED:{}]", key)),
                            };
                            cleaned.insert(key.clone(), masked);
                        }
                        None => {
                            // No rule matched — recurse into nested objects
                            cleaned.insert(key.clone(), self.sanitize_value(val));
                        }
                    }
                }
                Value::Object(cleaned)
            }
            Value::Array(arr) => {
                Value::Array(arr.iter().map(|v| self.sanitize_value(v)).collect())
            }
            other => other.clone(),
        }
    }

    /// Check if a field name matches any rule. Returns the action for the
    /// first matching rule, or `None`.
    fn match_field(&self, field_name: &str) -> Option<SanitizationAction> {
        for rule in &self.rules {
            if let Ok(re) = Regex::new(&rule.field_pattern) {
                if re.is_match(field_name) {
                    return Some(rule.action.clone());
                }
            }
        }
        None
    }

    /// Hash a JSON value to a hex string.
    fn hash_value(&self, value: &Value) -> String {
        let input = match value {
            Value::String(s) => s.clone(),
            _ => value.to_string(),
        };
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        let result = hasher.finalize();
        hex::encode(result)
    }

    /// Returns the list of rules currently registered.
    pub fn rules(&self) -> &[SanitizationRule] {
        &self.rules
    }
}

impl Default for DataSanitizer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Mask a string by keeping only the first `prefix_visible` characters visible
/// and replacing the rest with asterisks.
fn mask_string(s: &str, prefix_visible: usize) -> Value {
    if s.len() <= prefix_visible {
        return Value::String(s.to_string());
    }
    let visible: String = s.chars().take(prefix_visible).collect();
    let masked = format!("{}***", visible);
    Value::String(masked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use serde_json::json;

    #[derive(Serialize)]
    struct UserProfile {
        email: String,
        password: String,
        ip_address: String,
        phone: String,
        credit_card: String,
        username: String,
    }

    #[test]
    fn test_default_rules_mask_email() {
        let mut sanitizer = DataSanitizer::new();
        sanitizer.add_default_rules();

        let profile = UserProfile {
            email: "john.doe@example.com".to_string(),
            password: "s3cret!".to_string(),
            ip_address: "192.168.1.5".to_string(),
            phone: "+1-555-123-4567".to_string(),
            credit_card: "4111-1111-1111-1111".to_string(),
            username: "johndoe".to_string(),
        };

        let result = sanitizer.sanitize(&profile).unwrap();
        let obj = result.as_object().unwrap();

        // Email: first 3 chars visible
        assert_eq!(obj["email"], "joh***");
        // Password: redacted
        assert_eq!(obj["password"], "***");
        // IP: first 3 chars visible
        assert_eq!(obj["ip_address"], "192***");
        // Phone: first 4 chars visible
        assert_eq!(obj["phone"], "+1-5***");
        // Credit card: dropped entirely
        assert!(!obj.contains_key("credit_card"));
        // Username: no matching rule, passes through
        assert_eq!(obj["username"], "johndoe");
    }

    #[test]
    fn test_drop_removes_field() {
        let mut sanitizer = DataSanitizer::new();
        sanitizer.add_rule("secret", SanitizationAction::Drop);

        let payload = json!({
            "name": "public",
            "secret": "hidden",
            "nested": { "secret": "also-hidden" }
        });

        let result = sanitizer.sanitize(&payload).unwrap();
        assert!(!result.as_object().unwrap().contains_key("secret"));
        assert!(!result["nested"].as_object().unwrap().contains_key("secret"));
        assert_eq!(result["name"], "public");
    }

    #[test]
    fn test_hash_action() {
        let mut sanitizer = DataSanitizer::new();
        sanitizer.add_rule("token", SanitizationAction::Hash);

        let payload = json!({ "token": "my-secret-token", "label": "visible" });
        let result = sanitizer.sanitize(&payload).unwrap();
        let hashed = result["token"].as_str().unwrap().to_string();
        // Should be a hex string (SHA-256 = 64 hex chars)
        assert_eq!(hashed.len(), 64);
        assert!(hashed.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(result["label"], "visible");
    }

    #[test]
    fn test_redact_action() {
        let mut sanitizer = DataSanitizer::new();
        sanitizer.add_rule(
            "api_key",
            SanitizationAction::Redact {
                replacement: "[REDACTED]".to_string(),
            },
        );

        let payload = json!({ "api_key": "sk-123456", "name": "public" });
        let result = sanitizer.sanitize(&payload).unwrap();
        assert_eq!(result["api_key"], "[REDACTED]");
    }

    #[test]
    fn test_no_rules_passthrough() {
        let sanitizer = DataSanitizer::new();
        let payload = json!({ "email": "test@example.com" });
        let result = sanitizer.sanitize(&payload).unwrap();
        assert_eq!(result["email"], "test@example.com");
    }

    #[test]
    fn test_regex_field_pattern() {
        let mut sanitizer = DataSanitizer::new();
        // Rule matching any field containing "key"
        sanitizer.add_rule("key", SanitizationAction::Redact {
            replacement: "[REDACTED]".to_string(),
        });

        let payload = json!({
            "api_key": "abc123",
            "access_key": "def456",
            "username": "john"
        });

        let result = sanitizer.sanitize(&payload).unwrap();
        assert_eq!(result["api_key"], "[REDACTED]");
        assert_eq!(result["access_key"], "[REDACTED]");
        assert_eq!(result["username"], "john");
    }
}
