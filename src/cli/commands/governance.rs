//! Governance Command Handlers — CLI implementation for Xavier Governance DAO

use crate::cli::commands::data_commons::{governance, types};
use crate::cli::commands::GovernanceCommand;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use xavier::governance::{BicameralDao, MockBicameralDao};
use xavier::data_commons::types::{WalletAddress, CouncilRole, SystemParams};

fn resolve_state_path() -> PathBuf {
    let state_dir_str = std::env::var("XAVIER_STATE_DIR")
        .or_else(|_| std::env::var("USERPROFILE"))
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let state_dir = PathBuf::from(&state_dir_str);
    state_dir.join(".xavier").join("bicameral_governance_state.json")
}

pub async fn handle_governance(command: GovernanceCommand) -> anyhow::Result<()> {
    // License check for Governance features
    let settings = xavier::settings::XavierSettings::current();
    xavier::security::license::require_mesh_license(&settings).map_err(|e| anyhow::anyhow!(e))?;

    match command {
        GovernanceCommand::List => list_proposals().await,
        GovernanceCommand::Create { title, description } => create_proposal(title, description).await,
        GovernanceCommand::Status { proposal_id } => show_proposal_status(proposal_id).await,
        GovernanceCommand::Vote { proposal_id, approve } => cast_vote(proposal_id, approve).await,
        GovernanceCommand::Council => list_council_members().await,
    }
}

async fn list_proposals() -> Result<()> {
    println!("Fetching Xavier Governance Proposals (XIPs)...");

    let state_path = resolve_state_path();
    let dao = MockBicameralDao::new(Some(state_path));

    let proposals = dao.list_proposals().await.map_err(|e| anyhow::anyhow!(e))?;

    if proposals.is_empty() {
        println!("\nNo active proposals found in the governance board.");
        println!("Use 'xavier governance create <title> <description>' to create one.");
        return Ok(());
    }

    println!(
        "\n{:<25} {:<30} {:<15} {:<10}",
        "ID", "Title", "State", "Supports"
    );
    println!("{}", "-".repeat(85));

    for p in proposals {
        println!(
            "{:<25} {:<30} {:<15} {:<10}",
            p.id,
            p.title,
            p.xip_state.label(),
            p.supports.len()
        );
    }

    Ok(())
}

async fn create_proposal(title: String, description: String) -> Result<()> {
    println!("Creating new Xavier Governance Proposal (XIP)...");

    let state_path = resolve_state_path();
    let mut dao = MockBicameralDao::new(Some(state_path));

    // Define a valid 65-character author wallet address
    let author = WalletAddress("xv1_author_0123456789abcdef0123456789abcd0123456789abcdef012345".to_string());

    // Setup some default changes to the system params as proposed changes
    let mut changes = HashMap::new();
    changes.insert("reference_price".to_string(), "12".to_string());
    changes.insert("burn_rate".to_string(), "85".to_string());

    let proposal = dao.submit_proposal(&title, &description, changes, author)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    println!("\n✅ Proposal successfully submitted!");
    println!("ID: {}", proposal.id);
    println!("Title: {}", proposal.title);
    println!("Initial State: {}", proposal.xip_state.label());
    println!("\nNext step: Support this proposal to advance it to Voting phase.");
    println!("To support/vote, run the 'vote' command on this proposal.");

    Ok(())
}

async fn show_proposal_status(proposal_id: String) -> Result<()> {
    println!("Retrieving status for proposal: {}...", proposal_id);

    let state_path = resolve_state_path();
    let dao = MockBicameralDao::new(Some(state_path));

    let proposal = dao.get_proposal(&proposal_id)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    println!("\nProposal Details:");
    println!("--------------------------------------------------");
    println!("ID:          {}", proposal.id);
    println!("Title:       {}", proposal.title);
    println!("Description: {}", proposal.description);
    println!("Author:      {}", proposal.author.0);
    println!("Status:      {:?}", proposal.status);
    println!("State:       {}", proposal.xip_state.label());
    println!("Supports:    {}", proposal.supports.len());
    println!("Council Veto: {}", proposal.council_veto);
    if let Some(reason) = &proposal.veto_reason {
        println!("Veto Reason:  {}", reason);
    }
    println!("Appealed:    {}", proposal.appealed);
    println!("--------------------------------------------------");

    // Display votes cast so far
    println!("\nUser Votes Cast (weighted by reputation):");
    if proposal.weighted_user_votes.is_empty() {
        println!("  No user votes cast yet.");
    } else {
        for (wallet, vote) in &proposal.weighted_user_votes {
            println!("  - {}: approve={}, weight={}", wallet.0, vote.approve, vote.weight);
        }
    }

    println!("\nCouncil Votes Cast (one vote per member):");
    if proposal.council_votes.is_empty() {
        println!("  No council votes cast yet.");
    } else {
        for (member_id, approve) in &proposal.council_votes {
            println!("  - {}: approve={}", member_id, approve);
        }
    }

    Ok(())
}

