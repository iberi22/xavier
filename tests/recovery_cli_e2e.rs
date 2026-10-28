xavier::isolate_test_process!();

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use rand::RngCore;
use xavier::crypto::{hex_decode, hex_encode};
use xavier::memory::{sqlite_vec_store::at_rest, store::MemoryRecord};
use xavier::recovery::{MnemonicPath, RecoveryStore, MNEMONIC_SEAL_FILE, PASSPHRASE_SEAL_FILE};

struct Harness {
    tmp: tempfile::TempDir,
    key: [u8; 32],
    key_bytes: Vec<u8>,
}

impl Harness {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        for dir in [
            "home",
            "data/node",
            "recovery",
            "run",
            "config",
            "share",
            "cache",
        ] {
            fs::create_dir_all(tmp.path().join(dir)).unwrap();
        }
        let mut key = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        let key_bytes = hex_encode(key).into_bytes();
        let h = Self {
            tmp,
            key,
            key_bytes,
        };
        xavier::keystore::create_private_file_new(&h.key_path(), &h.key_bytes).unwrap();
        h
    }

    fn path(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }

    fn key_path(&self) -> PathBuf {
        self.path("data/node/record.key")
    }

    fn run(&self, args: &[&str]) -> Output {
        if args.first() == Some(&"seal") && !args.contains(&"--offline-i-know-the-key-is-live") {
            let mut args = args.to_vec();
            args.push("--offline-i-know-the-key-is-live");
            self.run_with_input(&args, None)
        } else {
            self.run_with_input(args, None)
        }
    }

    fn run_with_input(&self, args: &[&str], input: Option<&str>) -> Output {
        self.run_at(args, input, "http://127.0.0.1:0")
    }

    fn run_at(&self, args: &[&str], input: Option<&str>, url: &str) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xavier"));
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.path("home"))
            .env("XAVIER_CONFIG_PATH", self.path("config/absent.json"))
            .env("XAVIER_RUNTIME_STATE_PATH", self.path("run/absent.json"))
            .env("XAVIER_URL", url)
            .env("XAVIER_TOKEN", "recovery-test-token")
            .env("XAVIER_DATA_DIR", self.path("data"))
            .env("XAVIER_RECOVERY_DIR", self.path("recovery"))
            .env("XDG_RUNTIME_DIR", self.path("run"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_DATA_HOME", self.path("share"))
            .env("XDG_CACHE_HOME", self.path("cache"))
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", self.path("nobus").display()),
            )
            .current_dir(self.tmp.path())
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .args(["recovery"])
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
        let mut child = command.spawn().unwrap();
        if let Some(input) = input {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            !text(&output).contains(&hex_encode(self.key)),
            "key leaked in CLI output"
        );
        for entry in fs::read_dir(self.tmp.path()).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name().to_string_lossy().contains("words") {
                if let Ok(words) = fs::read_to_string(entry.path()) {
                    if words.split_whitespace().count() == 24 {
                        assert!(
                            !text(&output).contains(words.trim()),
                            "mnemonic leaked in CLI output"
                        );
                    }
                }
            }
        }
        assert!(
            !self.path("home/.xavier/master.key").exists(),
            "CLI minted a master key"
        );
        assert!(
            !self.path("data/spaces/default/keystore.json").exists(),
            "test must exercise XRK1, not XDK2"
        );
        output
    }

    fn ok(&self, args: &[&str]) -> Output {
        let output = self.run(args);
        assert!(output.status.success(), "CLI failed: {}", text(&output));
        output
    }

    fn fail(&self, args: &[&str]) -> Output {
        let before = fs::read(self.key_path()).ok();
        let output = self.run(args);
        assert!(
            !output.status.success(),
            "CLI unexpectedly succeeded: {}",
            text(&output)
        );
        assert_eq!(
            fs::read(self.key_path()).ok(),
            before,
            "failed command changed live key"
        );
        self.no_candidates();
        output
    }

    fn seal_words(&self) -> PathBuf {
        let words = self.path("words.txt");
        let output = self.ok(&["seal", "--mnemonic", "--words-out", arg(&words)]);
        let secret = fs::read_to_string(&words).unwrap();
        assert_eq!(secret.split_whitespace().count(), 24);
        assert!(secret.ends_with('\n'));
        assert!(
            !text(&output).contains(secret.trim()),
            "mnemonic leaked in output"
        );
        mode_0600(&words);
        words
    }

    fn encrypted_record(&self) -> (MemoryRecord, String) {
        let content = format!("secreto familiar {}", uuid::Uuid::new_v4());
        let mut record = MemoryRecord {
            id: uuid::Uuid::new_v4().to_string(),
            workspace_id: "default".into(),
            path: "private/family.md".into(),
            content: content.clone(),
            metadata: serde_json::json!({"private": true}),
            ..Default::default()
        };
        at_rest::encrypt_columns_for_write_with_key(&mut record, &self.key).unwrap();
        assert!(record.encrypted_dek.as_ref().unwrap().starts_with(b"XRK1"));
        assert_ne!(record.content, content);
        (record, content)
    }

    fn readable(&self, mut record: MemoryRecord, content: &str) {
        assert_eq!(fs::read(self.key_path()).unwrap(), self.key_bytes);
        mode_0600(&self.key_path());
        let bytes = hex_decode(fs::read_to_string(self.key_path()).unwrap().trim()).unwrap();
        let key: [u8; 32] = bytes.try_into().unwrap();
        at_rest::decrypt_record_with_key(&mut record, &key).unwrap();
        assert_eq!(record.content, content);
        self.no_candidates();
    }

    fn no_candidates(&self) {
        for dir in ["data/node", "recovery"] {
            for entry in fs::read_dir(self.path(dir)).unwrap() {
                assert!(!entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".candidate"));
            }
        }
    }
}

