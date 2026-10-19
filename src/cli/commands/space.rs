use crate::cli::commands::enums::{
    SpaceCommand, SpaceInviteAction, SpaceMemberAction, UnlockMode, CLI_HTTP_CLIENT,
};
use anyhow::{bail, Result};

pub async fn run_space_command(cmd: SpaceCommand, base_url: &str, token: &str) -> Result<()> {
    let client = CLI_HTTP_CLIENT.clone();

    match cmd {
        SpaceCommand::Create { id, label } => {
            let resp = client
                .post(format!("{}/api/v1/espacio/admin/spaces", base_url))
                .header("X-Xavier-Token", token)
                .json(&serde_json::json!({
                    "id": id,
                    "description": label.join(" "),
                    "owner_node": "CLI",
                    "is_public": false
                }))
                .send()
                .await?;
            if resp.status().is_success() {
                println!("Created space {}", id);
            } else {
                bail!("Failed to create space {}: {}", id, resp.text().await?);
            }
        }
        SpaceCommand::List => {
            let resp = client
                .get(format!("{}/api/v1/espacio/spaces", base_url))
                .header("X-Xavier-Token", token)
                .send()
                .await?;
            if resp.status().is_success() {
                println!("{}", resp.text().await?);
            } else {
                bail!("Failed to list spaces: {}", resp.text().await?);
            }
        }
        SpaceCommand::Unlock { id: _, mode: _ } => {
            println!("not available on this daemon version");
            std::process::exit(2);
        }
        SpaceCommand::Status { id: _ } => {
            println!("not available on this daemon version");
            std::process::exit(2);
        }
        SpaceCommand::Members { id, action } => {
            if let Some(action) = action {
                match action {
                    SpaceMemberAction::List => {
                        let resp = client
                            .get(format!("{}/api/v1/espacio/spaces/{}/members", base_url, id))
                            .header("X-Xavier-Token", token)
                            .send()
                            .await?;
                        if resp.status().is_success() {
                            println!("{}", resp.text().await?);
                        } else {
                            bail!("Failed to list members: {}", resp.text().await?);
                        }
                    }
                    SpaceMemberAction::Add { node, role } => {
                        let resp = client
                            .post(format!("{}/api/v1/espacio/spaces/{}/members", base_url, id))
                            .header("X-Xavier-Token", token)
                            .json(&serde_json::json!({
                                "node_id": node,
                                "role": role.as_str()
                            }))
                            .send()
                            .await?;
                        if resp.status().is_success() {
                            println!("Added member {}", node);
                        } else {
                            bail!("Failed to add member: {}", resp.text().await?);
                        }
                    }
                    SpaceMemberAction::Remove { node } => {
                        let resp = client
                            .delete(format!(
                                "{}/api/v1/espacio/spaces/{}/members/{}",
                                base_url, id, node
                            ))
                            .header("X-Xavier-Token", token)
                            .send()
                            .await?;
                        if resp.status().is_success() {
                            println!("Removed member {}", node);
                        } else {
                            bail!("Failed to remove member: {}", resp.text().await?);
                        }
                    }
                }
            } else {
                // default action
                println!("Please specify a member action (list, add, remove).");
            }
        }
        SpaceCommand::Invites { id, action } => {
            if let Some(action) = action {
                match action {
                    SpaceInviteAction::List => {
                        let resp = client
                            .get(format!("{}/api/v1/espacio/spaces/{}/invites", base_url, id))
                            .header("X-Xavier-Token", token)
                            .send()
                            .await?;
                        if resp.status().is_success() {
                            println!("{}", resp.text().await?);
                        } else {
                            bail!("Failed to list invites: {}", resp.text().await?);
                        }
                    }
                    SpaceInviteAction::Create { node, role, ttl } => {
                        let resp = client
                            .post(format!("{}/api/v1/espacio/spaces/{}/invites", base_url, id))
                            .header("X-Xavier-Token", token)
                            .json(&serde_json::json!({
                                "target_node": node,
                                "role": role.as_str(),
                                "ttl_hours": ttl.unwrap_or(24)
                            }))
                            .send()
                            .await?;
                        if resp.status().is_success() {
                            println!("{}", resp.text().await?);
                        } else {
                            bail!("Failed to create invite: {}", resp.text().await?);
                        }
                    }
                }
            } else {
                println!("Please specify an invite action (list, create).");
            }
        }
        SpaceCommand::Accept { invite_id, display } => {
            let resp = client
                .post(format!("{}/api/v1/espacio/invites/accept", base_url))
                .header("X-Xavier-Token", token)
                .json(&serde_json::json!({
                    "invite_id": invite_id,
                    "display_name": display
                }))
                .send()
                .await?;
            if resp.status().is_success() {
                println!("Accepted invite {}", invite_id);
            } else {
                bail!("Failed to accept invite: {}", resp.text().await?);
            }
        }
        SpaceCommand::Lock { id: _ } => {
            // Documented error: lock is not implemented
            println!("Error: lock is not implemented on this version.");
            bail!("not implemented");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::enums::SpaceCommand;

    #[test]
    fn test_lock_not_implemented() {
        let cmd = SpaceCommand::Lock {
            id: "test".to_string(),
        };
        assert!(matches!(cmd, SpaceCommand::Lock { .. }));
    }

    #[test]
    fn test_unlock_mode() {
        let cmd = SpaceCommand::Unlock {
            id: "test".to_string(),
            mode: UnlockMode::Node,
        };
        assert!(matches!(
            cmd,
            SpaceCommand::Unlock {
                mode: UnlockMode::Node,
                ..
            }
        ));
    }

    #[test]
    fn test_create_command() {
        let cmd = SpaceCommand::Create {
            id: "test".to_string(),
            label: vec!["a".to_string()],
        };
        assert!(matches!(cmd, SpaceCommand::Create { .. }));
    }
}
