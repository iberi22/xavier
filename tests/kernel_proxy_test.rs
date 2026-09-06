use anyhow::Result;
use xavier::kernel::runner::{condense_output, execute_proxy_command, index_command_failure};
use xavier::memory::store::{InMemoryMemoryStore, MemoryStore};

#[tokio::test]
async fn test_proxy_command_success() -> Result<()> {
    let store = InMemoryMemoryStore::new();
    let workspace_id = "test-workspace";

    #[cfg(target_os = "windows")]
    let cmd = "echo hello";
    #[cfg(not(target_os = "windows"))]
    let cmd = "echo hello";

    let res = execute_proxy_command(cmd, Some(workspace_id), Some(&store)).await?;

    assert_eq!(res.exit_code, 0);
    assert!(res.stdout.contains("hello"));
    assert!(res.failure_record.is_none());

    let records = store.list(workspace_id).await?;
    assert!(records.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_proxy_command_failure_indexing() -> Result<()> {
    let store = InMemoryMemoryStore::new();
    let workspace_id = "test-workspace";

    #[cfg(target_os = "windows")]
    let cmd = "cmd /c exit 1";
    #[cfg(not(target_os = "windows"))]
    let cmd = "sh -c 'echo \"failure error trace\" >&2; exit 1'";

    let res = execute_proxy_command(cmd, Some(workspace_id), Some(&store)).await?;

    assert_ne!(res.exit_code, 0);
    assert!(res.failure_record.is_some());

    let record = res.failure_record.unwrap();
    assert!(record.path.starts_with("terminal/failures/"));
    assert_eq!(
        record.metadata.get("kind").and_then(|v| v.as_str()),
        Some("failure_trace")
    );
    assert_eq!(
        record.metadata.get("command").and_then(|v| v.as_str()),
        Some(cmd)
    );
    assert_eq!(
        record.metadata.get("exit_code").and_then(|v| v.as_i64()),
        Some(i64::from(res.exit_code))
    );

    let fetched = store.get(workspace_id, &record.id).await?;
    assert!(fetched.is_some());
    let fetched_rec = fetched.unwrap();
    assert_eq!(fetched_rec.path, record.path);

    Ok(())
}

#[tokio::test]
async fn test_index_command_failure_direct() -> Result<()> {
    let store = InMemoryMemoryStore::new();
    let workspace_id = "ws-direct";
    let cmd = "cargo test non_existent_test_suite";
    let exit_code = 101;
    let snippet = "error: no test target found matching `non_existent_test_suite`";

    let record =
        index_command_failure(&store, workspace_id, cmd, exit_code, snippet).await?;

    assert!(record.path.starts_with("terminal/failures/"));
    assert_eq!(record.content, snippet);
    assert_eq!(
        record.metadata.get("kind").and_then(|v| v.as_str()),
        Some("failure_trace")
    );
    assert_eq!(
        record.metadata.get("command").and_then(|v| v.as_str()),
        Some(cmd)
    );

    let list = store.list(workspace_id).await?;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, record.id);

    Ok(())
}

#[test]
fn test_condense_output() {
    let stdout = "short stdout";
    let stderr = "short stderr";
    let output = condense_output(stdout, stderr);
    assert!(output.contains("STDOUT:\nshort stdout"));
    assert!(output.contains("STDERR:\nshort stderr"));

    let long_str = "a".repeat(3000);
    let condensed = condense_output(&long_str, "");
    assert!(condensed.contains("[... condensed 1000 bytes ...]"));
}
