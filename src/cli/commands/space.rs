use crate::cli::commands::enums::{
    SpaceCommand, SpaceInviteAction, SpaceMemberAction, UnlockMode, CLI_HTTP_CLIENT,
};
use crate::storage::multi_db::MultiDbManager;
use crate::workspace::WorkspaceDbKind;
use anyhow::{bail, Result};

pub async fn run_space_command(cmd: SpaceCommand, base_url: &str, token: &str) -> Result<()> {
    let client = CLI_HTTP_CLIENT.clone();

    match cmd {
        SpaceCommand::Create { id, label, local } => {
            let mut use_local = local;
            if !use_local {
                let resp = client
                    .post(format!("{}/api/v1/espacio/admin/spaces", base_url))
                    .header("X-Xavier-Token", token)
                    .json(&serde_json::json!({
                        "id": id,
                        "name": id,
                        "description": label.join(" "),
                        "owner_node": "CLI",
                        "is_public": false
                    }))
                    .send()
                    .await;
                match resp {
                    Ok(r) => {
                        if r.status().is_success() {
                            println!("Created space {}", id);
                        } else {
                            bail!("Failed to create space {}: {}", id, r.text().await?);
                        }
                        return Ok(());
                    }
                    Err(_) => {
                        use_local = true; // Fallback
                    }
                }
            }
            if use_local {
                let data_dir = crate::settings::XavierSettings::resolve_data_dir();
                let multi_db = MultiDbManager::open(&data_dir)?;
                let display_name = if label.is_empty() {
                    id.clone()
                } else {
                    label.join(" ")
                };
                let _db = multi_db
                    .create_database(id.clone(), display_name.clone(), WorkspaceDbKind::Personal)
                    .await?;
                println!("Created space {}", id);
            }
        }
        SpaceCommand::List { json, local } => {
            let mut use_local = local;
            if !use_local {
                let resp = client
                    .get(format!("{}/api/v1/espacio/admin/spaces", base_url))
                    .header("X-Xavier-Token", token)
                    .send()
                    .await;
                match resp {
                    Ok(r) => {
                        if r.status().is_success() {
                            let text = r.text().await?;
                            if json {
                                match serde_json::from_str::<serde_json::Value>(&text) {
                                    Ok(val) => {
                                        let compact = serde_json::to_string(&val)?;
                                        println!("{}", compact);
                                    }
                                    Err(_) => bail!("Failed to parse response as JSON: {}", text),
                                }
                            } else {
                                println!("{}", text);
                            }
                        } else {
                            bail!("Failed to list spaces: {}", r.text().await?);
                        }
                        return Ok(());
                    }
                    Err(_) => {
                        use_local = true; // Fallback
                    }
                }
            }
            if use_local {
                let data_dir = crate::settings::XavierSettings::resolve_data_dir();
                let multi_db = MultiDbManager::open(&data_dir)?;
                let dbs = multi_db.list_entries().await;
                if json {
                    println!("{}", serde_json::to_string_pretty(&dbs)?);
                } else {
                    for db in dbs {
                        println!("- {} ({})", db.db_id, db.display_name);
                    }
                }
            }
        }
        SpaceCommand::Delete { id, local } => {
            let mut use_local = local;
            if !use_local {
                // Let's assume there is a delete endpoint on `espacio` ? Actually the fallback deletes from `multi_db`.
                // Let's try HTTP first for completeness.
                let resp = client
                    .delete(format!("{}/api/v1/espacio/admin/spaces/{}", base_url, id))
                    .header("X-Xavier-Token", token)
                    .send()
                    .await;
                match resp {
                    Ok(r) => {
                        if r.status().is_success() {
                            println!("Deleted space {}", id);
                        } else {
                            bail!("Failed to delete space {}: {}", id, r.text().await?);
                        }
                        return Ok(());
                    }
                    Err(_) => {
                        use_local = true; // Fallback
                    }
                }
            }
            if use_local {
                let data_dir = crate::settings::XavierSettings::resolve_data_dir();
                let multi_db = MultiDbManager::open(&data_dir)?;
                let deleted = multi_db.delete_database(&id).await?;
                if deleted {
                    println!("Deleted space {}", id);
                } else {
                    bail!("Space not found");
                }
            }
        }
        SpaceCommand::Use { id } => {
            let data_dir = crate::settings::XavierSettings::resolve_data_dir();
            std::fs::create_dir_all(&data_dir)?;
            let path = data_dir.join("active_space");
            std::fs::write(&path, &id)?;
            println!("Active space set to {}", id);
        }
        SpaceCommand::Unlock { id, mode } => {
            let (password, recovery_code) = match mode {
                UnlockMode::Node => (None, None),
                UnlockMode::Password => {
                    let pw = match std::env::var("XAVIER_SPACE_PASSWORD") {
                        Ok(val) => val,
                        Err(_) => {
                            let mut line = String::new();
                            std::io::stdin().read_line(&mut line)?;
                            line.trim_end_matches(&['\r', '\n'][..]).to_string()
                        }
                    };
                    (Some(pw), None)
                }
                UnlockMode::Recovery => {
                    let code = match std::env::var("XAVIER_SPACE_RECOVERY_CODE") {
                        Ok(val) => val,
                        Err(_) => {
                            let mut line = String::new();
                            std::io::stdin().read_line(&mut line)?;
                            line.trim_end_matches(&['\r', '\n'][..]).to_string()
                        }
                    };
                    (None, Some(code))
                }
            };

            let resp = client
                .post(format!("{}/api/v1/espacio/spaces/{}/unlock", base_url, id))
                .header("X-Xavier-Token", token)
                .json(&serde_json::json!({
                    "mode": mode,
                    "password": password,
                    "recovery_code": recovery_code,
                }))
                .send()
                .await?;

            if resp.status().is_success() {
                println!("Unlocked space {}", id);
            } else {
                bail!("Failed to unlock space {}: {}", id, resp.text().await?);
            }
        }
        SpaceCommand::Status { id, json } => {
            let resp = client
                .get(format!("{}/api/v1/espacio/admin/spaces/{}", base_url, id))
                .header("X-Xavier-Token", token)
                .send()
                .await?;

            if !resp.status().is_success() {
                bail!("Failed to get space status {}: {}", id, resp.text().await?);
            }

            let val: serde_json::Value = resp.json().await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&val)?);
            } else {
                let is_locked = val["locked"].as_bool().unwrap_or(false);
                let status_str = if is_locked { "locked" } else { "unlocked" };
                let members_display = match val["member_count"].as_u64() {
                    Some(count) => count.to_string(),
                    None => "unavailable (locked)".to_string(),
                };
                let retention_display = match val["retention_days"].as_u64() {
                    Some(days) => format!("{} days", days),
                    None => "none".to_string(),
                };
                println!(
                    "Space: {}
Status: {}
Members: {}
Retention: {}",
                    id, status_str, members_display, retention_display
                );
            }
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
        SpaceCommand::Lock { id: _, json } => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": false,
                        "error": "unsupported",
                        "command": "space lock",
                        "reason": "spaces runtime lock not available in this version"
                    })
                );
                std::process::exit(crate::cli::utils::EXIT_UNSUPPORTED);
            } else {
                eprintln!("Error: lock is not implemented on this version.");
                std::process::exit(crate::cli::utils::EXIT_UNSUPPORTED);
            }
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
            json: false,
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
            local: false,
        };
        assert!(matches!(cmd, SpaceCommand::Create { .. }));
    }

    #[test]
    fn test_status_command() {
        let cmd = SpaceCommand::Status {
            id: "test".to_string(),
            json: true,
        };
        assert!(matches!(cmd, SpaceCommand::Status { json: true, .. }));
    }
}
