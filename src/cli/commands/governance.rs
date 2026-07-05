//! Governance DAO CLI command handlers
//!
//! Handles governance subcommands: list, create, status, vote, council.

use super::enums::GovernanceSubcommand;
use crate::data_commons::governance::{GovernanceConfig, GovernanceEngine};
use crate::data_commons::types::{CouncilRole, WalletAddress};

use anyhow::Result;

/// Handle a governance subcommand from the CLI.
pub async fn handle_governance_command(cmd: GovernanceSubcommand) -> Result<()> {
    let config = GovernanceConfig::default();
    let mut engine = GovernanceEngine::new(config);

    match cmd {
        GovernanceSubcommand::List => {
            // For demo: create a default engine and list proposals
            let proposals = engine.active_proposals();
            if proposals.is_empty() {
                println!("📋 No active proposals found.");
            } else {
                println!("📋 Active Proposals ({}):", proposals.len());
                for p in proposals {
                    println!(
                        "  • {} [{}] — {} (state: {})",
                        p.id,
                        p.status_label(),
                        p.title,
                        p.xip_state.label()
                    );
                }
            }
            Ok(())
        }

        GovernanceSubcommand::Create { title, description } => {
            // Use a demo wallet for CLI-based creation
            let demo_wallet = WalletAddress("xv1_cli_operator".into());
            engine.register_activity(demo_wallet.clone());

            let proposal = engine.create_proposal(
                title,
                description,
                std::collections::HashMap::new(),
                demo_wallet,
            )?;

            println!("✅ Proposal created: {}", proposal.id);
            println!("   Status: {}", proposal.status_label());
            println!("   State:  {}", proposal.xip_state.label());
            println!("   Needs {} more supports to reach voting.", engine.get_proposal(&proposal.id).map_or(0, |p| 5u32.saturating_sub(p.supports.len() as u32)));
            Ok(())
        }

        GovernanceSubcommand::Status { proposal_id } => {
            let proposal = engine
                .get_proposal(&proposal_id)
                .ok_or_else(|| anyhow::anyhow!("Proposal '{}' not found", proposal_id))?;

            println!("📄 Proposal: {}", proposal.id);
            println!("   Title:    {}", proposal.title);
            println!("   Status:   {}", proposal.status_label());
            println!("   State:    {}", proposal.xip_state.label());
            println!("   Author:   {}", proposal.author.0);
            println!("   Supports: {}", proposal.supports.len());
            println!("   User votes:    {} (weighted: {})", proposal.user_votes.len(), proposal.weighted_user_votes.len());
            println!("   Council votes: {}", proposal.council_votes.len());
            if proposal.council_veto {
                println!("   ⚠️  COUNCIL VETO ACTIVE: {:?}", proposal.veto_reason);
            }
            if proposal.appealed {
                println!("   🔄 Community appeal filed.");
            }
            Ok(())
        }

        GovernanceSubcommand::Vote { proposal_id, approve } => {
            let demo_wallet = WalletAddress("xv1_cli_voter".into());
            engine.register_activity(demo_wallet.clone());

            let vote_str = if approve { "YES (approve)" } else { "NO (reject)" };

            match engine.user_vote(&proposal_id, &demo_wallet, approve, vec![], vec![]) {
                Ok(()) => {
                    println!("✅ Vote cast: {} on proposal {}", vote_str, proposal_id);
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Failed to cast vote: {}", e));
                }
            }
            Ok(())
        }

        GovernanceSubcommand::Council => {
            let members = engine.active_council_members();
            if members.is_empty() {
                println!("🏛️ No council members configured.");
                println!("   (This is a demo engine — add members programmatically.)");
            } else {
                println!("🏛️ Council Members ({}):", members.len());
                for m in members {
                    let role_str = match m.role {
                        CouncilRole::CoreMaintainer => "Core Maintainer",
                        CouncilRole::SkillContributor => "Skill Contributor",
                        CouncilRole::SecurityAuditor => "Security Auditor",
                        CouncilRole::Architect => "Architect",
                        CouncilRole::CommunityRepresentative => "Community Rep",
                    };
                    println!("  • {} — {} ({})", m.id, m.wallet.0, role_str);
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handle_list_returns_ok() {
        let result = handle_governance_command(GovernanceSubcommand::List).await;
        assert!(result.is_ok(), "List command should return Ok");
    }

    #[tokio::test]
    async fn test_handle_status_unknown_proposal_errors() {
        let result = handle_governance_command(GovernanceSubcommand::Status {
            proposal_id: "nonexistent_id".into(),
        })
        .await;
        assert!(result.is_err(), "Unknown proposal should return an error");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not found"),
            "Error should mention 'not found', got: {}",
            err_msg
        );
    }
}