fn arg(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn mode_0600(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[test]
#[serial_test::serial]
fn mnemonic_cli_roundtrip_status_and_idempotent_restore() {
    let h = Harness::new();
    let (record, content) = h.encrypted_record();
    let words = h.seal_words();
    let status = text(&h.ok(&["status"]));
    assert!(
        status.contains("mnemonic seal   : valid (0600)"),
        "{status}"
    );
    assert!(status.contains("matches live key"), "{status}");
    fs::remove_file(h.key_path()).unwrap();
    let status = text(&h.ok(&["status"]));
    assert!(status.contains("MISSING"), "{status}");
    assert!(
        status.contains("live key missing (restore possible)"),
        "{status}"
    );
    assert!(!h.key_path().exists(), "status minted record.key");
    let restored = h.ok(&["unseal", "--mnemonic", "--words-file", arg(&words)]);
    assert!(text(&restored).to_lowercase().contains("installed"));
    h.readable(record, &content);
    let again = h.fail(&["unseal", "--mnemonic", "--words-file", arg(&words)]);
    assert!(text(&again).contains("target_exists"));
    h.fail(&["restore", "--key-file", arg(&h.key_path())]);
    assert_eq!(fs::read(h.key_path()).unwrap(), h.key_bytes);
}

#[test]
#[serial_test::serial]
fn passphrase_cli_roundtrip_crlf_and_lone_cr() {
    let h = Harness::new();
    let (record, content) = h.encrypted_record();
    let pass = h.path("pass.txt");
    fs::write(&pass, "frase larga de prueba 123\r\n").unwrap();
    h.ok(&["seal", "--passphrase", "--passphrase-file", arg(&pass)]);
    fs::remove_file(h.key_path()).unwrap();
    fs::write(&pass, "frase larga de prueba 123\r").unwrap();
    let restored = h.ok(&["unseal", "--passphrase", "--passphrase-file", arg(&pass)]);
    assert!(text(&restored).to_lowercase().contains("installed"));
    h.readable(record, &content);
}

#[test]
#[serial_test::serial]
fn wrong_secrets_and_tampered_seal_are_non_destructive() {
    let h = Harness::new();
    let words = h.seal_words();
    let pass = h.path("pass.txt");
    fs::write(&pass, "frase larga de prueba 123").unwrap();
    h.ok(&["seal", "--passphrase", "--passphrase-file", arg(&pass)]);
    fs::remove_file(h.key_path()).unwrap();
    let wrong = h.path("wrong.txt");
    fs::write(&wrong, MnemonicPath::generate().unwrap().0).unwrap();
    h.fail(&["unseal", "--mnemonic", "--words-file", arg(&wrong)]);
    fs::write(&wrong, "una frase incorrecta bastante larga").unwrap();
    h.fail(&["unseal", "--passphrase", "--passphrase-file", arg(&wrong)]);
    let seal = h.path(&format!("recovery/{MNEMONIC_SEAL_FILE}"));
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&seal).unwrap()).unwrap();
    let ciphertext = json["ciphertext"].as_str().unwrap();
    let flipped = format!(
        "{}{}",
        if ciphertext.starts_with('0') {
            '1'
        } else {
            '0'
        },
        &ciphertext[1..]
    );
    json["ciphertext"] = flipped.into();
    fs::write(&seal, serde_json::to_vec(&json).unwrap()).unwrap();
    h.fail(&["unseal", "--mnemonic", "--words-file", arg(&words)]);
    fs::write(&seal, b"{\"salt\":").unwrap();
    let status = text(&h.ok(&["status"]));
    assert!(status.contains("MALFORMED (json_invalid)"), "{status}");
    assert!(!h.key_path().exists());
}

