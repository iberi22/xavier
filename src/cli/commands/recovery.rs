//! CLI `recovery` command: inspect, seal, and restore the memory-store key.
//!
//! # What is being recovered, and what is not
//!
//! This command protects the **node record key**
//! (`xavier::memory::sqlite_vec_store::at_rest`), which unwraps XRK1 per-record
//! DEKs. XDK2 rows also require the master key and default-space keystore.
//! It is a *different domain* from
//! `xavier::security::recovery`, which handles the auth seed phrase and backup
//! codes; see ADR-038 for why the two must not be merged.
//!
//! It deliberately does **not** duplicate the Fase 0 shell work:
//! `~/.xavier-offline/record.key` and backup "Puerta 5" already exist and keep
//! working. This is the native path — a key-check value that proves a restored
//! key is *the* right one, and a non-destructive install that never overwrites a
//! live key.
//!
//! # Invariants
//!
//! - Never prints key material.
//! - `restore` refuses to overwrite an existing `record.key`: a clobbering restore
//!   can permanently orphan every row (ADR-038).
//! - `restore` verifies the KCV *before* writing anything.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use xavier::drive;
use xavier::memory::sqlite_vec_store::at_rest;
use xavier::recovery::{
    manifest::RecoveryManifest, store::live_key_source, KcvState, KeySourceReport, MnemonicPath,
    RecoveryStore, SealHealth, MNEMONIC_SEAL_FILE, PASSPHRASE_SEAL_FILE,
};

use crate::cli::commands::enums::{RecoveryArgs, RecoveryCommand};

/// Handle `xavier recovery <subcommand>`.
pub async fn handle_recovery_command(args: RecoveryArgs) -> Result<()> {
    match args.command {
        RecoveryCommand::Status {
            crypt_passphrase_backed_up,
        } => status(crypt_passphrase_backed_up),
        RecoveryCommand::Seal {
            mnemonic,
            passphrase,
            passphrase_file,
            words_out,
            replace_mnemonic,
            replace_passphrase,
            rekey,
        } => seal(
            mnemonic,
            passphrase,
            passphrase_file,
            words_out,
            replace_mnemonic,
            replace_passphrase,
            rekey,
        ),
        RecoveryCommand::Restore { key_file, kcv } => {
            use std::io::Read;
            let content = if key_file.as_os_str() == "-" {
                let mut content = String::new();
                std::io::stdin().read_to_string(&mut content)?;
                content
            } else {
                std::fs::read_to_string(&key_file)
                    .with_context(|| format!("cannot read key from {}", key_file.display()))?
            };
            restore(&strip_one_line_ending(content), kcv.as_deref())
        }
        RecoveryCommand::Unseal {
            mnemonic,
            passphrase,
            words_file,
            passphrase_file,
            out,
        } => unseal(mnemonic, passphrase, words_file, passphrase_file, out),
        RecoveryCommand::Drive => drive_status(),
    }
}

