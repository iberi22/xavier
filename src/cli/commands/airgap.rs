//! Airgap Capsule CLI commands
//!
//! Provides CLI commands for interacting with physical air-gapped storage media (USB)
//! and creating/unpacking encrypted offline SWAL capsules (`.swal_capsule`).

use crate::crypto::airgap_capsule::{AirgapCapsule, CapsulePayloadKind};
use crate::system::usb_detector::{format_bytes, list_removable_devices, UsbStorageDevice};
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

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
    /// Pack a file, directory, or database backup into an encrypted .swal_capsule
    Pack {
        /// Input file or directory to pack
        #[arg(short, long)]
        input: PathBuf,
        /// Output capsule file path (default: <input>.swal_capsule)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Passphrase to encrypt the capsule. If omitted, prompts interactively.
        #[arg(short, long)]
        passphrase: Option<String>,
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
        /// Destination directory to extract contents
        #[arg(short, long)]
        output_dir: PathBuf,
        /// Passphrase to decrypt the capsule. If omitted, prompts interactively.
        #[arg(short, long)]
        passphrase: Option<String>,
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
        AirgapCommand::Pack {
            input,
            output,
            passphrase,
            kind,
            author,
        } => run_pack(input, output, passphrase, kind, author).await,
        AirgapCommand::Unpack {
            capsule,
            output_dir,
            passphrase,
        } => run_unpack(capsule, output_dir, passphrase).await,
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
        println!("   Insert a USB drive and ensure it is mounted (e.g. in /run/media or /media).\n");
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
    println!("     Capacity:    {} available / {} total", available, total);
    println!("     Writable:    {}", if dev.is_writable { "Yes" } else { "No (Read-Only)" });
    println!("---------------------------------------------------------------------------");
}

async fn run_pack(
    input: PathBuf,
    output: Option<PathBuf>,
    passphrase: Option<String>,
    kind: Option<String>,
    author: Option<String>,
) -> Result<()> {
    if !input.exists() {
        bail!("Input path does not exist: {}", input.display());
    }

    let out_path = output.unwrap_or_else(|| {
        let filename = input
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("payload");
        PathBuf::from(format!("{}.swal_capsule", filename))
    });

    let pass = match passphrase {
        Some(p) => p,
        None => {
            dialoguer::Password::new()
                .with_prompt("Enter encryption passphrase for airgap capsule")
                .interact()
                .context("Failed to read passphrase")?
        }
    };

    if pass.trim().is_empty() {
        bail!("Passphrase cannot be empty");
    }

    let payload_kind = match kind.as_deref() {
        Some("file") => CapsulePayloadKind::SingleFile,
        Some("directory") => CapsulePayloadKind::DirectoryArchive,
        Some("db_backup") => CapsulePayloadKind::DatabaseBackup,
        Some("memory_dump") => CapsulePayloadKind::MemoryDump,
        Some("secrets_vault") => CapsulePayloadKind::SecretsVault,
        Some(other) => bail!("Unknown payload kind: '{}'. Valid: file, directory, db_backup, memory_dump, secrets_vault", other),
        None => {
            if input.is_dir() {
                CapsulePayloadKind::DirectoryArchive
            } else {
                CapsulePayloadKind::SingleFile
            }
        }
    };

    let filename = input
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("payload")
        .to_string();

    let author_tag = author.unwrap_or_else(|| {
        whoami_username()
    });

    println!("📦 Packaging: {}", input.display());
    println!("   Payload Kind: {:?}", payload_kind);
    println!("   Author:       {}", author_tag);

    let raw_data = if input.is_dir() {
        println!("   Compressing directory zip archive into memory buffer...");
        zip_dir_to_memory(&input)?
    } else {
        fs::read(&input).context("Failed to read input file")?
    };

    println!("   Plaintext size: {} bytes ({})", raw_data.len(), format_bytes(raw_data.len() as u64));
    println!("   Deriving key (Argon2id 64MB RAM, 3 iterations) and encrypting AES-256-GCM...");

    let capsule_bytes = AirgapCapsule::pack_with_passphrase(
        &raw_data,
        payload_kind,
        filename,
        author_tag,
        &pass,
    )?;

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(&out_path, &capsule_bytes)?;

    println!("🔒 Capsule created successfully!");
    println!("   Output:       {}", out_path.display());
    println!("   Capsule size: {} bytes ({})", capsule_bytes.len(), format_bytes(capsule_bytes.len() as u64));
    println!("   Ready for offline USB transport.\n");

    Ok(())
}

