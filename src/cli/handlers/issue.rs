//! Issue context packager CLI handler

use xavier::codebase::issue_context::{pack_issue, save_pack};
use anyhow::Result;

/// Handle `xavier issue pack <id> [--repo <repo>]`
pub async fn handle_issue_pack(id: &str, repo: Option<String>) -> Result<()> {
    let repo_name = repo.as_deref().unwrap_or("xavier");
    println!("Packaging GitHub issue #{} from repository '{}'...", id, repo_name);

    let pack = pack_issue(id, repo_name).await?;

    let out_dir = "data/issue_packs";
    let out_path = format!("{}/{}.json", out_dir, id);

    save_pack(&pack, &out_path).await?;

    let json_str = serde_json::to_string_pretty(&pack)?;
    println!("{}", json_str);
    println!("\n✅ Issue context pack saved to {}", out_path);

    Ok(())
}
