xavier::isolate_test_process!();

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Harness {
    tmp: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        for dir in ["home", "data", "config", "run", "share", "cache", "recovery"] {
            fs::create_dir_all(tmp.path().join(dir)).unwrap();
        }
        Self { tmp }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }

    fn run(&self, args: &[&str]) -> Output {
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
    String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn recovery_status_returns_success_and_valid_output() {
    let h = Harness::new();
    let output = h.run(&["recovery", "status"]);
    let out_text = text(&output);
    assert!(output.status.success(), "{}", out_text);
    assert!(out_text.contains("recovery status"), "Output should contain 'recovery status'");
}

#[test]
fn cloud_backup_verify_requires_passphrase() {
    let h = Harness::new();
    let output = h.run(&["cloud", "verify"]);
    let out_text = text(&output);
    assert!(!output.status.success(), "Should fail without passphrase: {}", out_text);
    assert!(out_text.to_lowercase().contains("passphrase"), "Output should complain about passphrase");
}
