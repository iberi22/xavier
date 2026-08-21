use xavier::agents::telemetry::anonymizer::{
    AnonymizerConfig, RedactionStrategy, TelemetryAnonymizer,
};

#[test]
fn test_api_keys_and_tokens_redaction() {
    let anonymizer = TelemetryAnonymizer::default();

    let input = "Credentials: sk-1234567890abcdef12345678 and ghp_1234567890abcdef1234567890abcdef1234 and glpat-abcdef1234567890abcdef and xoxb-1234567890123 and AKIAIOSFODNN7EXAMPLE";
    let anonymized = anonymizer.anonymize(input);

    assert!(!anonymized.contains("sk-1234567890abcdef12345678"));
    assert!(!anonymized.contains("ghp_1234567890abcdef1234567890abcdef1234"));
    assert!(!anonymized.contains("glpat-abcdef1234567890abcdef"));
    assert!(!anonymized.contains("xoxb-1234567890123"));
    assert!(!anonymized.contains("AKIAIOSFODNN7EXAMPLE"));

    assert!(anonymized.contains("[API_KEY:"));
    assert!(anonymized.contains("[GITHUB_PAT:"));
    assert!(anonymized.contains("[GITLAB_PAT:"));
    assert!(anonymized.contains("[SLACK_TOKEN:"));
    assert!(anonymized.contains("[AWS_ACCESS_KEY:"));
}

#[test]
fn test_jwt_redaction() {
    let anonymizer = TelemetryAnonymizer::default();

    let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    let input = format!("User token is {}", jwt);
    let anonymized = anonymizer.anonymize(&input);

    assert!(!anonymized.contains(jwt));
    assert!(anonymized.contains("[JWT:"));
}

#[test]
fn test_email_ip_redaction() {
    let anonymizer = TelemetryAnonymizer::default();

    let input = "Contact user@example.com at IPv4 192.168.1.100 or IPv6 2001:0db8:85a3:0000:0000:8a2e:0370:7334";
    let anonymized = anonymizer.anonymize(input);

    assert!(!anonymized.contains("user@example.com"));
    assert!(!anonymized.contains("192.168.1.100"));
    assert!(!anonymized.contains("2001:0db8:85a3:0000:0000:8a2e:0370:7334"));

    assert!(anonymized.contains("[EMAIL:"));
    assert!(anonymized.contains("[IPV4:"));
    assert!(anonymized.contains("[IPV6:"));
}

#[test]
fn test_tag_redaction_strategy() {
    let config = AnonymizerConfig {
        strategy: RedactionStrategy::Tag,
        enable_dp: false,
        dp_epsilon: 1.0,
    };
    let anonymizer = TelemetryAnonymizer::new(config);

    let input = "Key: sk-1234567890abcdef12345678 Email: test@swal.dev";
    let anonymized = anonymizer.anonymize(input);

    assert_eq!(
        anonymized,
        "Key: [REDACTED:API_KEY] Email: [REDACTED:EMAIL]"
    );
}

#[test]
fn test_clean_input_short_circuit() {
    let anonymizer = TelemetryAnonymizer::default();

    let clean = "System status: OK. All 12 workers running at 98.5% efficiency.";
    assert!(!anonymizer.is_sensitive(clean));
    let result = anonymizer.anonymize(clean);
    assert_eq!(result, clean);
}

#[test]
fn test_differential_privacy_noise() {
    let config = AnonymizerConfig {
        strategy: RedactionStrategy::TruncatedHash,
        enable_dp: true,
        dp_epsilon: 0.5,
    };
    let anonymizer = TelemetryAnonymizer::new(config);

    let original_val = 100.0;
    let sensitivity = 1.0;

    let mut noisy_samples = Vec::new();
    for _ in 0..100 {
        noisy_samples.push(anonymizer.add_differential_privacy_noise(original_val, sensitivity));
    }

    // Ensure values are perturbed and not identical to original
    let matches_original = noisy_samples
        .iter()
        .filter(|&&val| val == original_val)
        .count();
    assert!(matches_original < 5);

    // Mean of noisy samples should be close to original value (unbiased Laplace noise)
    let sum: f64 = noisy_samples.iter().sum();
    let mean = sum / noisy_samples.len() as f64;
    assert!((mean - original_val).abs() < 20.0);
}
