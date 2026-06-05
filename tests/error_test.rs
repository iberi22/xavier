use xavier::domain::AppError;

#[test]
fn test_app_error_usage() {
    let err = AppError::Internal("test".to_string());
    assert_eq!(format!("{}", err), "Internal error: test");
}