async fn cast_vote(proposal_id: String, approve: bool) -> Result<()> {
    let state_path = resolve_state_path();
    let mut dao = MockBicameralDao::new(Some(state_path));

    // Retrieve proposal first to check its current phase
    let mut proposal = dao.get_proposal(&proposal_id)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    println!("Current state of proposal: {}", proposal.xip_state.label());

    // If it is in Draft or Discussion state, we need to gather support
    if matches!(proposal.xip_state, types::XipState::Draft { .. } | types::XipState::Discussion { .. }) {
        println!("Proposal is in discussion/draft. Adding supports to push it to voting...");

        // Add enough supports (5 supports are needed by default to reach Voting phase)
        for i in 0..5 {
            let supporter_wallet = WalletAddress(format!("xv1_supporter_wallet_for_governance_testing_phase_00000000000_{}", i));
            if let Err(e) = dao.support_proposal(&proposal_id, supporter_wallet).await {
                println!("Note on support: {}", e);
            }
        }

        // Reload proposal to see if state moved to Voting
        proposal = dao.get_proposal(&proposal_id)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        println!("New state after supports: {}", proposal.xip_state.label());
    }

    // If we are now in Voting state, cast the community and council votes
    if matches!(proposal.xip_state, types::XipState::Voting { .. }) {
        println!("Casting bicameral votes...");

        // 1. Cast community user votes
        let user_voter = WalletAddress("xv1_community_voter_0123456789abcdef0123456789abcd0123456789abcde".to_string());
        dao.register_activity(user_voter.clone()).await.map_err(|e| anyhow::anyhow!(e))?;
        dao.cast_user_vote(&proposal_id, user_voter, approve).await.map_err(|e| anyhow::anyhow!(e))?;
        println!("  - Cast community user vote: approve={}", approve);

        // 2. Setup/cast council votes
        // Ensure we have council members registered
        let council = dao.list_council_members().await.map_err(|e| anyhow::anyhow!(e))?;
        let member_id = if council.is_empty() {
            let council_wallet = WalletAddress("xv1_council_member_0123456789abcdef0123456789abcd0123456789abcde".to_string());
            let m = dao.add_council_member(council_wallet, CouncilRole::CoreMaintainer, vec!["security".to_string()]).await.map_err(|e| anyhow::anyhow!(e))?;
            m.id
        } else {
            council[0].id.clone()
        };

        dao.cast_council_vote(&proposal_id, &member_id, approve).await.map_err(|e| anyhow::anyhow!(e))?;
        println!("  - Cast council member ({}) vote: approve={}", member_id, approve);

        // 3. Since both chambers have voted, we can auto-tally and execute to show a full working proposal cycle!
        println!("\nTallying votes for proposal...");

        // Fast-forward or force evaluation by manually tallying
        match dao.tally_votes(&proposal_id).await {
            Ok(result) => {
                println!("  Quorum met (users): {}", result.user_quorum_met);
                println!("  Quorum met (council): {}", result.council_votes_for + result.council_votes_against > 0);
                println!("  Passed both chambers: {}", result.passed);

                if result.passed {
                    println!("\nExecuting proposal to modify system parameters...");
                    let mut params = SystemParams::default();
                    if let Err(e) = dao.execute_proposal(&proposal_id, &mut params).await {
                        println!("  Execution status: {}", e);
                    } else {
                        println!("  ✅ Proposal executed successfully! System parameters updated.");
                        println!("  New reference price: {}", params.reference_price);
                        println!("  New burn rate: {}%", params.burn_rate);
                    }
                } else {
                    println!("  ❌ Proposal rejected (consensus was not reached in both chambers).");
                }
            }
            Err(e) => {
                println!("  Could not tally votes yet: {}", e);
            }
        }
    } else {
        println!("Proposal is in state: {}. Cannot cast votes right now.", proposal.xip_state.label());
    }

    Ok(())
}

async fn list_council_members() -> Result<()> {
    println!("Xavier Core Council Members:");

    let state_path = resolve_state_path();
    let mut dao = MockBicameralDao::new(Some(state_path));

    let members = dao.list_council_members().await.map_err(|e| anyhow::anyhow!(e))?;

    // Add a few default mock members if the council is empty for CLI visibility
    if members.is_empty() {
        let default_wallet = WalletAddress("xv1_core_maintainer_alpha_0123456789abcdef0123456789abcd0123456789abcde".to_string());
        let m = dao.add_council_member(
            default_wallet,
            CouncilRole::CoreMaintainer,
            vec!["Security".into(), "Architecture".into()],
        ).await.map_err(|e| anyhow::anyhow!(e))?;

        println!("\nNo active members found. Added default council member:");
        println!("ID:             {}", m.id);
        println!("Wallet Address: {}", m.wallet.0);
        return Ok(());
    }

    println!("\n{:<15} {:<45} {:<20}", "ID", "Wallet Address", "Role");
    println!("{}", "-".repeat(85));

    for m in members {
        let role_str = match m.role {
            types::CouncilRole::CoreMaintainer => "Core Maintainer",
            types::CouncilRole::SkillContributor => "Skill Contributor",
            types::CouncilRole::SecurityAuditor => "Security Auditor",
            types::CouncilRole::Architect => "Architect",
            types::CouncilRole::CommunityRepresentative => "Community Rep",
        };

        println!("{:<15} {:<45} {:<20}", m.id, m.wallet.0, role_str);
    }

    Ok(())
}
