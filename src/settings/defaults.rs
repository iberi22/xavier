//! Logical concern: Default values for Xavier settings.
//!
//! This module contains Default trait implementations for the configuration structs.

use super::types::*;

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: XavierSettings::default_host(),
            port: XavierSettings::default_port(),
            log_level: XavierSettings::default_log_level(),
            code_graph_db_path: XavierSettings::default_code_graph_db_path(),
            url: String::new(),
            config_path: None,
        }
    }
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            default_workspace_id: "default".to_string(),
            default_plan: "community".to_string(),
            storage_limit_bytes: None,
            request_limit: None,
            request_unit_limit: None,
            embedding_provider_mode: "bring_your_own".to_string(),
            managed_google_embeddings: false,
            sync_policy: "local_only".to_string(),
        }
    }
}

impl Default for MemorySettings {
    fn default() -> Self {
        let data_dir = XavierSettings::resolve_data_dir();
        Self {
            backend: "vec".to_string(),
            data_dir: data_dir.to_string_lossy().to_string(),
            embedding_dimensions: 768,
            workspace_dir: data_dir.join("workspaces").to_string_lossy().to_string(),
            file_path: data_dir
                .join("workspaces")
                .join("default")
                .join("memory-store.json")
                .to_string_lossy()
                .to_string(),
            sqlite_path: data_dir
                .join("memory-store.sqlite3")
                .to_string_lossy()
                .to_string(),
            vec_path: data_dir
                .join("vec-store.sqlite3")
                .to_string_lossy()
                .to_string(),
            supabase_url: None,
            supabase_key: None,
            postgres_url: None,
        }
    }
}

impl Default for WorkingMemoryLayerConfig {
    fn default() -> Self {
        Self {
            capacity: 100,
            lru_exempt_access_threshold: 2,
            bm25_k1: 1.5,
            bm25_b: 0.75,
        }
    }
}

impl Default for EpisodicMemoryLayerConfig {
    fn default() -> Self {
        Self {
            summary_window: 10,
            max_sessions: 50,
            min_event_importance: 0.5,
        }
    }
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            provider: "local".to_string(),
            api_flavor: "openai-compatible".to_string(),
            local_llm_url: "http://localhost:11434/v1".to_string(),
            local_llm_model: "qwen3-coder".to_string(),
            embedding_url: "http://localhost:11434/v1".to_string(),
            embedding_model: "embeddinggemma".to_string(),
            router_retrieved_model: String::new(),
            router_complex_model: String::new(),
            router_fast_model: "opencode/minimax-m2.7".to_string(),
            router_quality_model: "opencode/deepseek-v4-pro".to_string(),
            llm_model: None,
            llm_api_key: None,
            cloud_llm_model: None,
            cloud_llm_url: None,
            local_llm_api_key: None,
            local_anthropic_url: None,
            compaction_model: None,
        }
    }
}

impl Default for RetrievalSettings {
    fn default() -> Self {
        Self {
            disable_hyde: true,
            rrf_k: None,
            keyword_weight: None,
            vector_weight: None,
            zone_boost_multiplier: None,
            zone_penalty_multiplier: None,
            cache_warming_enabled: false,
            cache_warming_threshold: None,
            learned_policy: NavigationPolicyConfig::default(),
        }
    }
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            interval_ms: 300_000,
            lag_threshold_ms: 30_000,
            save_ok_rate_threshold: 0.95,
            max_retries: 3,
            retry_delay_ms: 1_000,
            min_health_interval_ms: 1_000,
            timeout_ms: 5_000,
        }
    }
}

impl Default for EmbeddingSettings {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            embedder: String::new(),
            gllm_model: String::new(),
            api_flavor: "openai-compatible".to_string(),
            api_key: None,
            gllm_dimension: None,
            cache_enabled: true,
            cache_size: 10_000,
            cache_ttl_hours: 24,
            cache_db_path: "data/embedding_cache.db".to_string(),
        }
    }
}

impl Default for RouterSettings {
    fn default() -> Self {
        Self {
            policy_path: String::new(),
            policy_refresh_secs: 300,
        }
    }
}

impl Default for EnterpriseSettings {
    fn default() -> Self {
        Self {
            db_path: "data/enterprise.db".to_string(),
        }
    }
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            qjl_threshold: 500,
            entity_extraction_enabled: true,
            audit_chain_enabled: true,
            panel_store_dir: String::new(),
            minhash_threshold: 0.85,
        }
    }
}

impl Default for PgHeartSettings {
    fn default() -> Self {
        Self {
            url: None,
            token: None,
            instance_id: None,
            sync_interval_ms: 60000,
            auto_heartbeat: true,
        }
    }
}