#[test]
#[serial_test::serial]
fn replaced_kcv_fails_unseal_and_reports_mismatch() {
    let h = Harness::new();
    let words = h.seal_words();
    let mut other = [0; 32];
    rand::rngs::OsRng.fill_bytes(&mut other);
    RecoveryStore::at(h.path("recovery"))
        .write_kcv(&other)
        .unwrap();
    let status = text(&h.ok(&["status"]));
    assert!(status.contains("MISMATCH"), "{status}");
    fs::remove_file(h.key_path()).unwrap();
    h.fail(&["unseal", "--mnemonic", "--words-file", arg(&words)]);
}

#[test]
#[serial_test::serial]
fn seal_twice_and_missing_key_are_refused() {
    let h = Harness::new();
    h.seal_words();
    let seal = h.path(&format!("recovery/{MNEMONIC_SEAL_FILE}"));
    let before = fs::read(&seal).unwrap();
    let next_words = h.path("next-words.txt");
    h.fail(&["seal", "--mnemonic", "--words-out", arg(&next_words)]);
    assert_eq!(fs::read(&seal).unwrap(), before);
    assert!(!next_words.exists());
    fs::remove_file(h.key_path()).unwrap();
    h.fail(&[
        "seal",
        "--mnemonic",
        "--words-out",
        arg(&next_words),
        "--replace-mnemonic",
    ]);
    assert!(!h.key_path().exists());
    assert_eq!(fs::read(&seal).unwrap(), before);
}

#[test]
#[serial_test::serial]
fn short_passphrase_writes_no_recovery_artifacts() {
    let h = Harness::new();
    let pass = h.path("short.txt");
    fs::write(&pass, "short\r\n").unwrap();
    h.fail(&["seal", "--passphrase", "--passphrase-file", arg(&pass)]);
    assert_eq!(fs::read_dir(h.path("recovery")).unwrap().count(), 0);
    assert!(!h.path(&format!("recovery/{PASSPHRASE_SEAL_FILE}")).exists());
}

#[test]
#[serial_test::serial]
fn unseal_out_refuses_existing_files_and_symlinks() {
    let h = Harness::new();
    let words = h.seal_words();
    let out = h.path("out.key");
    fs::write(&out, b"do not overwrite").unwrap();
    h.fail(&[
        "unseal",
        "--mnemonic",
        "--words-file",
        arg(&words),
        "--out",
        arg(&out),
    ]);
    assert_eq!(fs::read(&out).unwrap(), b"do not overwrite");
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let dangling = h.path("dangling.key");
        let absent = h.path("absent.key");
        symlink(&absent, &dangling).unwrap();
        h.fail(&[
            "unseal",
            "--mnemonic",
            "--words-file",
            arg(&words),
            "--out",
            arg(&dangling),
        ]);
        assert!(!absent.exists());
        assert_eq!(fs::read_link(&dangling).unwrap(), absent);
        let link = h.path("linked.key");
        symlink(&out, &link).unwrap();
        h.fail(&[
            "unseal",
            "--mnemonic",
            "--words-file",
            arg(&words),
            "--out",
            arg(&link),
        ]);
        assert_eq!(fs::read_link(&link).unwrap(), out);
        assert_eq!(fs::read(&out).unwrap(), b"do not overwrite");
    }
}