async fn run_unpack(
    capsule: PathBuf,
    output_dir: PathBuf,
    passphrase: Option<String>,
) -> Result<()> {
    if !capsule.exists() {
        bail!("Capsule file does not exist: {}", capsule.display());
    }

    let capsule_bytes = fs::read(&capsule).context("Failed to read capsule file")?;

    let header = AirgapCapsule::inspect_header(&capsule_bytes)?;
    println!("🔍 Capsule Header: Version={}, Kind={:?}, Original='{}', Author='{}'",
        header.version, header.payload_kind, header.original_filename, header.author);

    let pass = match passphrase {
        Some(p) => p,
        None => {
            dialoguer::Password::new()
                .with_prompt("Enter decryption passphrase")
                .interact()
                .context("Failed to read passphrase")?
        }
    };

    println!("🔓 Decrypting and authenticating capsule...");
    let (decrypted_header, plaintext) = AirgapCapsule::unpack_with_passphrase(&capsule_bytes, &pass)?;

    if !output_dir.exists() {
        fs::create_dir_all(&output_dir)?;
    }

    match decrypted_header.payload_kind {
        CapsulePayloadKind::DirectoryArchive => {
            println!("   Unpacking directory archive into: {}", output_dir.display());
            unzip_memory_to_dir(&plaintext, &output_dir)?;
            println!("✅ Successfully extracted directory to: {}", output_dir.display());
        }
        _ => {
            let out_file = output_dir.join(&decrypted_header.original_filename);
            fs::write(&out_file, &plaintext)?;
            println!("✅ Successfully extracted file: {}", out_file.display());
            println!("   Size: {} bytes ({})", plaintext.len(), format_bytes(plaintext.len() as u64));
        }
    }

    Ok(())
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
    println!(" Total Size:         {} bytes ({})", capsule_bytes.len(), format_bytes(capsule_bytes.len() as u64));
    println!(" Magic:              SWALCAPS (valid)");
    println!(" Format Version:     {}", header.version);
    println!(" Payload Kind:       {:?}", header.payload_kind);
    println!(" Original Filename:  {}", header.original_filename);
    println!(" Author / Operator:  {}", header.author);
    println!(" Created At:         {}", header.created_at);
    println!(" Salt:               {}", &header.salt[..16]);
    println!(" Nonce:              {}", &header.nonce[..16]);
    println!(" Ciphertext Length:  {} bytes\n", header.ciphertext_length);

    Ok(())
}

fn whoami_username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "xavier-node".to_string())
}

fn zip_dir_to_memory(src_dir: &Path) -> Result<Vec<u8>> {
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buffer);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for entry in walkdir::WalkDir::new(src_dir) {
            let entry = entry?;
            let path = entry.path();
            let name = path.strip_prefix(src_dir)?.to_str().unwrap_or("");

            if name.is_empty() {
                continue;
            }

            if path.is_file() {
                zip.start_file(name, options)?;
                let mut f = fs::File::open(path)?;
                std::io::copy(&mut f, &mut zip)?;
            } else if path.is_dir() {
                zip.add_directory(name, options)?;
            }
        }
        zip.finish()?;
    }
    Ok(buffer.into_inner())
}

fn unzip_memory_to_dir(zip_bytes: &[u8], dest_dir: &Path) -> Result<()> {
    let reader = Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(reader)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => dest_dir.join(path),
            None => continue,
        };

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p)?;
                }
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}