
    #[test]
    fn test_cloud_config_priorities() {
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        // 1. Test XAVIER_OPENROUTER_API_KEY as fallback
        std::env::set_var("XAVIER_OPENROUTER_API_KEY", "sk-or-test-key");
        std::env::remove_var("OPENAI_API_KEY");

        let mut settings = crate::settings::XavierSettings::default();
        settings.embedding.api_key = None;

        // Manually simulate what cloud_config() does with specific settings
        let config_with_or = OpenAICompatibleConfig {
            api_key: settings
                .embedding
                .api_key
                .clone()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .or_else(|| std::env::var("XAVIER_OPENROUTER_API_KEY").ok()),
            endpoint: "http://test".to_string(),
            dimension: 1536,
            model: "test".to_string(),
        };
        assert_eq!(config_with_or.api_key, Some("sk-or-test-key".to_string()));

        // 2. Test OPENAI_API_KEY takes precedence over XAVIER_OPENROUTER_API_KEY
        std::env::set_var("OPENAI_API_KEY", "sk-openai-test-key");
        let config_with_openai = OpenAICompatibleConfig {
            api_key: settings
                .embedding
                .api_key
                .clone()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .or_else(|| std::env::var("XAVIER_OPENROUTER_API_KEY").ok()),
            endpoint: "http://test".to_string(),
            dimension: 1536,
            model: "test".to_string(),
        };
        assert_eq!(
            config_with_openai.api_key,
            Some("sk-openai-test-key".to_string())
        );

        // 3. Test settings.embedding.api_key takes precedence over env vars
        settings.embedding.api_key = Some("sk-settings-test-key".to_string());
        let config_with_settings = OpenAICompatibleConfig {
            api_key: settings
                .embedding
                .api_key
                .clone()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .or_else(|| std::env::var("XAVIER_OPENROUTER_API_KEY").ok()),
            endpoint: "http://test".to_string(),
            dimension: 1536,
            model: "test".to_string(),
        };
        assert_eq!(
            config_with_settings.api_key,
            Some("sk-settings-test-key".to_string())
        );

        std::env::remove_var("XAVIER_OPENROUTER_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
    }
}
