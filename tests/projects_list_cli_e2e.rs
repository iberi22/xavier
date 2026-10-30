use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_projects_list_cli() {
    let data_dir = TempDir::new().unwrap();
    let temp_proj_dir = TempDir::new().unwrap();
    let temp_proj_path = temp_proj_dir.path().to_str().unwrap().to_string();

    // Create DB manually with dummy data
    let db_path = data_dir.path().join("xavier_memory_vec.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute("CREATE TABLE memory_records (workspace_id TEXT)", [])
        .unwrap();

    // Insert projects
    conn.execute(
        "INSERT INTO memory_records (workspace_id) VALUES ('/home/user/my_proj_1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memory_records (workspace_id) VALUES ('/home/user/my_proj_2')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memory_records (workspace_id) VALUES (?1)",
        [&temp_proj_path],
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("xavier").unwrap();
    cmd.env("XAVIER_DATA_DIR", data_dir.path())
        .env("TMPDIR", temp_proj_dir.path().parent().unwrap()) // Mock temp dir location so that it recognizes temp_proj as temp
        .arg("projects")
        .arg("list");

    // Standard list
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("/home/user/my_proj_1"))
        .stdout(predicate::str::contains("/home/user/my_proj_2"))
        .stdout(
            predicate::str::contains("hidden temp projects")
                .or(predicate::str::contains("1 hidden temp projects")),
        )
        .stdout(predicate::str::contains(&temp_proj_path).not());

    let mut cmd2 = Command::cargo_bin("xavier").unwrap();
    cmd2.env("XAVIER_DATA_DIR", data_dir.path())
        .env("TMPDIR", temp_proj_dir.path().parent().unwrap())
        .arg("projects")
        .arg("list")
        .arg("--all");

    // All projects
    cmd2.assert()
        .success()
        .stdout(predicate::str::contains("/home/user/my_proj_1"))
        .stdout(predicate::str::contains("/home/user/my_proj_2"))
        .stdout(predicate::str::contains(&temp_proj_path));

    let mut cmd3 = Command::cargo_bin("xavier").unwrap();
    cmd3.env("XAVIER_DATA_DIR", data_dir.path())
        .env("TMPDIR", temp_proj_dir.path().parent().unwrap())
        .arg("projects")
        .arg("list")
        .arg("--json");

    // Json output
    let output = cmd3.output().unwrap();
    let out_str = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value = serde_json::from_str(&out_str).expect("Valid JSON");
    let arr = json["projects"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(json["filtered"].as_u64().unwrap(), 1);
}
