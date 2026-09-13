//! Interactive CLI onboarding installer for RAG configuration and multi-modal profiles.

use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, MultiSelect};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::cli::commands::enums::InitArgs;
use crate::cli::onboarding::{detect_gpu_and_vram, detect_ram_gb};

/// Supported data modalities for RAG indexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataTypeKind {
    Legal,
    Code,
    Video,
    Image,
    Audio,
}

impl fmt::Display for DataTypeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataTypeKind::Legal => write!(f, "legal"),
            DataTypeKind::Code => write!(f, "code"),
            DataTypeKind::Video => write!(f, "video"),
            DataTypeKind::Image => write!(f, "image"),
            DataTypeKind::Audio => write!(f, "audio"),
        }
    }
}

impl FromStr for DataTypeKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "legal" => Ok(DataTypeKind::Legal),
            "code" => Ok(DataTypeKind::Code),
            "video" => Ok(DataTypeKind::Video),
            "image" => Ok(DataTypeKind::Image),
            "audio" => Ok(DataTypeKind::Audio),
            other => anyhow::bail!(
                "Unknown data modality profile: '{}'. Expected legal, code, video, image, or audio",
                other
            ),
        }
    }
}

/// Installation configuration options generated during onboarding wizard execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallOptions {
    pub profiles: Vec<DataTypeKind>,
    pub storage_quota_gb: u32,
    pub local_only: bool,
    pub output_path: PathBuf,
}

/// Hardware hardware metrics tier categorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HardwareTier {
    Entry,
    Standard,
    Pro,
    Enterprise,
}

impl fmt::Display for HardwareTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HardwareTier::Entry => write!(f, "Entry (<= 8GB RAM)"),
            HardwareTier::Standard => write!(f, "Standard (16GB RAM)"),
            HardwareTier::Pro => write!(f, "Pro (32GB RAM / Dedicated GPU)"),
            HardwareTier::Enterprise => write!(f, "Enterprise (64GB+ RAM / High VRAM)"),
        }
    }
}

/// Hardware metrics detected during environment scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareMetrics {
    pub cpu_cores: usize,
    pub ram_gb: f32,
    pub vram_gb: Option<f32>,
    pub gpu_name: Option<String>,
    pub tier: HardwareTier,
}

impl HardwareMetrics {
    /// Detect system hardware metrics and classify performance tier.
    pub fn detect_current() -> Self {
        let cpu_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let ram_gb = detect_ram_gb();
        let (gpu_name, vram_gb) = detect_gpu_and_vram();

        let tier = match (ram_gb, vram_gb.unwrap_or(0.0)) {
            (r, v) if r >= 64.0 || v >= 24.0 => HardwareTier::Enterprise,
            (r, v) if r >= 32.0 || v >= 8.0 => HardwareTier::Pro,
            (r, _) if r >= 16.0 => HardwareTier::Standard,
            _ => HardwareTier::Entry,
        };

        HardwareMetrics {
            cpu_cores,
            ram_gb,
            vram_gb,
            gpu_name,
            tier,
        }
    }
}

/// Primary entrypoint for running the onboarding installer.
pub fn run_installer(args: InitArgs) -> Result<()> {
    run_installer_interactive(args)
}

/// Interactive or headless implementation of the onboarding installer flow.
pub fn run_installer_interactive(args: InitArgs) -> Result<()> {
    let metrics = HardwareMetrics::detect_current();

    println!("\n━━━━ Xavier Interactive Onboarding Installer ━━━━");
    println!("💻 Hardware Scan Specs:");
    println!("  • CPU Cores : {}", metrics.cpu_cores);
    println!("  • System RAM: {:.1} GB", metrics.ram_gb);
    if let Some(ref gpu) = metrics.gpu_name {
        println!(
            "  • GPU / VRAM: {} ({:.1} GB VRAM)",
            gpu,
            metrics.vram_gb.unwrap_or(0.0)
        );
    } else {
        println!("  • GPU / VRAM: None detected");
    }
    println!("  • System Tier: {}", metrics.tier);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let is_tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let headless = args.non_interactive || !is_tty;

    let profiles = if !args.profile.is_empty() {
        args.profile
            .iter()
            .map(|p| DataTypeKind::from_str(p))
            .collect::<Result<Vec<DataTypeKind>>>()?
    } else if headless {
        vec![DataTypeKind::Code, DataTypeKind::Legal]
    } else {
        let available_profiles = [
            ("Legal", DataTypeKind::Legal),
            ("Code", DataTypeKind::Code),
            ("Video", DataTypeKind::Video),
            ("Image", DataTypeKind::Image),
            ("Audio", DataTypeKind::Audio),
        ];
        let items: Vec<&str> = available_profiles.iter().map(|(label, _)| *label).collect();
        let defaults = vec![true, true, false, false, false];

        let selected = MultiSelect::new()
            .with_prompt(
                "Select data modality profiles to configure (Space to select, Enter to confirm)",
            )
            .items(&items)
            .defaults(&defaults)
            .interact()?;

        if selected.is_empty() {
            vec![DataTypeKind::Code]
        } else {
            selected
                .iter()
                .map(|&idx| available_profiles[idx].1)
                .collect()
        }
    };

    let storage_quota_gb = match args.storage_quota_gb {
        Some(quota) => quota,
        None if headless => match metrics.tier {
            HardwareTier::Entry => 20,
            HardwareTier::Standard => 50,
            HardwareTier::Pro => 100,
            HardwareTier::Enterprise => 250,
        },
        None => {
            let default_quota = match metrics.tier {
                HardwareTier::Entry => 20,
                HardwareTier::Standard => 50,
                HardwareTier::Pro => 100,
                HardwareTier::Enterprise => 250,
            };

            Input::new()
                .with_prompt("Storage quota in GB allocated for RAG modalities")
                .default(default_quota)
                .interact_text()?
        }
    };

    let local_only = if args.local_only {
        true
    } else if headless {
        false
    } else {
        Confirm::new()
            .with_prompt("Restrict system to 100% local-only mode (no cloud syncing)?")
            .default(false)
            .interact()?
    };

    let output_path = args
        .output_path
        .unwrap_or_else(|| PathBuf::from("xavier.toml"));

    let install_opts = InstallOptions {
        profiles,
        storage_quota_gb,
        local_only,
        output_path: output_path.clone(),
    };

    println!("⚡ Initializing database schema for selected profiles...");
    let db_path = PathBuf::from(".xavier").join("modality_store.db");
    initialize_modality_tables(&db_path, &install_opts.profiles)?;

    println!(
        "💾 Writing configuration to {}...",
        install_opts.output_path.display()
    );
    write_installer_config(&install_opts)?;

    println!("\n✅ Onboarding complete!");
    println!("  Profiles  : {:?}", install_opts.profiles);
    println!("  Quota     : {} GB", install_opts.storage_quota_gb);
    println!("  Local Only: {}", install_opts.local_only);
    println!("  Config    : {}", install_opts.output_path.display());

    Ok(())
}

