use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;
use zip::write::SimpleFileOptions;

xavier::isolate_test_process!();

// Compile the capsule engine locally so creation remains test-only.
mod crypto {
    pub use xavier::crypto::{hex_decode, hex_encode};
}
#[allow(dead_code)]
#[path = "../src/crypto/airgap_capsule.rs"]
mod capsule_engine;
use capsule_engine::{AirgapCapsule, CapsulePayloadKind};

const PASS: &str = "airgap test passphrase";
const DATA: &[u8] = b"capsule test data";

struct Harness(TempDir);
impl Harness {
    fn new() -> Self {
        let h = Self(tempfile::tempdir().unwrap());
        for dir in ["home", "data/node", "out"] {
            fs::create_dir_all(h.path(dir)).unwrap();
        }
        fs::write(h.path("pass.txt"), format!("{PASS}\r\n")).unwrap();
        fs::write(h.path("data/node/record.key"), b"preserve record").unwrap();
        h
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.path().join(name)
    }
    fn capsule(&self, name: &str, kind: CapsulePayloadKind, data: &[u8]) {
        let bytes =
            AirgapCapsule::pack_with_passphrase(data, kind, name.into(), "test".into(), PASS)
                .unwrap();
        fs::write(self.path("sample.swal_capsule"), bytes).unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.path("home"))
            .env("XAVIER_DATA_DIR", self.path("data"))
            .env("XAVIER_LOG_DIR", self.path("logs"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_DATA_HOME", self.path("share"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_CACHE_HOME", self.path("cache"))
            .env("XDG_RUNTIME_DIR", self.path("run"))
            .current_dir(self.0.path())
            .stdin(Stdio::null())
            .args(["airgap"])
            .args(args)
            .output()
            .unwrap();
        assert_eq!(
            fs::read(self.path("data/node/record.key")).unwrap(),
            b"preserve record"
        );
        assert!(!self.path("home/.xavier/master.key").exists());
        output
    }
    fn unpack(&self, flag: &str, path: &str, success: bool) -> Output {
        let output = self.run(&[
            "unpack",
            "--capsule",
            "sample.swal_capsule",
            flag,
            path,
            "--passphrase-file",
            "pass.txt",
        ]);
        assert_eq!(output.status.success(), success, "{}", text(&output));
        output
    }
}
fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn pack_is_disabled() {
    let h = Harness::new();
    let out = h.run(&[
        "pack",
        "--input",
        "pass.txt",
        "--output",
        "new.swal_capsule",
    ]);
    assert!(!out.status.success());
    assert!(text(&out).contains("capsule creation is disabled pending a format redesign"));
    assert!(!h.path("new.swal_capsule").exists());
}

#[test]
fn header_names_do_not_select_output_paths() {
    let h = Harness::new();
    let names = [
        h.path("absolute.txt").to_string_lossy().into_owned(),
        "../outside.txt".into(),
        "record.key".into(),
        "data/node/record.key".into(),
    ];
    for (i, name) in names.iter().enumerate() {
        h.capsule(name, CapsulePayloadKind::SingleFile, DATA);
        let selected = format!("out/selected-{i}.txt");
        h.unpack("--output", &selected, true);
        assert_eq!(fs::read(h.path(&selected)).unwrap(), DATA);
    }
    assert!(!h.path("absolute.txt").exists());
    assert!(!h.path("record.key").exists());
    assert!(!h.path("outside.txt").exists());
    assert_eq!(fs::read_dir(h.path("out")).unwrap().count(), 4);
}

#[test]
fn default_output_uses_capsule_path_stem() {
    let h = Harness::new();
    h.capsule("record.key", CapsulePayloadKind::SingleFile, DATA);
    h.unpack("--output-dir", "out", true);
    assert_eq!(fs::read(h.path("out/sample.out")).unwrap(), DATA);
    assert!(!h.path("out/record.key").exists());
}

#[test]
fn existing_output_is_preserved() {
    let h = Harness::new();
    h.capsule("source", CapsulePayloadKind::SingleFile, DATA);
    fs::write(h.path("out/existing.txt"), b"preserve output").unwrap();
    h.unpack("--output", "out/existing.txt", false);
    assert_eq!(
        fs::read(h.path("out/existing.txt")).unwrap(),
        b"preserve output"
    );
}

