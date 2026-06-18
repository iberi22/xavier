//! Logical concern: Environment variable management for Xavier settings.
//!
//! This module handles synchronization between the settings struct and environment variables.

use super::types::XavierSettings;
use super::validation::non_empty;

pub fn set_if_absent(key: &str, value: &str) {
    if std::env::var_os(key).is_none() {
        std::env::set_var(key, value);
    }
}

pub fn set_optional_if_absent(key: &str, value: Option<String>) {
    if let Some(value) = value {
        set_if_absent(key, &value);
    }
}

pub fn apply_to_env_impl(settings: &XavierSettings) {
    set_if_absent("XAVIER_HOST", &settings.server.host);
    set_if_absent("XAVIER_PORT", &settings.server.port.to_string());
    set_if_absent("XAVIER_LOG_LEVEL", &settings.server.log_level);
    set_if_absent(
        "XAVIER_CODE_GRAPH_DB_PATH",
        &settings.server.code_graph_db_path,
    );
    set_optional_if_absent("XAVIER_URL", non_empty(&settings.server.url));
    set_optional_if_absent("XAVIER_CONFIG_PATH", settings.server.config_path.clone());

    set_if_absent(
        "XAVIER_DEFAULT_WORKSPACE_ID",
        &settings.workspace.default_workspace_id,
    );
    set_if_absent("XAVIER_DEFAULT_PLAN", &settings.workspace.default_plan);
    set_optional_if_absent(
        "XAVIER_STORAGE_LIMIT_BYTES",
        settings
            .workspace
            .storage_limit_bytes
            .map(|v| v.to_string()),
    );
    set_optional_if_absent(
        "XAVIER_REQUEST_LIMIT",
        settings.workspace.request_limit.map(|v| v.to_string()),
    );
    set_optional_if_absent(
        "XAVIER_REQUEST_UNIT_LIMIT",
        settings.workspace.request_unit_limit.map(|v| v.to_string()),
    );
    set_if_absent(
        "XAVIER_EMBEDDING_PROVIDER_MODE",
        &settings.workspace.embedding_provider_mode,
    );
    set_if_absent(
        "XAVIER_MANAGED_GOOGLE_EMBEDDINGS",
        if settings.workspace.managed_google_embeddings {
            "true"
        } else {
            "false"
        },
    );
    set_optional_if_absent(
        "XAVIER_RRF_K",
        settings.retrieval.rrf_k.map(|v| v.to_string()),
    );
    set_optional_if_absent(
        "XAVIER_ZONE_BOOST",
        settings
            .retrieval
            .zone_boost_multiplier
            .map(|v| v.to_string()),
    );
    set_optional_if_absent(
        "XAVIER_ZONE_PENALTY",
        settings
            .retrieval
            .zone_penalty_multiplier
            .map(|v| v.to_string()),
    );
    set_if_absent("XAVIER_SYNC_POLICY", &settings.workspace.sync_policy);

    set_if_absent("XAVIER_MEMORY_BACKEND", &settings.memory.backend);
    set_if_absent("XAVIER_DATA_DIR", &settings.memory.data_dir);
    set_if_absent(
        "XAVIER_EMBEDDING_DIMENSIONS",
        &settings.memory.embedding_dimensions.to_string(),
    );
    set_if_absent("XAVIER_WORKSPACE_DIR", &settings.memory.workspace_dir);
    set_if_absent("XAVIER_MEMORY_FILE_PATH", &settings.memory.file_path);
    set_if_absent("XAVIER_MEMORY_SQLITE_PATH", &settings.memory.sqlite_path);
    set_if_absent("XAVIER_MEMORY_VEC_PATH", &settings.memory.vec_path);

    // Memory layers
    set_if_absent(
        "XAVIER_WORKING_MEMORY_CAPACITY",
        &settings.memory_layers.working.capacity.to_string(),
    );
    set_if_absent(
        "XAVIER_WORKING_LRU_THRESHOLD",
        &settings
            .memory_layers
            .working
            .lru_exempt_access_threshold
            .to_string(),
    );
    set_if_absent(
        "XAVIER_WORKING_BM25_K1",
        &settings.memory_layers.working.bm25_k1.to_string(),
    );
    set_if_absent(
        "XAVIER_WORKING_BM25_B",
        &settings.memory_layers.working.bm25_b.to_string(),
    );
    set_if_absent(
        "XAVIER_EPISODIC_SUMMARY_WINDOW",
        &settings.memory_layers.episodic.summary_window.to_string(),
    );
    set_if_absent(
        "XAVIER_MAX_EPISODIC_SESSIONS",
        &settings.memory_layers.episodic.max_sessions.to_string(),
    );
    set_if_absent(
        "XAVIER_EPISODIC_MIN_EVENT_IMPORTANCE",
        &settings
            .memory_layers
            .episodic
            .min_event_importance
            .to_string(),
    );

    set_if_absent("XAVIER_MODEL_PROVIDER", &settings.models.provider);
    set_if_absent("XAVIER_API_FLAVOR", &settings.models.api_flavor);
    set_if_absent("XAVIER_LOCAL_LLM_MODEL", &settings.models.local_llm_model);
    set_if_absent("XAVIER_EMBEDDING_URL", &settings.models.embedding_url);
    set_if_absent("XAVIER_EMBEDDING_MODEL", &settings.models.embedding_model);
    set_optional_if_absent(
        "XAVIER_ROUTER_RETRIEVED_MODEL",
        non_empty(&settings.models.router_retrieved_model),
    );
    set_optional_if_absent(
        "XAVIER_ROUTER_COMPLEX_MODEL",
        non_empty(&settings.models.router_complex_model),
    );
    set_if_absent(
        "XAVIER_ROUTER_FAST_MODEL",
        &settings.models.router_fast_model,
    );
    set_if_absent(
        "XAVIER_ROUTER_QUALITY_MODEL",
        &settings.models.router_quality_model,
    );
    set_optional_if_absent("XAVIER_LLM_MODEL", settings.models.llm_model.clone());
    set_optional_if_absent(
        "XAVIER_CLOUD_LLM_MODEL",
        settings.models.cloud_llm_model.clone(),
    );

    set_if_absent(
        "XAVIER_DISABLE_HYDE",
        if settings.retrieval.disable_hyde {
            "true"
        } else {
            "false"
        },
    );
    set_optional_if_absent(
        "XAVIER_ZONE_BOOST",
        settings
            .retrieval
            .zone_boost_multiplier
            .map(|v| v.to_string()),
    );
    set_optional_if_absent(
        "XAVIER_ZONE_PENALTY",
        settings
            .retrieval
            .zone_penalty_multiplier
            .map(|v| v.to_string()),
    );

    set_if_absent(
        "XAVIER_SYNC_INTERVAL_MS",
        &settings.sync.interval_ms.to_string(),
    );
    set_if_absent(
        "XAVIER_SYNC_LAG_THRESHOLD_MS",
        &settings.sync.lag_threshold_ms.to_string(),
    );
    set_if_absent(
        "XAVIER_SYNC_SAVE_OK_RATE_THRESHOLD",
        &settings.sync.save_ok_rate_threshold.to_string(),
    );
    set_if_absent(
        "XAVIER_SYNC_MAX_RETRIES",
        &settings.sync.max_retries.to_string(),
    );
    set_if_absent(
        "XAVIER_SYNC_RETRY_DELAY_MS",
        &settings.sync.retry_delay_ms.to_string(),
    );
    set_if_absent(
        "XAVIER_SYNC_MIN_HEALTH_INTERVAL_MS",
        &settings.sync.min_health_interval_ms.to_string(),
    );
    set_if_absent(
        "XAVIER_SYNC_TIMEOUT_MS",
        &settings.sync.timeout_ms.to_string(),
    );

    // Embedding settings
    set_optional_if_absent("XAVIER_EMBEDDER", non_empty(&settings.embedding.embedder));
    set_optional_if_absent(
        "XAVIER_GLLM_MODEL",
        non_empty(&settings.embedding.gllm_model),
    );
    set_optional_if_absent(
        "XAVIER_EMBEDDING_API_FLAVOR",
        non_empty(&settings.embedding.api_flavor),
    );
    set_optional_if_absent(
        "XAVIER_GLLM_DIMENSION",
        settings.embedding.gllm_dimension.map(|v| v.to_string()),
    );

    // Embedding cache settings
    set_if_absent(
        "XAVIER_EMBEDDING_CACHE_ENABLED",
        if settings.embedding.cache_enabled {
            "true"
        } else {
            "false"
        },
    );
    set_if_absent(
        "XAVIER_EMBEDDING_CACHE_SIZE",
        &settings.embedding.cache_size.to_string(),
    );
    set_if_absent(
        "XAVIER_EMBEDDING_CACHE_TTL_HOURS",
        &settings.embedding.cache_ttl_hours.to_string(),
    );
    set_optional_if_absent(
        "XAVIER_EMBEDDING_CACHE_DB_PATH",
        non_empty(&settings.embedding.cache_db_path),
    );

    // Security settings
    set_optional_if_absent(
        "XAVIER_ALLOWED_DOMAINS",
        non_empty(&settings.security.allowed_domains),
    );
    set_if_absent(
        "XAVIER_ENCRYPTION_AT_REST_ENABLED",
        if settings.security.encryption_at_rest_enabled {
            "true"
        } else {
            "false"
        },
    );
    set_if_absent(
        "XAVIER_MASTER_KEY_NAME",
        &settings.security.master_key_name,
    );

    // Telegram settings
    set_if_absent(
        "XAVIER_TELEGRAM_ENABLED",
        if settings.telegram.enabled {
            "true"
        } else {
            "false"
        },
    );

    // Router settings
    set_optional_if_absent(
        "XAVIER_ROUTER_POLICY_PATH",
        non_empty(&settings.router.policy_path),
    );
    set_if_absent(
        "XAVIER_ROUTER_POLICY_REFRESH_SECS",
        &settings.router.policy_refresh_secs.to_string(),
    );

    // Chronicle settings
    set_optional_if_absent(
        "Xavier_CHRONICLE_MODEL",
        non_empty(&settings.chronicle.model),
    );

    // Enterprise settings
    set_if_absent("XAVIER_ENTERPRISE_DB_PATH", &settings.enterprise.db_path);

    // Agent settings
    set_optional_if_absent(
        "XAVIER_WEEKLY_BUDGET",
        settings.agents.weekly_budget.map(|v| v.to_string()),
    );

    // Advanced settings
    set_if_absent(
        "XAVIER_QJL_THRESHOLD",
        &settings.advanced.qjl_threshold.to_string(),
    );
    set_if_absent(
        "XAVIER_ENTITY_EXTRACTION_ENABLED",
        if settings.advanced.entity_extraction_enabled {
            "true"
        } else {
            "false"
        },
    );
    set_if_absent(
        "XAVIER_AUDIT_CHAIN_ENABLED",
        if settings.advanced.audit_chain_enabled {
            "true"
        } else {
            "false"
        },
    );
    set_optional_if_absent(
        "XAVIER_PANEL_STORE_DIR",
        non_empty(&settings.advanced.panel_store_dir),
    );

    // PgHeart settings
    set_optional_if_absent("PGHEART_INSTANCE_ID", settings.pgheart.instance_id.clone());
    set_if_absent(
        "PGHEART_SYNC_INTERVAL_MS",
        &settings.pgheart.sync_interval_ms.to_string(),
    );
    set_if_absent(
        "PGHEART_AUTO_HEARTBEAT",
        if settings.pgheart.auto_heartbeat {
            "true"
        } else {
            "false"
        },
    );

    // Aliases for backward compatibility
    set_if_absent(
        "XAVIER_WORKSPACE_ID",
        &settings.workspace.default_workspace_id,
    );
}
