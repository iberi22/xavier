use std::path::PathBuf;

pub fn state_dir() -> PathBuf {
    crate::settings::serialization::resolve_state_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn resolve_state_dir_matches_maloca_and_default() {
        let _env = crate::test_support::TempEnv::new();
        std::env::remove_var("XAVIER_STATE_DIR");
        let expected_temp_dir =
            std::env::temp_dir().join(format!("xavier-test-state-{}", std::process::id()));

        let neutral_path = state_dir();
        let maloca_path = crate::maloca::MalocaStore::resolve_state_dir();

        assert_eq!(neutral_path, maloca_path);
        assert_eq!(neutral_path, expected_temp_dir);

        std::env::set_var("XAVIER_STATE_DIR", "custom_state_dir");
        let custom_neutral_path = state_dir();
        let custom_maloca_path = crate::maloca::MalocaStore::resolve_state_dir();

        assert_eq!(custom_neutral_path, custom_maloca_path);
        assert_eq!(
            custom_neutral_path,
            std::path::PathBuf::from("custom_state_dir")
        );
    }
}
