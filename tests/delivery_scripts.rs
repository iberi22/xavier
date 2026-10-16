use std::{io::ErrorKind, path::Path, process::Command};

// Shell suites require jq, git and standard Unix tools; the review adapter also
// needs timeout and sha256sum (or shasum). Python suites use only the stdlib,
// with git additionally required by the scope suite. Missing dependencies fail.
fn run_delivery_suite(interpreter: &str, script: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(root.join(script).is_file(), "Missing suite: {script}");
    let result = Command::new(interpreter)
        .arg(script)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "Delivery Test")
        .env("GIT_AUTHOR_EMAIL", "delivery-test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Delivery Test")
        .env("GIT_COMMITTER_EMAIL", "delivery-test@example.invalid")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "commit.gpgsign")
        .env("GIT_CONFIG_VALUE_0", "false")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        // A git hook running `cargo test` exports these; the suites must never touch the real repo.
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output();
    let output = match result {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            eprintln!("Skipping {script}: {interpreter} is missing");
            return;
        }
        Err(error) => panic!("Could not run {script} with {interpreter}: {error}"),
        Ok(output) => output,
    };
    assert!(
        output.status.success(),
        "{script} failed ({}):\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn delivery_done_check_task_suite() {
    run_delivery_suite("bash", "scripts/tests/done-check-task-test.sh");
}

#[test]
fn delivery_done_check_feature_suite() {
    run_delivery_suite("bash", "scripts/tests/done-check-feature-test.sh");
}

#[test]
fn delivery_done_check_increment_suite() {
    run_delivery_suite("bash", "scripts/tests/done-check-increment-test.sh");
}

#[test]
fn delivery_atlas_review_adapter_suite() {
    run_delivery_suite("bash", "scripts/tests/atlas-review-adapter-test.sh");
}

#[test]
fn delivery_runner_suite() {
    run_delivery_suite("bash", "scripts/tests/delivery-run-test.sh");
}

#[test]
fn delivery_dag_check_suite() {
    run_delivery_suite("python3", "scripts/tests/test_delivery_dag_check.py");
}

#[test]
fn delivery_scope_check_suite() {
    run_delivery_suite("python3", "scripts/tests/test_delivery_scope_check.py");
}
