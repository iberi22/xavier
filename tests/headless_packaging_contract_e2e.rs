use std::fs;
use std::process::Command;
use xavier::packaging_contract::{fake_release, run_user_install};

xavier::isolate_test_process!();

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
