//! Security Service Stub for xavier-core
use anyhow::{anyhow, Result};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub enabled: bool,
    pub encryption_algorithm: String,
    pub encryption_at_rest_enabled: bool,
    pub master_key_name: String,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            encryption_algorithm: "AES-256-GCM".to_string(),
            encryption_at_rest_enabled: false,
            master_key_name: "xavier_master_key".to_string(),
        }
    }
}

pub struct SecurityService;

impl SecurityService {
    pub fn new() -> Self {
        Self
    }

    pub fn get_config(&self) -> SecurityConfig {
        SecurityConfig::default()
    }

    pub fn get_key_manager(&self) -> Result<Arc<crate::crypto::KeyManager>> {
        Err(anyhow!("Security stub: get_key_manager not implemented"))
    }

    pub fn get_kek(&self) -> Result<crate::crypto::keys::KEK> {
        Err(anyhow!("Security stub: get_kek not implemented"))
    }
}

pub fn get_security_service() -> &'static SecurityService {
    static INSTANCE: SecurityService = SecurityService;
    &INSTANCE
}
