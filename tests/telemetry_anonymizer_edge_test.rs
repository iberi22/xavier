use xavier::agents::telemetry::anonymizer::{
    AnonymizerConfig, RedactionStrategy, TelemetryAnonymizer,
};

#[test]
fn test_base64_encoded_secrets_redaction() {
    let anonymizer = TelemetryAnonymizer::default();

    // Base64 containing raw API key sk-1234567890abcdef12345678 -> "c2stMTIzNDU2Nzg5MGFiY2RlZjEyMzQ1Njc4"
    let raw_key = "sk-1234567890abcdef12345678";
    let base64_payload = "Bearer sk-1234567890abcdef12345678== or authorization: Basic c2stMTIzNDU2Nzg5MGFiY2RlZjEyMzQ1Njc4";

    assert!(anonymizer.is_sensitive(base64_payload));
    let scrubbed = anonymizer.anonymize(base64_payload);
    assert!(!scrubbed.contains(raw_key));
    assert!(scrubbed.contains("[API_KEY:"));
}

#[test]
fn test_malformed_and_broken_ipv6_patterns() {
    let anonymizer = TelemetryAnonymizer::default();

    // Valid IPv6 addresses should be redacted
    let valid_ipv6_1 = "2001:0db8:85a3:0000:0000:8a2e:0370:7334";
    let valid_ipv6_2 = "fe80::1ff:fe23:4567:890a";
    let valid_ipv6_3 = "::1";

    let valid_input = format!(
        "IPs: {} and {} and {}",
        valid_ipv6_1, valid_ipv6_2, valid_ipv6_3
    );
    let valid_scrubbed = anonymizer.anonymize(&valid_input);

    assert!(!valid_scrubbed.contains(valid_ipv6_1));
    assert!(!valid_scrubbed.contains(valid_ipv6_2));
    assert!(!valid_scrubbed.contains(valid_ipv6_3));
    assert!(valid_scrubbed.contains("[IPV6:"));

    // Completely invalid IPv6 string without colons or hex
    let non_ipv6 = "invalid_ipv6_address_without_colons";
    assert!(!anonymizer.is_sensitive(non_ipv6));
    assert_eq!(anonymizer.anonymize(non_ipv6), non_ipv6);

    // Malformed IPv6 strings with extra colons
    let broken_ipv6 = "2001:db8:::1";
    let scrubbed = anonymizer.anonymize(broken_ipv6);
    // The prefix match scrubbed 2001:db8:: while leaving the trailing :1
    assert!(scrubbed.contains("[IPV6:"));
}

#[test]
fn test_malformed_and_broken_email_patterns() {
    let anonymizer = TelemetryAnonymizer::default();

    // Valid emails
    let valid_email = "alice.smith+dev@swal-platform.co.uk";
    assert!(anonymizer.is_sensitive(valid_email));
    let scrubbed = anonymizer.anonymize(&format!("User: {}", valid_email));
    assert!(!scrubbed.contains(valid_email));
    assert!(scrubbed.contains("[EMAIL:"));

    // Malformed email patterns
    let malformed_emails = vec![
        "missing-at-sign.com",
        "@domain.com",
        "user@.com",
        "user@domain.",
    ];

    for bad_email in malformed_emails {
        assert!(
            !anonymizer.is_sensitive(bad_email),
            "Expected bad email '{}' to not be classified as sensitive",
            bad_email
        );
        let scrubbed = anonymizer.anonymize(bad_email);
        assert_eq!(scrubbed, bad_email);
    }
}

#[test]
fn test_partial_and_malformed_ipv4_addresses() {
    let anonymizer = TelemetryAnonymizer::default();

    // Valid IPv4
    let valid_ip = "10.0.0.1";
    assert!(anonymizer.is_sensitive(valid_ip));
    let scrubbed_valid = anonymizer.anonymize(&format!("Host {}", valid_ip));
    assert!(!scrubbed_valid.contains(valid_ip));
    assert!(scrubbed_valid.contains("[IPV4:"));

    // Partial / Out of range IPv4
    let malformed_ips = vec![
        "256.1.1.1",     // Out of byte range
        "192.168.1",     // Partial 3 octets
        "10.0.0",        // Partial
        "192.168.1.300", // Out of byte range
    ];

    for bad_ip in malformed_ips {
        let scrubbed = anonymizer.anonymize(bad_ip);
        assert!(!scrubbed.contains("[IPV4:"));
    }
}

#[test]
fn test_malformed_and_broken_jwts() {
    let anonymizer = TelemetryAnonymizer::default();

    // Valid JWT
    let valid_jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    assert!(anonymizer.is_sensitive(valid_jwt));

    // Malformed JWTs
    let missing_signature = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ";
    let single_segment = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
    let short_segment = "eyJhbGci.eyJzdWIi.SflK";

    assert!(!anonymizer.is_sensitive(missing_signature));
    assert!(!anonymizer.is_sensitive(single_segment));
    assert!(!anonymizer.is_sensitive(short_segment));

    assert_eq!(anonymizer.anonymize(missing_signature), missing_signature);
    assert_eq!(anonymizer.anonymize(single_segment), single_segment);
    assert_eq!(anonymizer.anonymize(short_segment), short_segment);
}