/// Create local SQLite database and vector tables corresponding to chosen profiles.
pub fn initialize_modality_tables(db_path: &Path, profiles: &[DataTypeKind]) -> Result<()> {
    if let Some(parent) = db_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
    }

    let conn = rusqlite::Connection::open(db_path)
        .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS modality_metadata (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            profile TEXT NOT NULL UNIQUE,
            table_name TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    for profile in profiles {
        let table_name = format!("{}_vectors", profile);
        let create_sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                embedding_blob BLOB,
                metadata_json TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            table_name
        );
        conn.execute(&create_sql, [])?;

        conn.execute(
            "INSERT OR REPLACE INTO modality_metadata (profile, table_name) VALUES (?1, ?2)",
            [&profile.to_string(), &table_name],
        )?;
    }

    Ok(())
}

/// Write configuration file in TOML format (or JSON fallback).
pub fn write_installer_config(options: &InstallOptions) -> Result<()> {
    let toml_content = format!(
        r#"# Xavier Multi-Modal RAG Configuration
# Generated by xavier init

[rag]
profiles = [{}]
storage_quota_gb = {}
local_only = {}

[database]
modality_db_path = ".xavier/modality_store.db"
"#,
        options
            .profiles
            .iter()
            .map(|p| format!("\"{}\"", p))
            .collect::<Vec<_>>()
            .join(", "),
        options.storage_quota_gb,
        options.local_only
    );

    if let Some(parent) = options.output_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(&options.output_path, toml_content).with_context(|| {
        format!(
            "Failed to write configuration file to {}",
            options.output_path.display()
        )
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_non_interactive_flag_generation() {
        let dir = tempdir().unwrap();
        let out_path = dir.path().join("xavier.toml");

        let args = InitArgs {
            profile: vec!["code".to_string(), "legal".to_string()],
            storage_quota_gb: Some(45),
            local_only: true,
            output_path: Some(out_path.clone()),
            non_interactive: true,
        };

        let res = run_installer_interactive(args);
        assert!(res.is_ok());
        assert!(out_path.exists());

        let content = fs::read_to_string(&out_path).unwrap();
        assert!(content.contains(r#"profiles = ["code", "legal"]"#));
        assert!(content.contains("storage_quota_gb = 45"));
        assert!(content.contains("local_only = true"));
    }

    #[test]
    fn test_data_type_kind_parsing() {
        assert_eq!(
            DataTypeKind::from_str("legal").unwrap(),
            DataTypeKind::Legal
        );
        assert_eq!(DataTypeKind::from_str("code").unwrap(), DataTypeKind::Code);
        assert_eq!(
            DataTypeKind::from_str("video").unwrap(),
            DataTypeKind::Video
        );
        assert_eq!(
            DataTypeKind::from_str("image").unwrap(),
            DataTypeKind::Image
        );
        assert_eq!(
            DataTypeKind::from_str("audio").unwrap(),
            DataTypeKind::Audio
        );

        assert!(DataTypeKind::from_str("invalid_kind").is_err());
    }

    #[test]
    fn test_config_output_generation() {
        let dir = tempdir().unwrap();
        let config_file = dir.path().join("xavier.toml");

        let options = InstallOptions {
            profiles: vec![DataTypeKind::Legal, DataTypeKind::Video],
            storage_quota_gb: 120,
            local_only: false,
            output_path: config_file.clone(),
        };

        let res = write_installer_config(&options);
        assert!(res.is_ok());

        let content = fs::read_to_string(&config_file).unwrap();
        let parsed: toml::Value = toml::from_str(&content).expect("Valid TOML config expected");
        assert_eq!(parsed["rag"]["storage_quota_gb"].as_integer().unwrap(), 120);
        assert!(!parsed["rag"]["local_only"].as_bool().unwrap());
    }

    #[test]
    fn test_initialize_modality_tables() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("modality_store.db");

        let profiles = vec![DataTypeKind::Code, DataTypeKind::Audio];
        let res = initialize_modality_tables(&db_path, &profiles);
        assert!(res.is_ok());

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='code_vectors'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let audio_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='audio_vectors'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(audio_count, 1);
    }
}