fn status(crypt_passphrase_backed_up: bool) -> Result<()> {
    let store = RecoveryStore::from_env_or_default();
    let (live_key, _) = at_rest::peek_record_key_with_source();
    let report = store.status(Some(crypt_passphrase_backed_up), live_key.as_ref());
    let manifest = store.read_manifest().ok().flatten();

    println!("Xavier recovery status");
    println!("  recovery dir    : {}", store.root().display());
    println!(
        "  manifest        : {}",
        if manifest.is_some() {
            "present"
        } else {
            "ABSENT"
        }
    );
    println!(
        "  key-check value : {}",
        match report.kcv {
            KcvState::Absent => "ABSENT",
            KcvState::Malformed => "MALFORMED",
            KcvState::ManifestDisagrees =>
                "MANIFEST DISAGREES — MISMATCH between manifest and sidecar",
            KcvState::MatchesLiveKey => "matches live key",
            KcvState::MismatchesLiveKey => "MISMATCH — seals protect another key",
            KcvState::LiveKeyMissing => "present, live key missing (restore possible)",
        }
    );
    println!("  mnemonic seal   : {}", seal_health(&report.mnemonic_seal));
    println!(
        "  passphrase seal : {}",
        seal_health(&report.passphrase_seal)
    );
    println!(
        "  crypt passphrase: {}",
        if crypt_passphrase_backed_up {
            "reported backed up"
        } else {
            "NOT backed up — encrypted snapshots are unreadable without it"
        }
    );
    println!(
        "  live record key : {}",
        match live_key_source() {
            KeySourceReport::Env => format!("env ({})", at_rest::RECORD_KEY_ENV),
            KeySourceReport::File(p) => format!("file {}", p.display()),
            KeySourceReport::Missing(p) => format!(
                "MISSING at {} — do not start the daemon or run write commands; run 'xavier recovery unseal' first",
                p.display()
            ),
            KeySourceReport::Unavailable(r) => format!("UNAVAILABLE ({r})"),
        }
    );
    println!(
        "  recoverable     : {}",
        if report.is_recoverable() { "yes" } else { "NO" }
    );
    println!(
        "  note: \"valid\" = structurally openable; words/passphrase correctness needs unseal"
    );
    for gap in report.gaps() {
        println!("  ! {gap}");
    }
    Ok(())
}

fn seal_health(health: &SealHealth) -> String {
    match health {
        SealHealth::Absent => "ABSENT".into(),
        SealHealth::Malformed(reason) => format!("MALFORMED ({reason})"),
        SealHealth::Valid { mode_0600: true } => "valid (0600)".into(),
        SealHealth::Valid { mode_0600: false } => "valid (MODE NOT 0600)".into(),
    }
}

