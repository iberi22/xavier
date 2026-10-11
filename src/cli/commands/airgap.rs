//! Airgap Capsule CLI commands
//!
//! Provides CLI commands for interacting with physical air-gapped storage media (USB)
//! and creating/unpacking encrypted offline SWAL capsules (`.swal_capsule`).

use crate::crypto::airgap_capsule::{AirgapCapsule, CapsulePayloadKind};
use crate::system::usb_detector::{format_bytes, list_removable_devices, UsbStorageDevice};
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

#[derive(Args, Debug, Clone)]
pub struct AirgapArgs {
    #[command(subcommand)]
    pub cmd: AirgapCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum AirgapCommand {
    /// Detect removable USB storage devices and show mount points and capacity
    Detect {
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },
    /// Capsule creation is disabled pending a format redesign
    Pack {
        /// Input file or directory to pack
        #[arg(short, long)]
        input: PathBuf,
        /// Output capsule file path (default: <input>.swal_capsule)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Passphrase file (creation is currently disabled).
        #[arg(long)]
        passphrase_file: Option<PathBuf>,
        /// Payload kind override: file, directory, db_backup, memory_dump, secrets_vault
        #[arg(short, long)]
        kind: Option<String>,
        /// Optional author or operator identity tag
        #[arg(long)]
        author: Option<String>,
    },
    /// Unpack an encrypted .swal_capsule to a destination directory
    Unpack {
        /// Path to the .swal_capsule file
        #[arg(short, long)]
        capsule: PathBuf,
        /// Explicit relative output file path
        #[arg(
            long,
            conflicts_with = "output_dir",
            required_unless_present = "output_dir"
        )]
        output: Option<PathBuf>,
        /// Existing directory for a file, or new directory for an archive
        #[arg(short, long)]
        output_dir: Option<PathBuf>,
        /// Read the passphrase from a file; otherwise prompt interactively
        #[arg(long)]
        passphrase_file: Option<PathBuf>,
    },
    /// Inspect the unencrypted header metadata of a .swal_capsule file
    Inspect {
        /// Path to the .swal_capsule file
        #[arg(short, long)]
        capsule: PathBuf,
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_airgap_command(args: AirgapArgs) -> Result<()> {
    match args.cmd {
        AirgapCommand::Detect { json } => run_detect(json).await,
        AirgapCommand::Pack { .. } => {
            bail!("capsule creation is disabled pending a format redesign; see docs/ARCHITECTURE/AIRGAP_CAPSULE_PROTOCOL.md")
        }
        AirgapCommand::Unpack {
            capsule,
            output,
            output_dir,
            passphrase_file,
        } => run_unpack(capsule, output, output_dir, passphrase_file).await,
        AirgapCommand::Inspect { capsule, json } => run_inspect(capsule, json).await,
    }
}

async fn run_detect(json: bool) -> Result<()> {
    let devices = list_removable_devices();
    if json {
        println!("{}", serde_json::to_string_pretty(&devices)?);
        return Ok(());
    }

    println!("\n╔═══════════════════════════════════════════════════════════════════════════╗");
    println!("║                 XAVIER AIR-GAP STORAGE HARDWARE DETECTOR                  ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════╝\n");

    if devices.is_empty() {
        println!("ℹ️  No removable USB storage devices currently detected or mounted.");
        println!(
            "   Insert a USB drive and ensure it is mounted (e.g. in /run/media or /media).\n"
        );
        return Ok(());
    }

    println!("Found {} removable device(s):\n", devices.len());
    for (i, dev) in devices.iter().enumerate() {
        print_device_card(i + 1, dev);
    }

    Ok(())
}

fn print_device_card(idx: usize, dev: &UsbStorageDevice) {
    let label = dev.label.as_deref().unwrap_or("UNLABELED");
    let fs = dev.fs_type.as_deref().unwrap_or("unknown");
    let total = dev
        .total_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "N/A".to_string());
    let available = dev
        .available_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "N/A".to_string());

    println!(" [{}] Device:      {}", idx, dev.device_path.display());
    println!("     Mount point: {}", dev.mount_point.display());
    println!("     Label:       {}", label);
    println!("     Filesystem:  {}", fs);
    println!(
        "     Capacity:    {} available / {} total",
        available, total
    );
    println!(
        "     Writable:    {}",
        if dev.is_writable {
            "Yes"
        } else {
            "No (Read-Only)"
        }
    );
    println!("---------------------------------------------------------------------------");
}

