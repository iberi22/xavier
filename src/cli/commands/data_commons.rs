use crate::cli::commands::DataCommonsCommand;
use anyhow::{bail, Result};
use std::path::PathBuf;
use xavier::data_commons::readiness::ReadinessValidator;
use xavier::data_commons::training::TrainingExporter;

// Re-export governance and types for use by the governance CLI module
pub use xavier::data_commons::{governance, types};

/// Handle data commons command.
pub async fn handle_data_commons_command(cmd: DataCommonsCommand) -> Result<()> {
    // License check for Data Commons features
    let settings = xavier::settings::XavierSettings::current();
    xavier::security::license::require_mesh_license(&settings).map_err(|e| anyhow::anyhow!(e))?;

    match cmd {
        DataCommonsCommand::ExportTrainingBundle {
            output,
            seed,
            eval_ratio,
        } => export_training_bundle(output, seed, eval_ratio).await,
        DataCommonsCommand::Validate { bundle_path } => validate_training_bundle(bundle_path).await,
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
            // Create output directory if it doesn't exist
            if !output.exists() {
                std::fs::create_dir_all(&output)?;
            }

            // 1. Write bundle_manifest.json
            let manifest_json = serde_json::to_string_pretty(&bundle.manifest)?;
            std::fs::write(output.join("bundle_manifest.json"), manifest_json)?;

            // 2. Write anonymization_audit.json
            let audit_json = serde_json::to_string_pretty(&bundle.audit_summary)?;
            std::fs::write(output.join("anonymization_audit.json"), audit_json)?;

            // 3. Write train.jsonl
            let mut train_content = String::new();
            for record in &bundle.train_split {
                train_content.push_str(&serde_json::to_string(record)?);
                train_content.push('\n');
            }
            std::fs::write(output.join("train.jsonl"), train_content)?;

            // 4. Write eval.jsonl
            let mut eval_content = String::new();
            for record in &bundle.eval_split {
                eval_content.push_str(&serde_json::to_string(record)?);
                eval_content.push('\n');
            }
            std::fs::write(output.join("eval.jsonl"), eval_content)?;

            println!(
                "Training bundle exported successfully to directory {}",
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

async fn validate_training_bundle(bundle_path: PathBuf) -> Result<()> {
    println!("Validating training bundle at: {}", bundle_path.display());

    let validator = ReadinessValidator::new(bundle_path);
    let report = validator.validate();

    if report.is_ready {
        println!();
        println!("Bundle is READY for fine-tuning.");
    } else {
        println!();
        println!("Bundle is NOT READY for fine-tuning.");
        println!("Errors found:");
        for error in &report.errors {
            println!("  - {}", error);
        }
    }

    println!();
    println!("Checks performed:");
    for check in &report.checks_performed {
        println!("  [ok] {}", check);
    }

    if !report.is_ready {
        bail!("Readiness validation failed.");
    }

    Ok(())
}
