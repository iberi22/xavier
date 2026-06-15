use crate::cli::commands::DataCommonsCommand;
use anyhow::{bail, Result};
use std::path::PathBuf;
use xavier::data_commons::training::TrainingExporter;

pub async fn handle_data_commons_command(cmd: DataCommonsCommand) -> Result<()> {
    match cmd {
        DataCommonsCommand::ExportTrainingBundle {
            output,
            seed,
            eval_ratio,
        } => export_training_bundle(output, seed, eval_ratio).await,
    }
}

async fn export_training_bundle(output: PathBuf, seed: u64, eval_ratio: f32) -> Result<()> {
    if !(0.0..=1.0).contains(&eval_ratio) {
        bail!("eval-ratio must be between 0.0 and 1.0");
    }

    println!("Starting training bundle export...");
    println!("Output: {}", output.display());
    println!("Seed: {}", seed);
    println!("Eval ratio: {}", eval_ratio);

    // Get telemetry DB path from environment or default
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let default_db_path = format!("{}/.xavier/telemetry.db", home);
    let db_path_str = std::env::var("XAVIER_TELEMETRY_DB_PATH").unwrap_or(default_db_path);
    let db_path = std::path::Path::new(&db_path_str);

    if !db_path.exists() {
        println!("Telemetry database not found at {}", db_path.display());
        return Ok(());
    }

    let exporter = TrainingExporter::new(db_path);

    match exporter.generate_bundle(seed, eval_ratio, None) {
        Ok(bundle) => {
            let json = serde_json::to_string_pretty(&bundle)?;
            std::fs::write(&output, json)?;
            println!(
                "Training bundle exported successfully to {}",
                output.display()
            );
            println!(
                "Included records: {}",
                bundle.audit_summary.included_records
            );
            println!("Train split: {}", bundle.train_split.len());
            println!("Eval split: {}", bundle.eval_split.len());
        }
        Err(e) => {
            println!("Export failed: {}", e);
        }
    }

    Ok(())
}