async fn run_unpack(
    capsule: PathBuf,
    output: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    passphrase_file: Option<PathBuf>,
) -> Result<()> {
    let capsule_bytes = fs::read(&capsule).context("Failed to read capsule file")?;
    let pass = match passphrase_file {
        Some(path) => {
            let text = fs::read_to_string(path).context("Failed to read passphrase file")?;
            text.strip_suffix("\r\n")
                .or_else(|| text.strip_suffix(['\n', '\r']))
                .unwrap_or(&text)
                .to_owned()
        }
        None => dialoguer::Password::new()
            .with_prompt("Enter decryption passphrase")
            .interact()
            .context("Failed to read passphrase")?,
    };
    println!("Decrypting and authenticating capsule (Argon2id: 19456 KiB, 2 iterations, parallelism 1)...");
    let (header, plaintext) = AirgapCapsule::unpack_with_passphrase(&capsule_bytes, &pass)?;
    println!(
        "Original filename: {}",
        sanitize_metadata(&header.original_filename)
    );
    match header.payload_kind {
        CapsulePayloadKind::DirectoryArchive => {
            let directory = output_dir.context("Directory capsules require --output-dir")?;
            unzip_memory_to_dir(&plaintext, &directory)?;
            println!("Successfully extracted directory: {}", directory.display());
        }
        _ => {
            let destination = match output {
                Some(path) => path,
                None => {
                    let mut name = capsule
                        .file_stem()
                        .context("Capsule path needs a file stem")?
                        .to_os_string();
                    name.push(".out");
                    output_dir
                        .context("Provide --output or --output-dir")?
                        .join(name)
                }
            };
            validate_output_path(&destination)?;
            create_output_file(&destination)?.write_all(&plaintext)?;
            println!("Successfully extracted file: {}", destination.display());
        }
    }
    Ok(())
}

fn sanitize_metadata(text: &str) -> String {
    text.chars().flat_map(char::escape_default).collect()
}

fn validate_relative_path(path: &Path) -> Result<()> {
    use std::path::Component;
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        bail!("Output path must be relative and contain no parent components");
    }
    if path.components().any(
        |c| matches!(c, Component::Normal(name) if is_protected_key_name(&name.to_string_lossy())),
    ) {
        bail!("Protected key names are not valid output paths");
    }
    Ok(())
}

/// Key file names are matched case-insensitively, independent of the filesystem's case rules.
fn is_protected_key_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("record.key") || name.eq_ignore_ascii_case("master.key")
}

/// Directories unpack must never write into: the data dir, which holds `node/record.key`
/// (same env var that `record_key_path()` reads), and `~/.xavier`, which holds `master.key`.
/// An unresolvable home is an error; skipping the `~/.xavier` root would fail open.
fn protected_key_roots(data_dir: PathBuf, home: Option<PathBuf>) -> Result<Vec<PathBuf>> {
    let home =
        home.context("cannot resolve HOME; refusing unpack without the protected key-area check")?;
    Ok(vec![data_dir, home.join(".xavier")])
}

fn validate_output_path(path: &Path) -> Result<()> {
    validate_relative_path(path)?;
    for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("Output path contains a symlink")
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let canonical_parent = parent.canonicalize().context("Output parent must exist")?;
    let protected = protected_key_roots(
        crate::settings::XavierSettings::resolve_data_dir(),
        dirs::home_dir(),
    )?;
    for area in protected {
        // Resolve existing ancestors, including when the key area has not been created.
        let mut base = area.as_path();
        let mut suffix = Vec::new();
        while !base.exists() {
            if let Some(name) = base.file_name() {
                suffix.push(name);
            }
            base = base
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
        }
        let mut canonical_area = base.canonicalize()?;
        for name in suffix.iter().rev() {
            canonical_area.push(name);
        }
        if canonical_parent.starts_with(canonical_area) {
            bail!("Output parent is in a protected Xavier data area");
        }
    }
    Ok(())
}

