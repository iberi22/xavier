xavier::isolate_test_process!();

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Harness {
    tmp: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        for dir in [
            "home", "data", "config", "run", "share", "cache", "recovery",
        ] {
            fs::create_dir_all(tmp.path().join(dir)).unwrap();
        }
        Self { tmp }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_env(args, &[])
    }

    fn run_env(&self, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xavier"));
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.path("home"))
            .env("XAVIER_CONFIG_PATH", self.path("config/absent.json"))
            .env("XAVIER_RUNTIME_STATE_PATH", self.path("run/absent.json"))
            .env("XAVIER_DATA_DIR", self.path("data"))
            .env("XAVIER_RECOVERY_DIR", self.path("recovery"))
            .env("XDG_RUNTIME_DIR", self.path("run"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_DATA_HOME", self.path("share"))
            .env("XDG_CACHE_HOME", self.path("cache"))
            .env("XAVIER_URL", "http://127.0.0.1:0")
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", self.path("nobus").display()),
            )
            .current_dir(self.tmp.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .args(args);
        for (k, v) in extra_env {
            command.env(k, v);
        }

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }

        command.output().unwrap()
    }
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr)
}

#[test]
fn recovery_status_reports_unrecoverable_fresh_node_honestly() {
    let h = Harness::new();
    let output = h.run(&["recovery", "status", "--json"]);
    let out_text = text(&output);
    assert!(output.status.success(), "{}", out_text);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON object in output: {out_text}"));
    let v: serde_json::Value = serde_json::from_str(json_line).unwrap();

    // No record key exists on a fresh node, and status must not invent one.
    assert_eq!(v["key_source"]["kind"], "missing", "{v}");
    assert!(v["key_fingerprint"].is_null(), "{v}");
    // No recovery artifacts exist, so recovery must be reported as impossible.
    assert_eq!(v["manifest"]["present"], false, "{v}");
    assert_eq!(v["kcv"]["present"], false, "{v}");
    assert_eq!(v["mnemonic_seal"]["present"], false, "{v}");
    assert_eq!(v["passphrase_seal"]["present"], false, "{v}");
    assert_eq!(v["recoverable"], false, "{v}");
    let warnings: Vec<String> = v["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .filter_map(|w| w.as_str().map(str::to_owned))
        .collect();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("no recovery seal exists")),
        "{warnings:?}"
    );
    assert!(
        warnings.iter().any(|w| w.contains("no KCV")),
        "{warnings:?}"
    );

    // A read-only status call must not generate keys or recovery material.
    let recovery_entries: Vec<_> = fs::read_dir(h.path("recovery")).unwrap().collect();
    assert!(recovery_entries.is_empty(), "status wrote recovery files");
    let key_path = v["key_source"]["path"].as_str().expect("key path");
    assert!(
        !std::path::Path::new(key_path).exists(),
        "status generated a record key at {key_path}"
    );
}

#[test]
fn cloud_verify_without_cloud_config_fails_clearly() {
    let h = Harness::new();
    let output = h.run(&["cloud", "verify"]);
    let out_text = text(&output);
    assert!(
        !output.status.success(),
        "Should fail without cloud config: {}",
        out_text
    );
    assert!(
        out_text.contains("Xavier Cloud is not configured"),
        "Output should say cloud is not configured: {}",
        out_text
    );
}

#[test]
fn cloud_backup_deep_verify_requires_passphrase() {
    let h = Harness::new();
    // Configured Worker on an unroutable port (port 0 fails immediately, so
    // the health probe cannot hang); --deep must then refuse with the precise
    // missing-passphrase diagnostic.
    let output = h.run_env(
        &["cloud", "verify", "--deep"],
        &[
            ("PGHEART_URL", "http://127.0.0.1:0"),
            ("PGHEART_TOKEN", "test-token"),
            ("PGHEART_INSTANCE_ID", "e2e-instance"),
        ],
    );
    let out_text = text(&output);
    assert!(
        !output.status.success(),
        "Should fail without passphrase: {}",
        out_text
    );
    assert!(
        out_text.contains("no cloud backup passphrase"),
        "Output should name the missing cloud backup passphrase: {}",
        out_text
    );
}