#[test]
#[serial_test::serial]
fn explicit_replacement_changes_seals_and_preserves_live_key() {
    let h = Harness::new();
    let first_words = h.seal_words();
    let mnemonic = h.path(&format!("recovery/{MNEMONIC_SEAL_FILE}"));
    let before_mnemonic = fs::read(&mnemonic).unwrap();
    let new_words = h.path("replacement-words.txt");
    h.ok(&[
        "seal",
        "--mnemonic",
        "--words-out",
        arg(&new_words),
        "--replace-mnemonic",
    ]);
    assert_ne!(fs::read(&mnemonic).unwrap(), before_mnemonic);
    assert_ne!(
        fs::read(&first_words).unwrap(),
        fs::read(&new_words).unwrap()
    );
    mode_0600(&new_words);

    let pass = h.path("pass.txt");
    fs::write(&pass, "primera frase larga de prueba").unwrap();
    h.ok(&["seal", "--passphrase", "--passphrase-file", arg(&pass)]);
    let passphrase = h.path(&format!("recovery/{PASSPHRASE_SEAL_FILE}"));
    let before_passphrase = fs::read(&passphrase).unwrap();
    fs::write(&pass, "segunda frase larga de prueba").unwrap();
    h.fail(&["seal", "--passphrase", "--passphrase-file", arg(&pass)]);
    assert_eq!(fs::read(&passphrase).unwrap(), before_passphrase);
    h.ok(&[
        "seal",
        "--passphrase",
        "--passphrase-file",
        arg(&pass),
        "--replace-passphrase",
    ]);
    assert_ne!(fs::read(&passphrase).unwrap(), before_passphrase);
    assert_eq!(fs::read(h.key_path()).unwrap(), h.key_bytes);
    fs::remove_file(h.key_path()).unwrap();
    h.fail(&["unseal", "--mnemonic", "--words-file", arg(&first_words)]);
    h.ok(&["unseal", "--mnemonic", "--words-file", arg(&new_words)]);
    assert_eq!(fs::read(h.key_path()).unwrap(), h.key_bytes);
    fs::remove_file(h.key_path()).unwrap();
    h.ok(&["unseal", "--passphrase", "--passphrase-file", arg(&pass)]);
    assert_eq!(fs::read(h.key_path()).unwrap(), h.key_bytes);
    h.no_candidates();
}

#[test]
#[serial_test::serial]
fn mismatched_kcv_requires_explicit_rekey_before_sealing() {
    let h = Harness::new();
    h.seal_words();
    let mut other = [0; 32];
    rand::rngs::OsRng.fill_bytes(&mut other);
    RecoveryStore::at(h.path("recovery"))
        .write_kcv(&other)
        .unwrap();
    let kcv = h.path("recovery/record.key.kcv");
    let before_kcv = fs::read(&kcv).unwrap();
    let pass = h.path("pass.txt");
    fs::write(&pass, "frase larga de prueba para rekey").unwrap();
    h.fail(&["seal", "--passphrase", "--passphrase-file", arg(&pass)]);
    assert_eq!(fs::read(&kcv).unwrap(), before_kcv);
    assert!(!h.path(&format!("recovery/{PASSPHRASE_SEAL_FILE}")).exists());
    h.ok(&[
        "seal",
        "--passphrase",
        "--passphrase-file",
        arg(&pass),
        "--rekey",
    ]);
    assert_ne!(fs::read(&kcv).unwrap(), before_kcv);
    let status = text(&h.ok(&["status"]));
    assert!(status.contains("matches live key"), "{status}");
    assert_eq!(fs::read(h.key_path()).unwrap(), h.key_bytes);
}

