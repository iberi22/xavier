use xavier::kernel::runner::execute_proxy_command;
use xavier::observability::token_accounting::TRACKER;

#[tokio::test]
async fn test_mcp_kernel_execution_and_token_accounting() {
    let initial_stats = TRACKER.get_stats().await;

    let cmd = "echo 'test 1 ... ok\ntest 2 ... ok\ntest result: ok. 2 passed; 0 failed;'";
    let res = execute_proxy_command(cmd, None, Some("mcp_test_session")).await.unwrap();

    assert_eq!(res.exit_code, 0);
    assert_eq!(res.command, cmd);

    let updated_stats = TRACKER.get_stats().await;
    assert!(updated_stats.operation_count > initial_stats.operation_count);
}
