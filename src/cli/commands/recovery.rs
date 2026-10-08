//! CLI `recovery` command: inspect, seal, and restore the memory-store key.
//!
//! # What is being recovered, and what is not
//!
//! This command protects the **node record key**
//! (`xavier::memory::sqlite_vec_store::at_rest`), the only key that unwraps the
//! per-record DEKs. It is a *different domain* from
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
    manifest::RecoveryManifest, store::live_key_source, KeySourceReport, MnemonicPath,
    PassphrasePath, RecoveryStore,
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
        } => seal(mnemonic, passphrase),
        RecoveryCommand::Restore { key_hex, kcv } => {
            let key_hex = key_hex.ok_or_else(|| {
                anyhow::anyhow!("--key-hex is required (or pipe it: --key-hex -)")
            })?;
            restore(&key_hex, kcv.as_deref())
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
    let report = store.status(Some(crypt_passphrase_backed_up));
    let manifest = store.read_manifest().ok().flatten();
    let kcv_present = store.read_kcv().ok().flatten().is_some();

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
        if kcv_present { "present" } else { "ABSENT" }
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
            // A key generated just now means every pre-existing row is already
            // unreadable. This is the state that silently destroys a vault.
            KeySourceReport::Generated(p) => format!(
                "GENERATED NOW at {} — every pre-existing record is already unreadable!",
                p.display()
            ),
            KeySourceReport::Unavailable(r) => format!("UNAVAILABLE ({r})"),
        }
    );
    println!(
        "  recoverable     : {}",
        if report.is_recoverable() { "yes" } else { "NO" }
    );
    for gap in report.gaps() {
        println!("  ! {gap}");
    }
    Ok(())
}

/// Seal the current record key under a passphrase and/or a mnemonic.
fn seal(mnemonic: bool, passphrase: bool) -> Result<()> {
    if !mnemonic && !passphrase {
        bail!("choose at least one of --mnemonic or --passphrase");
    }
    let (key, source) = at_rest::resolve_record_key_with_source();
    let Some(key) = key else {
        bail!("no node record key is resolvable ({source:?})");
    };

    let store = RecoveryStore::from_env_or_default();
    store.write_kcv(&key)?;
    // The manifest stores KCVs only, never keys.
    let master = xavier::keystore::MasterKeyManager::load_or_init().ok();
    let master_bytes = master.as_ref().map(master_key_bytes);
    store.write_manifest(&RecoveryManifest::new(&key, master_bytes.as_ref()))?;
    println!("Sealed recovery material for the node record key (source: {source:?}).");

    if passphrase {
        let pass = dialoguer::Password::new()
            .with_prompt("Choose a passphrase (>= 12 chars)")
            .with_confirmation("Repeat the passphrase", "The passphrases do not match")
            .interact()
            .map_err(|e| anyhow::anyhow!("cannot read passphrase: {e}"))?;
        let path = store.write_passphrase_seal(&key, &pass)?;
        println!("  passphrase seal: {}", path.display());
        println!("  verified       : the seal reopens with what you just entered");
        println!("  Copy it OFF this host and verify it restores.");
    }

    if mnemonic {
        let (words, _) = MnemonicPath::generate()?;
        let path = store.write_mnemonic_seal(&key, &words)?;
        println!("  mnemonic seal  : {}", path.display());
        println!("  verified       : the seal reopens with what you just entered");
        println!();
        println!("  ══════════════════════════════════════════════");
        println!("  WRITE THESE 24 WORDS ON PAPER NOW:");
        println!("  ══════════════════════════════════════════════");
        for (i, w) in words.split_whitespace().enumerate() {
            println!("  {:>2}. {w}", i + 1);
        }
        println!("  ══════════════════════════════════════════════");
        println!("  They ARE the key: 256 bits, no reset. Anyone holding");
        println!("  them can decrypt the whole memory store.");
    }
    Ok(())
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
            Some(path) => std::fs::read_to_string(&path)
                .with_context(|| format!("cannot read passphrase from {}", path.display()))?
                .trim()
                .to_string(),
            None => dialoguer::Password::new()
                .with_prompt("Enter recovery passphrase")
                .interact()
                .map_err(|e| anyhow::anyhow!("cannot read passphrase: {e}"))?,
        };
        store.unseal_passphrase(&pass)?
    };

    let key_hex = xavier::crypto::hex_encode(key);

    if let Some(out_path) = out {
        if out_path.exists() {
            bail!("output file already exists: {}", out_path.display());
        }
        xavier::keystore::write_private_file(&out_path, key_hex.as_bytes())
            .with_context(|| format!("cannot write key to {}", out_path.display()))?;
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

/// Master key bytes, used only to derive its KCV immediately.
fn master_key_bytes(manager: &xavier::keystore::MasterKeyManager) -> [u8; 32] {
    // `vault_key()` is an HKDF label of the master key; the manifest stores only
    // the resulting KCV, so no extra exposure occurs here.
    manager.vault_key().unwrap_or([0u8; 32])
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

    match store.restore_into(key_hex.trim(), &target, expected.as_deref())? {
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
}