/// Creates a new file only: never follows a symlink at the final component, never overwrites.
#[cfg(unix)]
fn create_output_file(path: &Path) -> Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .context("Failed to create new output file")
}

#[cfg(not(unix))]
fn create_output_file(_path: &Path) -> Result<fs::File> {
    bail!("Safe capsule extraction requires O_NOFOLLOW support")
}

async fn run_inspect(capsule: PathBuf, json: bool) -> Result<()> {
    if !capsule.exists() {
        bail!("Capsule file does not exist: {}", capsule.display());
    }

    let capsule_bytes = fs::read(&capsule).context("Failed to read capsule file")?;
    let header = AirgapCapsule::inspect_header(&capsule_bytes)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&header)?);
        return Ok(());
    }

    println!("\n╔═══════════════════════════════════════════════════════════════════════════╗");
    println!("║                    SWAL AIR-GAP CAPSULE HEADER INSPECT                    ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════╝\n");
    println!(" Capsule File:       {}", capsule.display());
    println!(
        " Total Size:         {} bytes ({})",
        capsule_bytes.len(),
        format_bytes(capsule_bytes.len() as u64)
    );
    println!(" Magic:              SWALCAPS (valid)");
    println!(" Format Version:     {}", header.version);
    println!(" Payload Kind:       {:?}", header.payload_kind);
    println!(
        " Original Filename:  {}",
        sanitize_metadata(&header.original_filename)
    );
    println!(" Author / Operator:  {}", sanitize_metadata(&header.author));
    println!(" Created At:         {}", header.created_at);
    println!(
        " Salt:               {}",
        header.salt.chars().take(16).collect::<String>()
    );
    println!(
        " Nonce:              {}",
        header.nonce.chars().take(16).collect::<String>()
    );
    println!(" Ciphertext Length:  {} bytes\n", header.ciphertext_length);

    Ok(())
}

fn unzip_memory_to_dir(zip_bytes: &[u8], dest_dir: &Path) -> Result<()> {
    let mut archive = ZipArchive::new(Cursor::new(zip_bytes))?;
    // Validate all entries before creating the output directory.
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        // enclosed_name() would silently strip a leading root; refuse absolute entry names instead.
        if file.name().starts_with(['/', '\\']) {
            bail!("Absolute archive entry paths are not supported");
        }
        let name = file
            .enclosed_name()
            .context("Invalid archive output path")?;
        validate_relative_path(&name)?;
        if file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            bail!("Archive symlink entries are not supported");
        }
    }
    validate_output_path(dest_dir)?;
    fs::create_dir(dest_dir).context("Archive output directory must be new")?;
    if let Err(error) = extract_archive_entries(&mut archive, dest_dir) {
        // dest_dir was created above, so removing it cannot touch pre-existing data.
        let _ = fs::remove_dir_all(dest_dir);
        return Err(error);
    }
    Ok(())
}

fn extract_archive_entries(archive: &mut ZipArchive<Cursor<&[u8]>>, dest_dir: &Path) -> Result<()> {
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = dest_dir.join(
            file.enclosed_name()
                .context("Invalid archive output path")?,
        );
        if file.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            // Safe because dest_dir is new and symlink entries were refused in the validation pass.
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            validate_output_path(&outpath)?;
            let mut outfile = create_output_file(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_home_refuses_unpack() {
        let error = protected_key_roots(PathBuf::from("/data"), None).unwrap_err();
        assert!(error.to_string().contains("cannot resolve HOME"));
    }

    #[test]
    fn protected_roots_cover_data_dir_and_home_key_dir() {
        let roots =
            protected_key_roots(PathBuf::from("/data"), Some(PathBuf::from("/home/op"))).unwrap();
        assert_eq!(
            roots,
            vec![PathBuf::from("/data"), PathBuf::from("/home/op/.xavier")]
        );
    }

    #[test]
    fn protected_key_names_match_any_case() {
        for name in ["record.key", "RECORD.KEY", "Master.Key"] {
            assert!(is_protected_key_name(name), "{name}");
        }
        assert!(!is_protected_key_name("record.keys"));
        assert!(!is_protected_key_name("notes.txt"));
    }
}