#[test]
#[cfg(unix)]
fn symlink_output_and_parent_are_refused() {
    use std::os::unix::fs::symlink;
    let h = Harness::new();
    h.capsule("source", CapsulePayloadKind::SingleFile, DATA);
    symlink(h.path("absent.txt"), h.path("out/link.txt")).unwrap();
    h.unpack("--output", "out/link.txt", false);
    assert!(!h.path("absent.txt").exists());
    symlink(h.path("out"), h.path("linked-dir")).unwrap();
    h.unpack("--output", "linked-dir/new.txt", false);
    assert!(!h.path("out/new.txt").exists());
}

#[test]
fn output_paths_are_validated() {
    let h = Harness::new();
    h.capsule("source", CapsulePayloadKind::SingleFile, DATA);
    for name in [
        "out/record.key",
        "out/master.key",
        "data/node/new.txt",
        "out/../new.txt",
    ] {
        h.unpack("--output", name, false);
        assert!(!h.path(name).exists());
    }
    h.unpack("--output", h.path("absolute.txt").to_str().unwrap(), false);
    fs::create_dir(h.path("home/.xavier")).unwrap();
    h.unpack("--output", "home/.xavier/new.txt", false);
    assert!(!h.path("home/.xavier/new.txt").exists());
}

#[test]
fn directory_output_must_be_new() {
    let h = Harness::new();
    h.capsule(
        "directory",
        CapsulePayloadKind::DirectoryArchive,
        &zip(&[("nested/file.txt", DATA)]),
    );
    h.unpack("--output-dir", "out", false);
    assert_eq!(fs::read_dir(h.path("out")).unwrap().count(), 0);
    h.unpack("--output-dir", "new-directory", true);
    assert_eq!(
        fs::read(h.path("new-directory/nested/file.txt")).unwrap(),
        DATA
    );
}

#[test]
fn archive_output_paths_fail_closed() {
    let h = Harness::new();
    for name in [
        "../outside.txt",
        "/absolute.txt",
        "nested/record.key",
        "master.key",
    ] {
        h.capsule(
            "directory",
            CapsulePayloadKind::DirectoryArchive,
            &zip(&[("first.txt", DATA), (name, DATA)]),
        );
        h.unpack("--output-dir", "new-directory", false);
        assert!(!h.path("new-directory").exists());
        assert!(!h.path("outside.txt").exists());
    }
}

#[test]
fn archive_symlink_entries_fail_closed() {
    let h = Harness::new();
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink("link", "target", SimpleFileOptions::default())
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    h.capsule("directory", CapsulePayloadKind::DirectoryArchive, &bytes);
    h.unpack("--output-dir", "new-directory", false);
    assert!(!h.path("new-directory").exists());
}

#[test]
fn authentication_finishes_before_output_creation() {
    let h = Harness::new();
    h.capsule("source", CapsulePayloadKind::SingleFile, DATA);
    fs::write(h.path("pass.txt"), "different passphrase").unwrap();
    h.unpack("--output", "out/new.txt", false);
    assert!(!h.path("out/new.txt").exists());
    h.capsule(
        "directory",
        CapsulePayloadKind::DirectoryArchive,
        &zip(&[("file.txt", DATA)]),
    );
    h.unpack("--output-dir", "new-directory", false);
    assert!(!h.path("new-directory").exists());
}

#[test]
fn passphrases_on_argv_are_refused() {
    let h = Harness::new();
    for flag in ["--passphrase", "-p"] {
        let output = h.run(&[
            "unpack",
            "--capsule",
            "absent",
            "--output",
            "out/file",
            flag,
            PASS,
        ]);
        assert!(!output.status.success());
        assert!(text(&output).contains("unexpected argument"));
    }
}

