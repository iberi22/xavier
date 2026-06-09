use crate::security::auth::resolve_xavier_token;
use subtle::ConstantTimeEq;

/// Validates a provided Bearer token against the expected XAVIER_TOKEN.
/// In test mode, it also accepts "test-token".
pub fn validate_token(provided_token: &str) -> bool {
    // 1. Check if we are in test mode and the token is "test-token"
    #[cfg(test)]
    if provided_token == "test-token" {
        return true;
    }

    // 2. Resolve the official token
    let expected_token: String = match resolve_xavier_token() {
        Ok(token) => token,
        Err(_) => return false,
    };

    // 3. Constant-time comparison
    let provided_bytes = provided_token.as_bytes();
    let expected_bytes = expected_token.as_bytes();

    if provided_bytes.len() != expected_bytes.len() {
        return false;
    }

    provided_bytes.ct_eq(expected_bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_token_test_mode() {
        // Mock XAVIER_TOKEN for this test
        std::env::set_var("XAVIER_TOKEN", "official-token");
        assert!(validate_token("test-token"));
        assert!(validate_token("official-token"));
        assert!(!validate_token("wrong-token"));
        std::env::remove_var("XAVIER_TOKEN");
    }
}