/// Validate all input before installing seals; publish the KCV and manifest last.
fn seal(
    mnemonic: bool,
    passphrase: bool,
    passphrase_file: Option<PathBuf>,
    words_out: Option<PathBuf>,
    replace_mnemonic: bool,
    replace_passphrase: bool,
    rekey: bool,
) -> Result<()> {
    use std::io::Write;
    if !mnemonic && !passphrase {
        bail!("choose at least one of --mnemonic or --passphrase");
    }
    // Refuse before any prompt, generation or sealing.
    let mut words_tty = if mnemonic && words_out.is_none() {
        use std::io::IsTerminal;
        let tty = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/tty")
            .context(
                "no controlling TTY available; use --words-out <file> to deliver recovery words",
            )?;
        if !tty.is_terminal() {
            bail!("no controlling TTY available; use --words-out <file> to deliver recovery words");
        }
        Some(tty)
    } else {
        None
    };
    let (key, _) = at_rest::peek_record_key_with_source();
    let key = key.ok_or_else(|| {
        anyhow::anyhow!(
            "no record key at {}; refusing to seal a key that protects no data",
            at_rest::record_key_path().display()
        )
    })?;
    let store = RecoveryStore::from_env_or_default();
    for (selected, replace, file, flag) in [
        (
            mnemonic,
            replace_mnemonic,
            MNEMONIC_SEAL_FILE,
            "--replace-mnemonic",
        ),
        (
            passphrase,
            replace_passphrase,
            PASSPHRASE_SEAL_FILE,
            "--replace-passphrase",
        ),
    ] {
        if selected && !replace && std::fs::symlink_metadata(store.root().join(file)).is_ok() {
            bail!("seal already exists; use {flag} to replace it explicitly");
        }
    }
    if let Some(stored) = store.read_kcv()? {
        if !rekey && xavier::recovery::kcv::verify_kcv(&key, &stored).is_err() {
            bail!("stored KCV differs from the live key; use --rekey explicitly");
        }
    }
    let pass = if passphrase {
        let pass = match passphrase_file {
            Some(path) => strip_one_line_ending(
                std::fs::read_to_string(&path)
                    .with_context(|| format!("cannot read passphrase from {}", path.display()))?,
            ),
            None => dialoguer::Password::new()
                .with_prompt("Choose a passphrase (>= 12 chars)")
                .with_confirmation("Repeat the passphrase", "The passphrases do not match")
                .interact()
                .map_err(|e| anyhow::anyhow!("cannot read passphrase: {e}"))?,
        };
        if pass.chars().count() < xavier::recovery::passphrase::MIN_PASSPHRASE_LEN {
            bail!(
                "passphrase too short: {} characters minimum",
                xavier::recovery::passphrase::MIN_PASSPHRASE_LEN
            );
        }
        Some(pass)
    } else {
        None
    };
    let words = if mnemonic {
        Some(MnemonicPath::generate()?.0)
    } else {
        None
    };
    let mut manifest = RecoveryManifest::new(&key, None);
    match xavier::keystore::MasterKeyManager::load_existing() {
        Ok(Some(master)) => manifest.master_key_kcv = master.kcv_hex(),
        _ => {
            let note = "master key not found on this host; master KCV omitted";
            manifest.note = Some(note.into());
            eprintln!("warning: {note}");
        }
    }
    // Deliver the words before the seal exists, using checked I/O.
    if let Some(words) = &words {
        match &words_out {
            Some(path) => {
                xavier::keystore::create_private_file_new(path, format!("{words}\n").as_bytes())
                    .with_context(|| format!("cannot write words to {}", path.display()))?
            }
            None => {
                let tty = words_tty.as_mut().expect("mnemonic delivery validated");
                writeln!(tty, "WRITE THESE 24 WORDS ON PAPER NOW (they ARE the key):")?;
                for (i, w) in words.split_whitespace().enumerate() {
                    writeln!(tty, "  {:>2}. {w}", i + 1)?;
                }
                tty.flush()?;
            }
        }
    }
    let installed = (|| -> Result<()> {
        if let Some(pass) = &pass {
            if replace_passphrase {
                store.write_passphrase_seal(&key, pass)?;
            } else {
                store.write_passphrase_seal_new(&key, pass)?;
            }
        }
        if let Some(words) = &words {
            if replace_mnemonic {
                store.write_mnemonic_seal(&key, words)?;
            } else {
                store.write_mnemonic_seal_new(&key, words)?;
            }
        }
        Ok(())
    })();
    if installed.is_err() {
        if let Some(path) = &words_out {
            if words.is_some() {
                std::fs::remove_file(path).with_context(|| {
                    format!(
                        "cannot remove words file {} after seal failure",
                        path.display()
                    )
                })?;
            }
        }
    }
    installed?;
    // Once a seal exists, keep its delivered words even if metadata publication fails.
    store.write_kcv(&key)?;
    store.write_manifest(&manifest)?;
    if let Some(path) = &words_out {
        println!("words written to {}", path.display());
    }
    if mnemonic && words_out.is_none() {
        println!("mnemonic seal verified: reopens with these words");
    }
    if passphrase {
        println!("passphrase seal verified: reopens with this passphrase");
    }
    Ok(())
}

/// Strip at most one trailing line ending (`\r\n`, `\n`, or `\r`), preserving any
/// leading, internal, or trailing space before the line ending.
fn strip_one_line_ending(mut s: String) -> String {
    if s.ends_with("\r\n") {
        s.truncate(s.len() - 2);
    } else if s.ends_with(['\n', '\r']) {
        s.truncate(s.len() - 1);
    }
    s
}

