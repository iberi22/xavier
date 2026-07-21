// SPDX-License-Identifier: MIT OR LICENSE-MESH
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
    use xavier::security::license::detect_license;

    let license_kind = detect_license(&settings);

    println!("╔══════════════════════════════════════════════╗");
    println!("║           Xavier License Status              ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  Core License:  {:<33} ║", license_kind);
    println!("║  License Type:  {:<33} ║", settings.license.license_type);
    println!("╠══════════════════════════════════════════════╣");
    if settings.license.mesh_accepted {
        println!("║  Mesh License:  ✅ Accepted               ║");
        println!("║  Mesh features: ✅ Enabled                ║");
        println!("║  Governance:    ✅ Available               ║");
        println!(
            "║  Data Commons:  {:<36} ║",
            if xavier::settings::XavierSettings::current()
                .data_commons
                .enabled
            {
                "✅ Enabled"
            } else {
                "⬜ Disabled"
            }
        );
    } else {
        println!("║  Mesh License:  ❌ Not Accepted           ║");
        println!("║  Mesh features: ❌ Disabled                ║");
        println!("║  Governance:    ❌ Disabled                ║");
        println!("║  Data Commons:  ❌ Disabled                ║");
    }
    if let Some(ref key) = settings.license.commercial_key {
        println!("║  Commercial Key: {:.20}...         ║", key);
        println!("║  Enterprise Features: ✅ Unlocked         ║");
    }
    println!("╠══════════════════════════════════════════════╣");
    match license_kind {
        xavier::security::license::LicenseKind::Mit => {
            println!("║  MIT: Free for standalone, local-first use.  ║");
            println!("║  Permissive, open source.                  ║");
            println!("║  Commercial License: see COMMERCIAL_LICENSE.md ║");
        }
        xavier::security::license::LicenseKind::Mesh => {
            println!("║  Mesh/Commercial: network/commercial OK    ║");
            println!("║  Private mesh: allowed under active license║");
            println!("║  Enterprise features: ✅ Unlocked          ║");
        }
    }
    println!("╚══════════════════════════════════════════════╝");
    Ok(())
}

async fn handle_license_accept() -> Result<()> {
    let mut settings = xavier::settings::XavierSettings::current();
    if settings.license.mesh_accepted {
        println!("✅ Mesh License already accepted.");
        return Ok(());
    }

    println!("📜 Xavier License Agreement");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Xavier is dual-licensed:");
    println!();
    println!("  Core Engine: MIT License");
    println!("  Mesh Features: Xavier Mesh License v1.0");
    println!("  Enterprise: Xavier Commercial License");
    println!();
    println!("By accepting the Mesh License, you agree to:");
    println!("  1. Usage Tiers — Free for individuals/OSS, paid for large commercial entities");
    println!("  2. Network Participation — your node joins the Xavier Mesh");
    println!("  3. Governance Rights — earn voting rights via XP + reputation");
    println!("  4. Data Sovereignty — your data remains yours and encrypted");
    println!("  5. XP Tokenomics — earn XP for contributions (no monetary value)");
    println!();
    println!("Commercial terms: see COMMERCIAL_LICENSE.md or contact iberi22");
    println!("Full terms: LICENSE (MIT), LICENSE-MESH, COMMERCIAL_LICENSE.md");
    println!();

    // In a real interactive CLI we'd ask for confirmation.
    // For now, auto-accept since this is a development tool.
    settings.license.mesh_accepted = true;
    settings.license.license_type = "Xavier-Mesh-1.0".to_string();

    // Persist
    if let Err(e) = xavier::settings::serialization::save(&settings).await {
        tracing::warn!(error = %e, "failed to persist license acceptance");
        println!(
            "⚠️  License accepted for this session but could not persist: {}",
            e
        );
    } else {
        println!("✅ Mesh License accepted and saved!");
    }

    Ok(())
}

async fn handle_license_show() -> Result<()> {
    let full_license = include_str!("../../../LICENSE");
    let mesh_license = include_str!("../../../LICENSE-MESH");
    println!("════════════════════════════════════════════════════════");
    println!("  Xavier Licensing Summary");
    println!("════════════════════════════════════════════════════════");
    println!();
    println!("1. Core Engine — MIT License (LICENSE)");
    println!("   Free for standalone, local-first use.");
    println!();
    println!("2. Mesh Features — Xavier Mesh License v1.0 (LICENSE-MESH)");
    println!("   Free for individuals/OSS. Additional terms for P2P participation.");
    println!();
    println!("3. Commercial License (COMMERCIAL_LICENSE.md)");
    println!("   Required for commercial organizations over usage thresholds.");
    println!();
    println!("═══ Dual License & MIT terms ═══");
    println!("{}", &full_license[..full_license.len().min(2000)]);
    println!();
    println!("═══ Xavier Mesh License v1.0 ═══");
    println!("{}", mesh_license);
    println!();
    println!("For full terms, see LICENSE and LICENSE-MESH.");
    println!("For commercial terms, see COMMERCIAL_LICENSE.md.");
    Ok(())
}
