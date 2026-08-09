//! CLI training management handlers (client-side)

use anyhow::{anyhow, Result};
use colored::*;
use serde_json::json;
use std::path::PathBuf;

use crate::cli::commands::enums::{TrainingCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{require_xavier_token, resolve_base_url};

/// Handle training commands.
pub async fn handle_training_command(cmd: TrainingCommand) -> Result<()> {
    match cmd {
        TrainingCommand::Export {
            output,
            seed,
            eval_ratio,
            clearance,
            language,
            segment,
        } => {
            handle_export(output, seed, eval_ratio, clearance, language, segment).await
        }
        TrainingCommand::Datasets => handle_datasets().await,
        TrainingCommand::DatasetsTrain { id, output } => {
            handle_dataset_split(id, "train", output).await
        }
        TrainingCommand::DatasetsEval { id, output } => {
            handle_dataset_split(id, "eval", output).await
        }
    }
}

async fn handle_export(
    output: PathBuf,
    seed: u64,
    eval_ratio: f32,
    clearance: Option<String>,
    language: Option<String>,
    segment: Option<String>,
) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    println!(
        "{} Requesting training bundle generation from server...",
        "🤖".cyan()
    );

    let payload = json!({
        "seed": seed,
        "eval_ratio": eval_ratio,
        "clearance": clearance,
        "language": language,
        "segment": segment,
    });

    let resp = client
        .post(format!("{}/v1/training/bundles", base_url))
        .header("X-Xavier-Token", &token)
        .json(&payload)
        .send()
        .await?;

    if resp.status().is_success() {
        let res_val: serde_json::Value = resp.json().await?;
        let dataset_id = res_val["dataset_id"]
            .as_str()
            .ok_or_else(|| anyhow!("No dataset_id returned from server"))?;

        println!(
            "{} Bundle generated successfully on server: {}",
            "✅".green(),
            dataset_id.bold()
        );

        // Ensure local output directory exists
        std::fs::create_dir_all(&output)?;

        // Download train.jsonl
        println!("{} Downloading train split...", "📥".cyan());
        let train_resp = client
            .get(format!(
                "{}/v1/training/datasets/{}/train",
                base_url, dataset_id
            ))
            .header("X-Xavier-Token", &token)
            .send()
            .await?;

        if !train_resp.status().is_success() {
            return Err(anyhow!(
                "Failed to download train split: {}",
                train_resp.text().await?
            ));
        }
        let train_content = train_resp.text().await?;
        std::fs::write(output.join("train.jsonl"), train_content)?;

        // Download eval.jsonl
        println!("{} Downloading eval split...", "📥".cyan());
        let eval_resp = client
            .get(format!(
                "{}/v1/training/datasets/{}/eval",
                base_url, dataset_id
            ))
            .header("X-Xavier-Token", &token)
            .send()
            .await?;

        if !eval_resp.status().is_success() {
            return Err(anyhow!(
                "Failed to download eval split: {}",
                eval_resp.text().await?
            ));
        }
        let eval_content = eval_resp.text().await?;
        std::fs::write(output.join("eval.jsonl"), eval_content)?;

        // Generate and write metadata.json locally on the client
        let settings = crate::settings::XavierSettings::current();
        let consent_given = settings.data_commons.consent_given;

        let meta_clearance = res_val["metadata"]["clearance"]
            .as_str()
            .or_else(|| clearance.as_deref())
            .unwrap_or("INTERNAL");
        let meta_language = res_val["metadata"]["language"]
            .as_str()
            .or_else(|| language.as_deref())
            .unwrap_or("en");
        let meta_segment = res_val["metadata"]["segment"]
            .as_str()
            .or_else(|| segment.as_deref())
            .unwrap_or("telemetry");

        let metadata_val = json!({
            "seed": seed,
            "eval_ratio": eval_ratio,
            "clearance": meta_clearance,
            "consent": consent_given,
            "segment": meta_segment,
            "idioma": meta_language,
            "language": meta_language,
        });

        std::fs::write(
            output.join("metadata.json"),
            serde_json::to_string_pretty(&metadata_val)?,
        )?;

        println!(
            "{} Training export complete! Files written to: {}",
            "✅".green(),
            output.display().to_string().bold()
        );
        println!("  • train.jsonl");
        println!("  • eval.jsonl");
        println!("  • metadata.json");
    } else {
        let err = resp.text().await?;
        println!("{} Generation failed: {}", "❌".red(), err);
        return Err(anyhow!("Generation failed: {}", err));
    }

    Ok(())
}

async fn handle_datasets() -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    println!("{} Fetching datasets list from server...", "🔍".cyan());

    let resp = client
        .get(format!("{}/v1/training/datasets", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let datasets: Vec<xavier::data_commons::training::DatasetMetadata> =
            resp.json().await?;
        println!(
            "{} Found {} training datasets on server:",
            "📋".cyan(),
            datasets.len()
        );
        for ds in datasets {
            println!(
                "  • {} - Size: {}, Clearance: {}, Language: {}, Segment: {}",
                ds.id.bold(),
                ds.size,
                ds.clearance,
                ds.language,
                ds.segment
            );
        }
    } else {
        let err = resp.text().await?;
        println!("{} Failed to list datasets: {}", "❌".red(), err);
        return Err(anyhow!("Failed to list datasets: {}", err));
    }

    Ok(())
}

async fn handle_dataset_split(
    id: String,
    split: &str,
    output: Option<PathBuf>,
) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    if output.is_some() {
        println!(
            "{} Fetching {} split for dataset {}...",
            "📥".cyan(),
            split,
            id
        );
    }

    let resp = client
        .get(format!(
            "{}/v1/training/datasets/{}/{}",
            base_url, id, split
        ))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let content = resp.text().await?;
        if let Some(path) = output {
            std::fs::write(&path, &content)?;
            println!(
                "{} Successfully wrote {} split for dataset {} to {}",
                "✅".green(),
                split,
                id,
                path.display().to_string().bold()
            );
        } else {
            print!("{}", content);
        }
    } else {
        let err = resp.text().await?;
        println!(
            "{} Failed to retrieve {} split: {}",
            "❌".red(),
            split,
            err
        );
        return Err(anyhow!("Failed to retrieve {} split: {}", split, err));
    }

    Ok(())
}
