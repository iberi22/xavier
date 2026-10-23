//! Contract tests for the headless server packaging in `packaging/linux/`.
//! The installer tests run the real `install.sh` against a throwaway archive.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CARGO_TOML: &str = include_str!("../Cargo.toml");
const SYSTEM_UNIT: &str = include_str!("../packaging/linux/xavier.service");
const USER_UNIT: &str = include_str!("../packaging/linux/xavier-user.service");

fn packaging_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/linux")
}

fn feature_list(name: &str) -> Vec<String> {
    let line = CARGO_TOML
        .lines()
        .find(|l| l.starts_with(&format!("{name} = [")))
        .unwrap_or_else(|| panic!("feature `{name}` missing from Cargo.toml"));
    line.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

#[test]
fn headless_feature_excludes_tui_and_panel() {
    let headless = feature_list("headless");
    for forbidden in ["cli-interactive", "panel-ui", "tauri", "local-gllm"] {
        assert!(
            !headless.iter().any(|f| f == forbidden),
            "headless must not enable `{forbidden}`"
        );
    }
    assert!(feature_list("default").iter().any(|f| f == "headless"));
}

#[test]
fn system_unit_runs_headless_as_dedicated_user() {
    for required in [
        "ExecStart=/usr/bin/xavier http --no-ui",
        "User=xavier",
        "Environment=HOME=/var/lib/xavier",
        "EnvironmentFile=/etc/xavier/xavier.env",
        "NoNewPrivileges=true",
        "ProtectSystem=strict",
    ] {
        assert!(
            SYSTEM_UNIT.lines().any(|l| l == required),
            "system unit lacks `{required}`"
        );
    }
    assert!(USER_UNIT
        .lines()
        .any(|l| l == "ExecStart=%h/.local/bin/xavier http --no-ui"));
}

/// Builds `xavier-server-test-x86_64-unknown-linux-gnu.tar.gz` and its `.sha256` in `dir`.
fn fake_release(dir: &Path) -> (PathBuf, PathBuf) {
    let make = packaging_dir().join("make-tarball.sh");
    let bin = dir.join("xavier-fake");
    fs::write(&bin, "#!/bin/sh\necho xavier-fake\n").unwrap();
    let out = Command::new("sh")
        .arg(&make)
        .args([bin.to_str().unwrap(), "test", "x86_64-unknown-linux-gnu"])
        .arg(dir.join("dist"))
        .output()
        .expect("run make-tarball.sh");
    assert!(
        out.status.success(),
        "make-tarball.sh failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let archive = dir.join("dist/xavier-server-test-x86_64-unknown-linux-gnu.tar.gz");
    let sha = PathBuf::from(format!("{}.sha256", archive.display()));
    (archive, sha)
}

fn run_user_install(home: &Path, archive: &Path, sha: &Path) -> std::process::Output {
    Command::new("sh")
        .arg(packaging_dir().join("install.sh"))
        .args(["--user", "--no-start", "--archive"])
        .arg(archive)
        .arg("--sha256")
        .arg(sha)
        .env("HOME", home)
        // No user bus: the installer must not touch the caller's systemd manager.
        .env("XDG_RUNTIME_DIR", home.join("no-runtime"))
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .output()
        .expect("run install.sh")
}

#[test]
fn installer_rejects_tampered_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let (archive, sha) = fake_release(tmp.path());
    fs::write(
        &sha,
        "0000000000000000000000000000000000000000000000000000000000000000  x.tar.gz\n",
    )
    .unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();

    let out = run_user_install(&home, &archive, &sha);

    assert!(!out.status.success(), "tampered archive was accepted");
    assert!(String::from_utf8_lossy(&out.stderr).contains("checksum mismatch"));
    assert!(!home.join(".local/bin/xavier").exists());
}

#[test]
fn installer_user_mode_installs_binary_and_private_token() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let (archive, sha) = fake_release(tmp.path());
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();

    let out = run_user_install(&home, &archive, &sha);
    assert!(
        out.status.success(),
        "install.sh failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let bin = home.join(".local/bin/xavier");
    let ran = Command::new(&bin).output().expect("run installed binary");
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "xavier-fake");

    let env_file = home.join(".config/xavier/xavier.env");
    let mode = fs::metadata(&env_file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "env file must be private");
    let env = fs::read_to_string(&env_file).unwrap();
    let token = env
        .lines()
        .find_map(|l| l.strip_prefix("XAVIER_TOKEN="))
        .expect("XAVIER_TOKEN line");
    assert_eq!(token.len(), 64, "token must be 32 random bytes in hex");

    // A second run keeps the existing token.
    let again = run_user_install(&home, &archive, &sha);
    assert!(again.status.success());
    assert_eq!(fs::read_to_string(&env_file).unwrap(), env);

    assert!(home.join(".config/systemd/user/xavier.service").exists());
}
