use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

xavier::isolate_test_process!();

fn run_cli(args: &[&str], env_dir: &Path) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.current_dir(env_dir)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", env_dir)
        .env("XAVIER_CONFIG_PATH", env_dir.join(".xavier/config"))
        .env("XAVIER_RUNTIME_STATE_PATH", env_dir.join(".xavier/state"))
        .env("XAVIER_DATA_DIR", env_dir.join(".xavier/data"))
        .env("XAVIER_DAEMON_URL", "http://127.0.0.1:0/invalid")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("failed to spawn xavier cli process");
    let start = Instant::now();
    let timeout = Duration::from_secs(60);

    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                return child.wait_with_output().expect("failed to get output");
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    panic!("xavier cli process timed out after 60s");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("error attempting to wait: {}", e),
        }
    }
}

#[test]
fn offline_add_then_search_finds_unique_token() {
    let env_dir = TempDir::new().expect("tempdir");

    // Test add
    let add_args = ["add", "smoke anchor note carrying token zq81xk-a1"];
    let add_out = run_cli(&add_args, env_dir.path());

    assert!(
        add_out.status.success(),
        "Add failed!\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&add_out.stdout),
        String::from_utf8_lossy(&add_out.stderr)
    );

    // Test search
    let search_args = ["search", "smoke anchor note", "--offline-ok"];
    let search_out = run_cli(&search_args, env_dir.path());

    assert!(
        search_out.status.success(),
        "Search failed!\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&search_out.stdout),
        String::from_utf8_lossy(&search_out.stderr)
    );

    let search_stdout = String::from_utf8_lossy(&search_out.stdout);
    assert!(
        search_stdout.contains("zq81xk-a1"),
        "Search output did not contain 'zq81xk-a1'. Output:\n{}",
        search_stdout
    );
}

#[test]
fn offline_memory_survives_process_restart() {
    let env_dir = TempDir::new().expect("tempdir");

    // Process A: add
    let add_args = ["add", "another note carrying token zq81xk-b2"];
    let add_out = run_cli(&add_args, env_dir.path());

    assert!(
        add_out.status.success(),
        "Add failed!\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&add_out.stdout),
        String::from_utf8_lossy(&add_out.stderr)
    );

    // Assert xavier_memory_vec.db exists
    let db_path = env_dir
        .path()
        .join(".xavier/data")
        .join("vec-store.sqlite3");
    assert!(
        db_path.exists(),
        "Expected database to exist at {}",
        db_path.display()
    );

    // Process B: search
    let search_args = ["search", "zq81xk-b2", "--offline-ok"];
    let search_out = run_cli(&search_args, env_dir.path());

    assert!(
        search_out.status.success(),
        "Search failed!\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&search_out.stdout),
        String::from_utf8_lossy(&search_out.stderr)
    );

    let search_stdout = String::from_utf8_lossy(&search_out.stdout);
    assert!(
        search_stdout.contains("zq81xk-b2"),
        "Search output did not contain 'zq81xk-b2'. Output:\n{}",
        search_stdout
    );
}
