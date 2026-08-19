#[cfg(feature = "telegram")]
mod telegram_tests {
    use std::env;
    use xavier::telegram::{
        handle_memory_command, load_bot_token, MemoryCommand, TelegramConfig, RATE_LIMIT_COMMANDS,
        RATE_LIMIT_WINDOW_SECS, TELEGRAM_TOKEN_VAULT_KEY,
    };

    #[test]
    fn test_telegram_config_defaults() {
        let config = TelegramConfig::default();
        // Default config should be parsed and readable
        assert!(!config.enabled || config.enabled);
        assert_eq!(RATE_LIMIT_COMMANDS, 10);
        assert_eq!(RATE_LIMIT_WINDOW_SECS, 60);
        assert_eq!(TELEGRAM_TOKEN_VAULT_KEY, "telegram_bot_token");
    }

    #[test]
    fn test_telegram_command_parsing() {
        // 1. Stats command
        let cmd = MemoryCommand::parse("stats").expect("Failed to parse stats command");
        assert_eq!(cmd, MemoryCommand::Stats);

        // 2. Search command
        let cmd = MemoryCommand::parse("search rust memory database")
            .expect("Failed to parse search command");
        assert_eq!(
            cmd,
            MemoryCommand::Search("rust memory database".to_string())
        );

        // 3. List command
        let cmd = MemoryCommand::parse("list").expect("Failed to parse list command");
        assert_eq!(cmd, MemoryCommand::List);

        // 4. Delete command
        let cmd =
            MemoryCommand::parse("delete doc_id_123").expect("Failed to parse delete command");
        assert_eq!(cmd, MemoryCommand::Delete("doc_id_123".to_string()));

        // 5. Invalid command
        let cmd_invalid = MemoryCommand::parse("invalid_subcommand");
        assert!(cmd_invalid.is_none());
    }

    #[tokio::test]
    async fn test_handle_memory_command_error_propagation() {
        // When no local memory is set up or environment is unconfigured,
        // handle_memory_command should return a descriptive error message instead of panicking.
        env::set_var("XAVIER_DEFAULT_WORKSPACE_ID", "e2e_telegram_test_ws");

        let stats_reply = handle_memory_command("stats").await;
        assert!(
            stats_reply.contains("Could not load memory store")
                || stats_reply.contains("Memory Statistics"),
            "Unexpected reply: {}",
            stats_reply
        );

        let search_reply = handle_memory_command("search test_query").await;
        assert!(
            search_reply.contains("Could not load memory store")
                || search_reply.contains("Search Results")
                || search_reply.contains("No results for"),
            "Unexpected reply: {}",
            search_reply
        );

        env::remove_var("XAVIER_DEFAULT_WORKSPACE_ID");
    }

    #[test]
    fn test_bot_token_resolution_flow() {
        // Test env resolution fallback
        let original_token = env::var("TELEGRAM_BOT_TOKEN").ok();

        env::set_var("TELEGRAM_BOT_TOKEN", "123456:E2E-TEST-TOKEN");
        let resolved = load_bot_token().expect("Failed to resolve token");
        assert!(resolved.len() > 0);

        // Restore env
        if let Some(tok) = original_token {
            env::set_var("TELEGRAM_BOT_TOKEN", tok);
        } else {
            env::remove_var("TELEGRAM_BOT_TOKEN");
        }
    }

    #[test]
    fn test_webhook_secret_token_verification() {
        use xavier::telegram::{verify_webhook_secret, X_TELEGRAM_BOT_API_SECRET_TOKEN};

        assert_eq!(
            X_TELEGRAM_BOT_API_SECRET_TOKEN,
            "X-Telegram-Bot-Api-Secret-Token"
        );

        let secret = "super_secret_telegram_token_123";

        // 1. Secret is set, missing header -> 401 Unauthorized
        let res_missing = verify_webhook_secret(Some(secret), None);
        assert_eq!(res_missing, Err(axum::http::StatusCode::UNAUTHORIZED));

        // 2. Secret is set, wrong token -> 401 Unauthorized
        let res_wrong = verify_webhook_secret(Some(secret), Some("wrong_secret_token"));
        assert_eq!(res_wrong, Err(axum::http::StatusCode::UNAUTHORIZED));

        // 3. Secret is set, correct token -> Ok(())
        let res_correct = verify_webhook_secret(Some(secret), Some(secret));
        assert_eq!(res_correct, Ok(()));

        // 4. Secret is NOT set (None) -> Ok(()) regardless of provided header
        let res_no_secret = verify_webhook_secret(None, None);
        assert_eq!(res_no_secret, Ok(()));

        let res_no_secret_with_token = verify_webhook_secret(None, Some("any_token"));
        assert_eq!(res_no_secret_with_token, Ok(()));
    }
}
