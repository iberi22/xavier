//! Test environment guard and serial execution verification
//!
//! Provides EnvGuard for resetting environment variables after tests,
//! combined with serial_test #[serial] annotation to guarantee test thread isolation.

use serial_test::serial;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

pub struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    vars: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    pub fn new(keys: &[&'static str]) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let vars = keys.iter().map(|&k| (k, std::env::var(k).ok())).collect();
        Self { _lock: lock, vars }
    }

    pub fn set(&self, key: &'static str, value: &str) {
        std::env::set_var(key, value);
    }

    pub fn remove(&self, key: &'static str) {
        std::env::remove_var(key);
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, orig_val) in &self.vars {
            if let Some(val) = orig_val {
                std::env::set_var(key, val);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

#[test]
#[serial]
fn test_env_guard_isolation_set_and_restore() {
    let key = "XAVIER_TEST_ENV_GUARD_VAR_1";
    std::env::remove_var(key);

    {
        let guard = EnvGuard::new(&[key]);
        guard.set(key, "temporary_val");
        assert_eq!(std::env::var(key).unwrap(), "temporary_val");
    }

    assert!(std::env::var(key).is_err());
}

#[test]
#[serial]
fn test_env_guard_serial_execution_isolation() {
    let key = "XAVIER_TEST_ENV_GUARD_VAR_2";
    std::env::set_var(key, "original_val");

    {
        let guard = EnvGuard::new(&[key]);
        guard.set(key, "modified_val");
        assert_eq!(std::env::var(key).unwrap(), "modified_val");
    }

    assert_eq!(std::env::var(key).unwrap(), "original_val");
    std::env::remove_var(key);
}
