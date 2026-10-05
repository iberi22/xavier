use xavier::domain::AppError;

#[test]
fn test_app_error_usage() {
    let err = AppError::Internal("test".to_string());
    assert_eq!(format!("{}", err), "Internal error: test");
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