#[test]
#[serial_test::serial]
fn words_out_refuses_existing_files_and_symlinks_without_installing_seal() {
    let h = Harness::new();
    let out = h.path("protected-words.txt");
    fs::write(&out, b"preserve this file").unwrap();
    h.fail(&["seal", "--mnemonic", "--words-out", arg(&out)]);
    assert_eq!(fs::read(&out).unwrap(), b"preserve this file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let absent = h.path("absent-words.txt");
        let dangling = h.path("dangling-words.txt");
        symlink(&absent, &dangling).unwrap();
        h.fail(&["seal", "--mnemonic", "--words-out", arg(&dangling)]);
        assert!(!absent.exists());
        assert_eq!(fs::read_link(&dangling).unwrap(), absent);
        let linked = h.path("linked-words.txt");
        symlink(&out, &linked).unwrap();
        h.fail(&["seal", "--mnemonic", "--words-out", arg(&linked)]);
        assert_eq!(fs::read_link(&linked).unwrap(), out);
        assert_eq!(fs::read(&out).unwrap(), b"preserve this file");
    }
    assert_eq!(fs::read_dir(h.path("recovery")).unwrap().count(), 0);
}

#[test]
#[serial_test::serial]
fn failed_seal_installation_leaves_no_words_file_or_sidecars() {
    let h = Harness::new();
    // A directory at the seal destination makes the private-file installation fail.
    let seal = h.path(&format!("recovery/{MNEMONIC_SEAL_FILE}"));
    fs::create_dir(&seal).unwrap();
    let words = h.path("uninstalled-words.txt");
    h.fail(&[
        "seal",
        "--mnemonic",
        "--words-out",
        arg(&words),
        "--replace-mnemonic",
    ]);
    assert!(
        !words.exists(),
        "failed seal installation retained its words file"
    );
    assert!(seal.is_dir());
    assert_eq!(fs::read_dir(h.path("recovery")).unwrap().count(), 1);
}

#[test]
#[serial_test::serial]
fn mnemonic_sealing_requires_private_delivery_without_tty() {
    let h = Harness::new();
    let output = h.fail(&["seal", "--mnemonic"]);
    assert!(text(&output).contains("--words-out"));
    assert_eq!(fs::read_dir(h.path("recovery")).unwrap().count(), 0);
    assert!(!text(&output).contains("WRITE THESE"));
}

#[test]
#[serial_test::serial]
fn seal_refuses_missing_words_delivery_before_passphrase_prompt() {
    let h = Harness::new();
    let output = h.fail(&["seal", "--mnemonic", "--passphrase"]);
    let out = text(&output);
    assert!(out.contains("--words-out"), "{out}");
    assert!(!out.contains("cannot read passphrase"), "{out}");
    assert_eq!(fs::read_dir(h.path("recovery")).unwrap().count(), 0);
}

#[test]
#[serial_test::serial]
fn restore_reads_key_file_or_stdin_without_printing_key() {
    let h = Harness::new();
    let (record, content) = h.encrypted_record();
    RecoveryStore::at(h.path("recovery"))
        .write_kcv(&h.key)
        .unwrap();
    let input = h.path("input.key");
    fs::write(&input, format!("{}\r\n", hex_encode(h.key))).unwrap();
    fs::remove_file(h.key_path()).unwrap();
    h.ok(&["restore", "--key-file", arg(&input)]);
    h.readable(record.clone(), &content);
    fs::remove_file(h.key_path()).unwrap();
    let output = h.run_with_input(
        &["restore", "--key-file", "-"],
        Some(&format!("{}\n", hex_encode(h.key))),
    );
    assert!(output.status.success(), "{}", text(&output));
    h.readable(record, &content);
    h.fail(&["restore", "--key-hex", &hex_encode(h.key)]);
}