/// Unseal a recovery key from mnemonic or passphrase and install or export it.
fn unseal(
    mnemonic: bool,
    passphrase: bool,
    words_file: Option<PathBuf>,
    passphrase_file: Option<PathBuf>,
    out: Option<PathBuf>,
) -> Result<()> {
    if mnemonic == passphrase {
        bail!("choose exactly one of --mnemonic or --passphrase");
    }

    let store = RecoveryStore::from_env_or_default();
    let key = if mnemonic {
        let words = match words_file {
            Some(path) => std::fs::read_to_string(&path)
                .with_context(|| format!("cannot read words from {}", path.display()))?
                .trim()
                .to_string(),
            None => dialoguer::Password::new()
                .with_prompt("Enter 24-word recovery mnemonic")
                .interact()
                .map_err(|e| anyhow::anyhow!("cannot read mnemonic: {e}"))?,
        };
        store.unseal_mnemonic(&words)?
    } else {
        let pass = match passphrase_file {
            Some(path) => {
                let content = std::fs::read_to_string(&path)
                    .with_context(|| format!("cannot read passphrase from {}", path.display()))?;
                strip_one_line_ending(content)
            }
            None => dialoguer::Password::new()
                .with_prompt("Enter recovery passphrase")
                .interact()
                .map_err(|e| anyhow::anyhow!("cannot read passphrase: {e}"))?,
        };
        store.unseal_passphrase(&pass)?
    };

    let key_hex = xavier::crypto::hex_encode(key);

    if let Some(out_path) = out {
        if let Err(e) = xavier::keystore::create_private_file_new(&out_path, key_hex.as_bytes()) {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                bail!("output file already exists: {}", out_path.display());
            }
            return Err(e).with_context(|| format!("cannot write key to {}", out_path.display()));
        }
        println!("Unsealed key written to {}.", out_path.display());
        Ok(())
    } else {
        let expected = store.read_kcv()?;
        let target = at_rest::record_key_path();
        println!("target: {}", target.display());

        match store.restore_into(&key_hex, &target, expected.as_deref())? {
            xavier::recovery::RestoreOutcome::Installed => {
                println!("installed. Verify by reading a record before trusting it.");
                Ok(())
            }
            xavier::recovery::RestoreOutcome::Rejected { reason } => {
                bail!("restore refused ({reason}); the existing key was left untouched")
            }
        }
    }
}

/// Install a recovered record key — never overwriting an existing one.
fn restore(key_hex: &str, kcv_hex: Option<&str>) -> Result<()> {
    let store = RecoveryStore::from_env_or_default();
    let expected = match kcv_hex {
        Some(k) => Some(k.to_string()),
        None => store.read_kcv()?,
    };
    let target = at_rest::record_key_path();
    println!("target: {}", target.display());

    match store.restore_into(key_hex, &target, expected.as_deref())? {
        xavier::recovery::RestoreOutcome::Installed => {
            println!("installed. Verify by reading a record before trusting it.");
            Ok(())
        }
        xavier::recovery::RestoreOutcome::Rejected { reason } => {
            bail!("restore refused ({reason}); the existing key was left untouched")
        }
    }
}

