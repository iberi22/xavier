//! Anonymization utility for scrubbing sensitive information from technical data.
//!
//! Provides functions to redact absolute paths, IP addresses, secrets, and other
//! sensitive content before exporting data for training or sharing.

use regex::Regex;
use once_cell::sync::Lazy;
use std::borrow::Cow;

/// Regular expression to match absolute paths (Unix and Windows).
static PATH_REGEX: Lazy<Regex> = Lazy::new(|| {
    // Unix: starts with / followed by path components
    // Windows: starts with C:\ or similar, or \
    Regex::new(r#"(?i)(?:[a-z]:\\|[/\\])(?:[^/\\:\*\?\""<>|\s]+[/\\])+[^/\\:\*\?\""<>|\s]*"#).unwrap()
});

/// Regular expression to match IPv4 and IPv6 addresses.
static IP_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?:\d{1,3}\.){3}\d{1,3}|(?:[a-fA-F0-9]{1,4}:){7}[a-fA-F0-9]{1,4}"#).unwrap()
});

/// Regular expression to match potential secrets (API keys, tokens, etc.).
static SECRET_REGEX: Lazy<Regex> = Lazy::new(|| {
    // Matches common patterns like sk-..., xv1_..., and other hex-like tokens of certain length.
    Regex::new(r#"(?i)(?:sk-[a-zA-Z0-9]{32,}|xv1_[a-zA-Z0-9]{40,}|[a-f0-9]{32,}|[a-f0-9]{64})"#).unwrap()
});

/// Scrub absolute paths from the input string.
pub fn scrub_paths(input: &str) -> Cow<'_, str> {
    PATH_REGEX.replace_all(input, "[PATH]")
}

/// Scrub IP addresses from the input string.
pub fn scrub_ips(input: &str) -> Cow<'_, str> {
    IP_REGEX.replace_all(input, "[IP]")
}

/// Scrub potential secrets from the input string.
pub fn scrub_secrets(input: &str) -> Cow<'_, str> {
    SECRET_REGEX.replace_all(input, "[SECRET]")
}

/// Fully anonymize a string based on the provided configuration.
pub fn anonymize(
    input: &str,
    scrub_p: bool,
    scrub_i: bool,
    scrub_s: bool,
) -> String {
    let mut result = input.to_string();

    // Order matters to avoid nested redactions or partial matches
    if scrub_s {
        result = scrub_secrets(&result).into_owned();
    }

    if scrub_i {
        result = scrub_ips(&result).into_owned();
    }

    if scrub_p {
        result = scrub_paths(&result).into_owned();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrub_paths() {
        let input = "Error at /home/user/project/src/main.rs:10";
        assert_eq!(scrub_paths(input), "Error at [PATH]:10");

        let win_input = r"Failed to open C:\Users\Admin\Documents\secret.txt";
        assert_eq!(scrub_paths(win_input), "Failed to open [PATH]");
    }

    #[test]
    fn test_scrub_ips() {
        let input = "Connection from 192.168.1.100 refused";
        assert_eq!(scrub_ips(input), "Connection from [IP] refused");

        let ipv6 = "Server at 2001:0db8:85a3:0000:0000:8a2e:0370:7334 is up";
        assert_eq!(scrub_ips(ipv6), "Server at [IP] is up");
    }

    #[test]
    fn test_scrub_secrets() {
        let openai_key = "Using key sk-1234567890abcdef1234567890abcdef1234567890abcdef";
        assert_eq!(scrub_secrets(openai_key), "Using key [SECRET]");

        let xavier_token = "xv1_abcdef1234567890abcdef1234567890abcdef1234567890";
        assert_eq!(scrub_secrets(xavier_token), "[SECRET]");
    }

    #[test]
    fn test_anonymize_all() {
        let input = "Log: /var/log/app.log from 10.0.0.1 with sk-1234567890abcdef1234567890abcdef1234567890abcdef";
        let output = anonymize(input, true, true, true);
        assert_eq!(output, "Log: [PATH] from [IP] with [SECRET]");
    }
}