#[test]
#[serial_test::serial]
fn restore_requires_kcv_before_installing_key() {
    let h = Harness::new();
    let input = h.path("input.key");
    fs::write(&input, &h.key_bytes).unwrap();
    fs::remove_file(h.key_path()).unwrap();
    let output = h.fail(&["restore", "--key-file", arg(&input)]);
    assert!(text(&output).to_lowercase().contains("kcv"));
    assert!(!h.key_path().exists());
    assert_eq!(fs::read_dir(h.path("recovery")).unwrap().count(), 0);
}

fn expected_fingerprint(key: &[u8; 32]) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"xavier-record-key-fingerprint-v1\0");
    hash.update(key);
    hex_encode(hash.finalize())[..12].to_string()
}

#[test]
fn recovery_status_json_has_stable_safe_fingerprint_and_schema() {
    let h = Harness::new();
    for _ in 0..2 {
        let output = h.ok(&["status", "--json"]);
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["recovery_dir"], arg(&h.path("recovery")));
        assert_eq!(value["key_source"]["kind"], "file");
        assert_eq!(value["key_source"]["path"], arg(&h.key_path()));
        assert_eq!(value["key_path"], value["key_source"]["path"]);
        assert_eq!(value["seals"].as_array().unwrap().len(), 2);
        assert_eq!(value["seals"][0]["kind"], "mnemonic");
        assert_eq!(value["seals"][1]["kind"], "passphrase");
        assert_eq!(value["seals"][0]["valid"], false);
        assert_eq!(value["key_fingerprint"], expected_fingerprint(&h.key));
        assert_ne!(value["key_fingerprint"], hex_encode(h.key));
        for field in ["manifest", "kcv", "mnemonic_seal", "passphrase_seal"] {
            assert_eq!(value[field]["present"], false, "{field}");
            assert_eq!(value[field]["valid"], false, "{field}");
        }
        assert_eq!(value["crypt_passphrase_backed_up"], false);
        assert_eq!(value["recoverable"], false);
        assert!(value["warnings"].is_array());
    }
}

#[test]
fn recovery_seal_checks_authenticated_daemon_fingerprint_before_writing() {
    let h = Harness::new();
    let words = h.path("checked-words.txt");
    let args = ["seal", "--mnemonic", "--words-out", arg(&words)];
    let mut server = mockito::Server::new();
    let mismatch = server
        .mock("GET", "/admin/recovery/key")
        .match_header("authorization", "Bearer recovery-test-token")
        .with_status(200)
        .with_body(r#"{"key_fingerprint":"000000000000","key_path":"/daemon/node/record.key"}"#)
        .create();
    let output = h.run_at(&args, None, &server.url());
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(
        text(&output).contains("does not match"),
        "{}",
        text(&output)
    );
    assert!(!words.exists());
    assert_eq!(fs::read_dir(h.path("recovery")).unwrap().count(), 0);
    mismatch.assert();
    mismatch.remove();
    let matched = server.mock("GET", "/admin/recovery/key")
        .match_header("authorization", "Bearer recovery-test-token")
        .with_status(200)
        .with_body(serde_json::json!({"key_fingerprint": expected_fingerprint(&h.key), "key_path": "/daemon/node/record.key"}).to_string())
        .create();
    let output = h.run_at(&args, None, &server.url());
    assert!(output.status.success(), "{}", text(&output));
    matched.assert();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(arg(&h.key_path())));
    assert!(stderr.contains(&expected_fingerprint(&h.key)));
    assert_eq!(
        RecoveryStore::at(h.path("recovery"))
            .unseal_mnemonic(fs::read_to_string(words).unwrap().trim())
            .unwrap(),
        h.key
    );
}

#[test]
fn recovery_seal_offline_requires_explicit_acknowledgement() {
    let h = Harness::new();
    let words = h.path("offline-words.txt");
    let output = h.run_at(
        &["seal", "--mnemonic", "--words-out", arg(&words)],
        None,
        "http://127.0.0.1:0",
    );
    assert_eq!(output.status.code(), Some(4), "{}", text(&output));
    assert!(text(&output).contains("--offline-i-know-the-key-is-live"));
    assert!(!words.exists());
    h.ok(&[
        "seal",
        "--mnemonic",
        "--words-out",
        arg(&words),
        "--offline-i-know-the-key-is-live",
    ]);
}

