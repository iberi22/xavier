use xavier::auto_improvement::Cycler;
use xavier::settings::XavierSettings;
use crate::cli::commands::AutoImproveCommand;
use anyhow::Result;

pub async fn handle_auto_improve_command(cmd: AutoImproveCommand) -> Result<()> {
    // For CLI operations, we need settings
    // In a real scenario, we'd load them from a file.
    let mut settings = XavierSettings::default();
    // Try to load settings from default path if it exists
    if let Ok(content) = std::fs::read_to_string("xavier.config.json") {
        if let Ok(s) = serde_json::from_str(&content) {
            settings = s;
        }
    }

    let cycler = Cycler::new();

    match cmd {
        AutoImproveCommand::Run => {
            println!("🚀 Starting auto-improvement cycle...");
            match cycler.run_full_cycle(&mut settings, None).await {
                Ok(cycle) => {
                    println!("\n✅ Cycle {} complete!", cycle.cycle_id);
                    println!("📈 Improvement: {:.2}%", cycle.improvement_pct);
                    println!("🧪 Accepted changes: {}", cycle.accepted_changes.join(", "));

                    // Generate report
                    if let Ok(report_file) = cycler.generate_gap_report(&cycle).await {
                        println!("📄 Report generated: reports/{}", report_file);
                    }
                }
                Err(e) => {
                    println!("❌ Cycle failed: {}", e);
                }
            }
        }
        AutoImproveCommand::Status => {
            println!("📊 Auto-improvement Status");
            println!("Last run: Unknown (check history in Xavier server)");
            // In a real system, we would query the server or a local state file
        }
        AutoImproveCommand::Schedule => {
            println!("📅 Auto-improvement Schedule");
            println!("Current: {}", settings.tgd.schedule);
            println!("(Edit via xavier.config.json or XAVIER_TGD_SCHEDULE)");
        }
        AutoImproveCommand::Report => {
            let reports_dir = std::path::Path::new("reports");
            if let Ok(entries) = std::fs::read_dir(reports_dir) {
                let mut reports: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().map_or(false, |ext| ext == "md"))
                    .collect();

                reports.sort_by_key(|e| e.path());

                if let Some(last) = reports.last() {
                    println!("📄 Last report: {}", last.path().display());
                    if let Ok(content) = std::fs::read_to_string(last.path()) {
                        println!("\n---\n{}\n---\n", content);
                    }
                } else {
                    println!("No reports found in reports/");
                }
            } else {
                println!("No reports directory found.");
            }
        }
    }

    Ok(())
}
