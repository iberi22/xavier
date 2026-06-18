//! CLI License management commands
//!
//! Handles `xavier license status`, `xavier license accept`, `xavier license show`.

use crate::cli::commands::enums::LicenseCommand;
use anyhow::Result;

pub async fn handle_license_command(cmd: LicenseCommand) -> Result<()> {
    match cmd {
        LicenseCommand::Status => handle_license_status().await,
        LicenseCommand::Accept => handle_license_accept().await,
        LicenseCommand::Show => handle_license_show().await,
    }
}

async fn handle_license_status() -> Result<()> {
    let settings = xavier::settings::XavierSettings::current();
    println!("╔══════════════════════════════════════╗");
    println!("║        Xavier License Status         ║");
    println!("╠══════════════════════════════════════╣");
    if settings.license.mesh_accepted {
        println!("║  License: Xavier Mesh License v1.0    ║");
        println!("║  Mesh features: ✅ Enabled            ║");
        println!("║  Governance:    ✅ Available           ║");
        println!("║  Data Commons:  {}                         ║",
            if xavier::settings::XavierSettings::current().data_commons.enabled { "✅ Enabled" } else { "⬜ Disabled" });
    } else {
        println!("║  License: MIT (standalone)             ║");
        println!("║  Mesh features: ❌ Disabled            ║");
        println!("║  Governance:    ❌ Disabled            ║");
        println!("║  Data Commons:  ❌ Disabled            ║");
    }
    println!("╚══════════════════════════════════════╝");
    Ok(())
}

async fn handle_license_accept() -> Result<()> {
    let mut settings = xavier::settings::XavierSettings::current();
    if settings.license.mesh_accepted {
        println!("✅ Mesh License already accepted.");
        return Ok(());
    }

    println!("📜 Xavier Mesh License v1.0");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("By accepting this license, you agree to:");
    println!("  1. Usage Tiers — Free for individuals/OSS, paid for large commercial entities");
    println!("  2. Network Participation — your node joins the Xavier Mesh");
    println!("  3. Governance Rights — earn voting rights via XP + reputation");
    println!("  4. Data Sovereignty — your data remains yours and encrypted");
    println!("  5. XP Tokenomics — earn XP for contributions (no monetary value)");
    println!();
    println!("Commercial terms: see docs/PRICING.md or contact iberi22");
    println!("Full terms: LICENSE-MESH");
    println!();

    // In a real interactive CLI we'd ask for confirmation.
    // For now, auto-accept since this is a development tool.
    settings.license.mesh_accepted = true;
    settings.license.license_type = "Xavier-Mesh-1.0".to_string();

    // Persist
    if let Err(e) = xavier::settings::serialization::save(&settings).await {
        tracing::warn!(error = %e, "failed to persist license acceptance");
        println!("⚠️  License accepted for this session but could not persist: {}", e);
    } else {
        println!("✅ Mesh License accepted and saved!");
    }

    Ok(())
}

async fn handle_license_show() -> Result<()> {
    let mesh_license = include_str!("../../../LICENSE-MESH");
    println!("═══ Xavier Mesh License v1.0 ═══");
    println!("{}", mesh_license);
    println!();
    println!("For standalone MIT terms, see LICENSE.");
    Ok(())
}
