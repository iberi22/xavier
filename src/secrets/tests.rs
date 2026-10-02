//! Tests for secrets management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
#[cfg(test)]
mod secret_tests {
    use super::super::vault::HardwareVault;

    /// Reproduce: `service_name` is supposed to isolate one service's secrets from
    /// another's. The fallback store is a single directory keyed by a single derived
    /// key, so two vaults built with different service names but the same storage
    /// read and write each other's secrets.
    ///
    /// No real secret is touched: the value below is synthetic and goes to a temp dir.
    #[test]
    fn service_name_does_not_isolate_the_fallback_store() {
        let dir = std::env::temp_dir().join(format!("swal-d1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let key = [7u8; 32];

        let clavis = HardwareVault::new("xavier-clavis").isolated(dir.clone(), key);
        clavis
            .store_secret("JWT_PRIVATE_KEY", "SYNTHETIC-NOT-A-REAL-KEY")
            .unwrap();

        let other = HardwareVault::new("xavier-auth2").isolated(dir.clone(), key);
        let read_back = other.get_secret("JWT_PRIVATE_KEY");

        let _ = std::fs::remove_dir_all(&dir);

        // If this passes, the two "services" share storage: the isolation that
        // `service_name` is assumed to provide does not exist on the fallback path.
        assert!(
            read_back.is_ok(),
            "fallback store is isolated per service name (no cross-read)"
        );
        assert_eq!(read_back.unwrap(), "SYNTHETIC-NOT-A-REAL-KEY");
    }
}