/// Read-only Drive connection status. Never prints a token.
fn drive_status() -> Result<()> {
    let remote = drive::detect_rclone_remote();
    let creds = drive::DriveCredentialStore::from_env_or_default();
    println!("Xavier Drive connection");
    match &remote {
        Some(r) => println!(
            "  rclone remote   : {} (encrypted: {})",
            r.remote_string(),
            r.suitable_for_encrypted_backup()
        ),
        None => println!("  rclone remote   : NONE — an OAuth flow would be required"),
    }
    println!("  oauth required  : {}", remote.is_none());
    println!("  sealed creds    : {}", creds.exists());
    println!(
        "  requested scopes: {}",
        drive::DriveScopes::REQUESTED.join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    xavier::isolate_test_process!();

    use super::*;

    #[test]
    fn sealing_requires_at_least_one_path() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt
            .block_on(handle_recovery_command(RecoveryArgs {
                command: RecoveryCommand::Seal {
                    mnemonic: false,
                    passphrase: false,
                    passphrase_file: None,
                    words_out: None,
                    replace_mnemonic: false,
                    replace_passphrase: false,
                    rekey: false,
                },
            }))
            .unwrap_err();
        assert!(err.to_string().contains("at least one"));
    }

    /// The live key file must survive a refused restore, verbatim.
    #[test]
    fn restore_never_clobbers_an_existing_key() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("record.key");
        std::fs::write(&target, "live-key-material").unwrap();

        let store = RecoveryStore::at(dir.path().join("recovery"));
        store.write_kcv(&[0x11u8; 32]).unwrap();

        let outcome = store
            .restore_into(
                &"ab".repeat(32),
                &target,
                Some(&xavier::recovery::kcv::encode_kcv(&[0x22u8; 32])),
            )
            .unwrap();

        assert!(!outcome.installed());
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "live-key-material"
        );
    }

    #[test]
    #[serial_test::serial]
    fn unseal_command_with_words_file_installs_key() {
        let dir = tempfile::tempdir().unwrap();
        let recovery_dir = dir.path().join("recovery");
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&recovery_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();

        let prev_recovery = std::env::var("XAVIER_RECOVERY_DIR").ok();
        let prev_data = std::env::var("XAVIER_DATA_DIR").ok();
        let prev_record = std::env::var("XAVIER_RECORD_KEY").ok();
        let prev_home = std::env::var("HOME").ok();

        std::env::set_var("XAVIER_RECOVERY_DIR", &recovery_dir);
        std::env::set_var("XAVIER_DATA_DIR", &data_dir);
        std::env::set_var("HOME", dir.path());
        std::env::remove_var("XAVIER_RECORD_KEY");

        let store = RecoveryStore::at(&recovery_dir);
        let key = [0x42u8; 32];
        let (words, _) = MnemonicPath::generate().unwrap();
        store.write_kcv(&key).unwrap();
        store.write_mnemonic_seal(&key, &words).unwrap();

        let words_file = dir.path().join("words.txt");
        std::fs::write(&words_file, &words).unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let res = rt.block_on(handle_recovery_command(RecoveryArgs {
            command: RecoveryCommand::Unseal {
                mnemonic: true,
                passphrase: false,
                words_file: Some(words_file),
                passphrase_file: None,
                out: None,
            },
        }));

        let installed_path = at_rest::record_key_path();
        let key_exists = installed_path.exists();
        let read_hex = if key_exists {
            Some(std::fs::read_to_string(&installed_path).unwrap())
        } else {
            None
        };

        if let Some(v) = prev_recovery {
            std::env::set_var("XAVIER_RECOVERY_DIR", v);
        } else {
            std::env::remove_var("XAVIER_RECOVERY_DIR");
        }
        if let Some(v) = prev_data {
            std::env::set_var("XAVIER_DATA_DIR", v);
        } else {
            std::env::remove_var("XAVIER_DATA_DIR");
        }
        if let Some(v) = prev_record {
            std::env::set_var("XAVIER_RECORD_KEY", v);
        } else {
            std::env::remove_var("XAVIER_RECORD_KEY");
        }
        if let Some(v) = prev_home {
            std::env::set_var("HOME", v);
        } else {
            std::env::remove_var("HOME");
        }

        assert!(res.is_ok(), "unseal command failed: {:?}", res);
        assert!(
            key_exists,
            "key must be installed at {}",
            installed_path.display()
        );
        assert_eq!(read_hex.unwrap(), xavier::crypto::hex_encode(key));
    }

    #[test]
    fn test_strip_one_line_ending() {
        assert_eq!(
            strip_one_line_ending("  pass with spaces  \n".to_string()),
            "  pass with spaces  "
        );
        assert_eq!(strip_one_line_ending("abc\r\n".to_string()), "abc");
        assert_eq!(strip_one_line_ending("abc\n\n".to_string()), "abc\n");
        assert_eq!(strip_one_line_ending("abc\r".to_string()), "abc");
        assert_eq!(strip_one_line_ending("abc".to_string()), "abc");
    }
}