#[test]
fn test_nested_json_credentials_scrubbing() {
    let anonymizer = TelemetryAnonymizer::default();

    let nested_json = r#"{
        "service": "billing",
        "config": {
            "api_key": "sk-abcdef1234567890abcdef123456",
            "github_token": "ghp_1234567890abcdef1234567890abcdef1234",
            "gitlab_token": "glpat-1234567890abcdef1234",
            "slack": "xoxb-1234567890123",
            "aws": "AKIAIOSFODNN7EXAMPLE",
            "nested_array": [
                {"email": "admin@swal.dev", "ip": "172.16.254.1"}
            ]
        }
    }"#;

    assert!(anonymizer.is_sensitive(nested_json));

    let scrubbed = anonymizer.anonymize(nested_json);

    assert!(!scrubbed.contains("sk-abcdef1234567890abcdef123456"));
    assert!(!scrubbed.contains("ghp_1234567890abcdef1234567890abcdef1234"));
    assert!(!scrubbed.contains("glpat-1234567890abcdef1234"));
    assert!(!scrubbed.contains("xoxb-1234567890123"));
    assert!(!scrubbed.contains("AKIAIOSFODNN7EXAMPLE"));
    assert!(!scrubbed.contains("admin@swal.dev"));
    assert!(!scrubbed.contains("172.16.254.1"));

    assert!(scrubbed.contains("[API_KEY:"));
    assert!(scrubbed.contains("[GITHUB_PAT:"));
    assert!(scrubbed.contains("[GITLAB_PAT:"));
    assert!(scrubbed.contains("[SLACK_TOKEN:"));
    assert!(scrubbed.contains("[AWS_ACCESS_KEY:"));
    assert!(scrubbed.contains("[EMAIL:"));
    assert!(scrubbed.contains("[IPV4:"));
}

#[test]
fn test_all_pattern_branches_coverage() {
    let config = AnonymizerConfig {
        strategy: RedactionStrategy::Tag,
        enable_dp: true,
        dp_epsilon: 1.0,
    };
    let anonymizer = TelemetryAnonymizer::new(config);

    // Each pattern branch tested explicitly
    let cases = vec![
        ("sk-1234567890abcdef12345678", "[REDACTED:API_KEY]"),
        ("ghp_1234567890abcdef1234567890abcdef1234", "[REDACTED:GITHUB_PAT]"),
        ("glpat-1234567890abcdef1234", "[REDACTED:GITLAB_PAT]"),
        ("xoxb-1234567890123", "[REDACTED:SLACK_TOKEN]"),
        ("AKIAIOSFODNN7EXAMPLE", "[REDACTED:AWS_ACCESS_KEY]"),
        (
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
            "[REDACTED:JWT]",
        ),
        ("test.user@domain.com", "[REDACTED:EMAIL]"),
        ("192.168.0.1", "[REDACTED:IPV4]"),
        ("2001:0db8:85a3:0000:0000:8a2e:0370:7334", "[REDACTED:IPV6]"),
    ];

    for (input, expected) in cases {
        assert!(anonymizer.is_sensitive(input));
        assert_eq!(anonymizer.anonymize(input), expected);
    }
}

#[test]
fn test_disabled_or_zero_differential_privacy() {
    let disabled_dp_config = AnonymizerConfig {
        strategy: RedactionStrategy::Tag,
        enable_dp: false,
        dp_epsilon: 1.0,
    };
    let anonymizer_disabled = TelemetryAnonymizer::new(disabled_dp_config);
    assert_eq!(
        anonymizer_disabled.add_differential_privacy_noise(42.0, 1.0),
        42.0
    );

    let zero_epsilon_config = AnonymizerConfig {
        strategy: RedactionStrategy::Tag,
        enable_dp: true,
        dp_epsilon: 0.0,
    };
    let anonymizer_zero = TelemetryAnonymizer::new(zero_epsilon_config);
    assert_eq!(
        anonymizer_zero.add_differential_privacy_noise(42.0, 1.0),
        42.0
    );

    let negative_epsilon_config = AnonymizerConfig {
        strategy: RedactionStrategy::Tag,
        enable_dp: true,
        dp_epsilon: -0.5,
    };
    let anonymizer_neg = TelemetryAnonymizer::new(negative_epsilon_config);
    assert_eq!(
        anonymizer_neg.add_differential_privacy_noise(42.0, 1.0),
        42.0
    );
}

#[test]
fn test_traits_debug_clone_default() {
    let default_anonymizer = TelemetryAnonymizer::default();
    let debug_str = format!("{:?}", default_anonymizer);
    assert!(debug_str.contains("TelemetryAnonymizer"));

    let cloned_anonymizer = default_anonymizer.clone();
    assert_eq!(
        cloned_anonymizer.anonymize("sk-1234567890abcdef12345678"),
        default_anonymizer.anonymize("sk-1234567890abcdef12345678")
    );

    let config = AnonymizerConfig::default();
    let config_debug = format!("{:?}", config);
    assert!(config_debug.contains("AnonymizerConfig"));
    let cloned_config = config.clone();
    assert_eq!(cloned_config.strategy, RedactionStrategy::TruncatedHash);
    assert!(cloned_config.enable_dp);
    assert_eq!(cloned_config.dp_epsilon, 1.0);
}