#[test]
fn inspect_validates_salt_and_nonce() {
    let h = Harness::new();
    h.capsule("source", CapsulePayloadKind::SingleFile, DATA);
    let original = fs::read(h.path("sample.swal_capsule")).unwrap();
    let len = u32::from_le_bytes(original[8..12].try_into().unwrap()) as usize;
    let header: serde_json::Value = serde_json::from_slice(&original[12..12 + len]).unwrap();
    for field in ["salt", "nonce"] {
        let mut short = header.clone();
        short[field] = "00".into();
        let json = serde_json::to_vec(&short).unwrap();
        let mut bytes = original[..8].to_vec();
        bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&json);
        bytes.extend_from_slice(&original[12 + len..]);
        fs::write(h.path("sample.swal_capsule"), bytes).unwrap();
        let output = h.run(&["inspect", "--capsule", "sample.swal_capsule"]);
        assert!(!output.status.success());
        assert!(text(&output).contains("Invalid salt or nonce dimensions"));
        assert!(!text(&output).contains("panicked"));
    }
}

#[test]
fn inspect_reports_valid_metadata() {
    let h = Harness::new();
    h.capsule("line\nname", CapsulePayloadKind::SingleFile, DATA);
    let output = h.run(&["inspect", "--capsule", "sample.swal_capsule"]);
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("line\\nname"));
    let output = h.run(&["inspect", "--capsule", "sample.swal_capsule", "--json"]);
    assert!(output.status.success(), "{}", text(&output));
}

#[test]
fn tampered_ciphertext_writes_nothing() {
    let h = Harness::new();
    h.capsule("source", CapsulePayloadKind::SingleFile, DATA);
    let mut bytes = fs::read(h.path("sample.swal_capsule")).unwrap();
    let header_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    bytes[12 + header_len + 1] ^= 0x01;
    fs::write(h.path("sample.swal_capsule"), bytes).unwrap();
    let output = h.unpack("--output", "out/new.txt", false);
    assert!(text(&output).contains("Decryption failed"));
    assert!(!h.path("out/new.txt").exists());
    assert_eq!(fs::read_dir(h.path("out")).unwrap().count(), 0);
}

#[test]
fn protected_key_names_are_refused_in_any_case() {
    let h = Harness::new();
    h.capsule("source", CapsulePayloadKind::SingleFile, DATA);
    for name in ["out/RECORD.KEY", "out/Master.Key"] {
        h.unpack("--output", name, false);
        assert!(!h.path(name).exists());
    }
    assert_eq!(fs::read_dir(h.path("out")).unwrap().count(), 0);
}

#[test]
fn archive_failure_mid_extraction_removes_new_directory() {
    let h = Harness::new();
    let second: &[u8] = b"second entry payload";
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("first.txt", stored).unwrap();
    writer.write_all(DATA).unwrap();
    writer.start_file("second.txt", stored).unwrap();
    writer.write_all(second).unwrap();
    let mut archive = writer.finish().unwrap().into_inner();
    // Corrupt the stored data of the second entry so its CRC check fails after the first entry is written.
    let at = archive
        .windows(second.len())
        .position(|w| w == second)
        .unwrap();
    archive[at] ^= 0x20;
    h.capsule("directory", CapsulePayloadKind::DirectoryArchive, &archive);
    let output = h.unpack("--output-dir", "new-directory", false);
    assert!(text(&output).contains("checksum"), "{}", text(&output));
    assert!(!h.path("new-directory").exists());
}

fn unhex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn capsule_from_base_commit_still_unpacks() {
    let h = Harness::new();
    fs::write(h.path("sample.swal_capsule"), unhex(BASE_CAPSULE_HEX)).unwrap();
    h.unpack("--output", "out/base.txt", true);
    assert_eq!(fs::read(h.path("out/base.txt")).unwrap(), DATA);
}

// Capsule of DATA under PASS, produced once by the base commit (aff097e2) capsule code.
const BASE_CAPSULE_HEX: &str = "5357414c43415053e90000007b2276657273696f6e223a312c227061796c6f61645f6b696e64223a2273696e676c655f66696c65222c226f726967696e616c5f66696c656e616d65223a22736f75726365222c22617574686f72223a2274657374222c22637265617465645f6174223a22323032362d31302d30395430333a31373a33362e3131353532363134335a222c2273616c74223a223865623462366462386561343665613637636136383238303764363738363139222c226e6f6e6365223a22656539363063316138303436393935633431636239613438222c22636970686572746578745f6c656e677468223a33337d3f2c92e3b5e328dadbe85c3619cd343eea39170214bc9ee7f9e3bc5e7574bf295c";
