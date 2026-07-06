//! Governance Command Handlers — CLI implementation for Xavier Governance DAO

use crate::cli::commands::data_commons::{governance, types};
use crate::cli::commands::GovernanceSubcommand;
use anyhow::Result;

pub async fn handle_governance_command(command: GovernanceSubcommand) -> Result<()> {
    // License check for Governance features
    let settings = xavier::settings::XavierSettings::current();
    xavier::security::license::require_mesh_license(&settings).map_err(|e| anyhow::anyhow!(e))?;

    match command {
        GovernanceSubcommand::List => list_proposals().await,
        GovernanceSubcommand::Council => list_council_members().await,
        _ => todo!(),
    }
}

async fn list_proposals() -> Result<()> {
    println!("Fetching Xavier Governance Proposals (XIPs)...");

    // In this Phase 0/1 implementation, we use a default engine
    // Real persistence would load this from a shared state or mesh broadcast
    let config = governance::GovernanceConfig::default();
    let engine = governance::GovernanceEngine::new(config);

    // Mock some proposals for demonstration if needed,
    // but the engine starts empty by default.
    let proposals = engine.active_proposals();

    if proposals.is_empty() {
        println!("\nNo active proposals found in the governance board.");
        println!("Use 'xavier governance propose' (coming soon) to create one.");
        return Ok(());
    }

    println!(
        "\n{:<15} {:<30} {:<15} {:<10}",
        "ID", "Title", "State", "Supports"
    );
    println!("{}", "-".repeat(80));

    for p in proposals {
        println!(
            "{:<15} {:<30} {:<15} {:<10}",
            p.id,
            p.title,
            p.xip_state.label(),
            p.supports.len()
        );
    }

    Ok(())
}

async fn list_council_members() -> Result<()> {
    println!("Xavier Core Council Members:");

    let config = governance::GovernanceConfig::default();
    let mut engine = governance::GovernanceEngine::new(config);

    // Add a few default mock members if the council is empty for CLI visibility
    if engine.council_size() == 0 {
        use types::{CouncilRole, WalletAddress};
        engine.add_council_member(
            WalletAddress("xv1_core_maintainer_alpha_0123456789abcdef0123456789abcd".to_string()),
            CouncilRole::CoreMaintainer,
            vec!["Security".into(), "Architecture".into()],
        );
    }

    let members = engine.active_council_members();

    if members.is_empty() {
        println!("\nNo active council members found.");
        return Ok(());
    }

    println!("\n{:<15} {:<40} {:<20}", "ID", "Wallet Address", "Role");
    println!("{}", "-".repeat(85));

    for m in members {
        let role_str = match m.role {
            types::CouncilRole::CoreMaintainer => "Core Maintainer",
            types::CouncilRole::SkillContributor => "Skill Contributor",
            types::CouncilRole::SecurityAuditor => "Security Auditor",
            types::CouncilRole::Architect => "Architect",
            types::CouncilRole::CommunityRepresentative => "Community Rep",
        };

        println!("{:<15} {:<40} {:<20}", m.id, m.wallet.0, role_str);
    }

    Ok(())
}