#[test]
fn recovery_seal_non_tty_requires_private_input_and_output_flags() {
    let h = Harness::new();
    for (args, hint) in [
        (vec!["seal", "--mnemonic"], "--words-out"),
        (vec!["seal", "--passphrase"], "--passphrase-file"),
    ] {
        let output = h.run(&args);
        assert_eq!(output.status.code(), Some(2), "{}", text(&output));
        assert!(text(&output).contains(hint));
    }
    assert_eq!(fs::read_dir(h.path("recovery")).unwrap().count(), 0);
}

#[test]
fn recovery_seal_offline_flag_does_not_bypass_daemon_failures() {
    let h = Harness::new();
    let words = h.path("refused-words.txt");
    let args = [
        "seal",
        "--mnemonic",
        "--words-out",
        arg(&words),
        "--offline-i-know-the-key-is-live",
    ];
    let mut server = mockito::Server::new();
    for (status, body, exit) in [
        (401, "rejected", 3),
        (403, "rejected", 3),
        (404, "not supported", 1),
        (500, "failed", 1),
        (302, "redirect", 1),
        (200, "malformed", 1),
        (200, r#"{"key_fingerprint":null,"key_path":null}"#, 1),
        (
            200,
            r#"{"key_fingerprint":"000000000000","key_path":null}"#,
            1,
        ),
    ] {
        let mock = server
            .mock("GET", "/admin/recovery/key")
            .match_header("authorization", "Bearer recovery-test-token")
            .with_status(status)
            .with_body(body)
            .with_header("location", "http://127.0.0.1:0/unused")
            .create();
        let output = h.run_at(&args, None, &server.url());
        assert_eq!(
            output.status.code(),
            Some(exit),
            "{status}: {}",
            text(&output)
        );
        assert!(!words.exists());
        assert_eq!(fs::read_dir(h.path("recovery")).unwrap().count(), 0);
        assert_eq!(fs::read(h.key_path()).unwrap(), h.key_bytes);
        mock.assert();
        mock.remove();
    }
}

#[test]
fn recovery_status_json_tracks_seals_missing_key_and_malformed_files() {
    let h = Harness::new();
    h.seal_words();
    let json = || {
        serde_json::from_slice::<serde_json::Value>(
            &h.ok(&["status", "--json", "--crypt-passphrase-backed-up"])
                .stdout,
        )
        .unwrap()
    };
    let value = json();
    assert_eq!(value["mnemonic_seal"]["present"], true);
    assert_eq!(value["mnemonic_seal"]["valid"], true);
    assert_eq!(value["kcv"]["state"], "matches_live_key");
    assert_eq!(value["manifest"]["valid"], true);
    assert_eq!(value["recoverable"], true);
    fs::remove_file(h.key_path()).unwrap();
    let value = json();
    assert_eq!(value["key_source"]["kind"], "missing");
    assert!(value["key_fingerprint"].is_null());
    assert_eq!(value["kcv"]["state"], "live_key_missing");
    assert!(!h.key_path().exists());
    fs::write(h.key_path(), b"malformed test key").unwrap();
    let value = json();
    assert_eq!(value["key_source"]["kind"], "unavailable");
    assert_eq!(value["key_source"]["path"], arg(&h.key_path()));
    assert!(value["key_fingerprint"].is_null());
    fs::write(h.path("recovery/recovery-manifest.json"), b"{").unwrap();
    fs::write(h.path(&format!("recovery/{MNEMONIC_SEAL_FILE}")), b"{").unwrap();
    let value = json();
    assert_eq!(value["manifest"]["present"], true);
    assert_eq!(value["manifest"]["valid"], false);
    assert_eq!(value["mnemonic_seal"]["present"], true);
    assert_eq!(value["mnemonic_seal"]["valid"], false);
    assert_eq!(value["recoverable"], false);
    assert!(value["warnings"].as_array().unwrap().len() > 1);
}
