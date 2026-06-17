//! CLI navigation command handlers (ls, cd, pwd)

use crate::cli::commands::enums::CLI_HTTP_CLIENT;
use crate::cli::config::{require_xavier_token, resolve_base_url, resolve_cwd, save_cwd};
use crate::memory::graph_traversal::AffectedNode;
use crate::memory::qmd::types::NavEntry;
use anyhow::Result;

pub async fn handle_ls(path: Option<String>) -> Result<()> {
    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CLI_HTTP_CLIENT.clone();

    let cwd = resolve_cwd();
    let effective_path = match path {
        Some(p) if p.starts_with('/') => p,
        Some(p) => {
            if cwd == "/" {
                format!("/{}", p)
            } else {
                format!("{}/{}", cwd, p)
            }
        }
        None => cwd.clone(),
    };

    let response = client
        .get(format!("{}/v1/nav/ls?path={}", base_url, effective_path))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if response.status().is_success() {
        let body: serde_json::Value = response.json().await?;
        let entries: Vec<NavEntry> = serde_json::from_value(body["entries"].clone())?;

        println!("Contents of {}:", body["path"]);
        if entries.is_empty() {
            println!("  (empty)");
        } else {
            for entry in entries {
                let prefix = if entry.is_dir { "DIR " } else { "DOC " };
                println!("  {} {}", prefix, entry.name);
            }
        }
    } else {
        println!("ls failed: {}", response.text().await?);
    }
    Ok(())
}

pub async fn handle_visualize(format: String) -> Result<()> {
    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .get(format!("{}/v1/nav/visualize", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if response.status().is_success() {
        let body: serde_json::Value = response.json().await?;

        if format == "json" {
            println!("{}", serde_json::to_string_pretty(&body)?);
        } else {
            println!(
                "Memory Graph Visualization for workspace: {}",
                body["workspace_id"]
            );
            println!("\n[Navigation Weights]");
            println!(
                "  Working:  {:.4}",
                body["weights"]["working"].as_f64().unwrap_or(0.0)
            );
            println!(
                "  Episodic: {:.4}",
                body["weights"]["episodic"].as_f64().unwrap_or(0.0)
            );
            println!(
                "  Semantic: {:.4}",
                body["weights"]["semantic"].as_f64().unwrap_or(0.0)
            );

            println!("\n[Directory Structure]");
            let docs: Vec<xavier::memory::qmd::MemoryDocument> =
                serde_json::from_value(body["documents"].clone())?;
            render_tree(&docs);

            println!("\n[Belief Graph Edges]");
            let edges: Vec<xavier::domain::memory::belief::BeliefEdge> =
                serde_json::from_value(body["edges"].clone())?;
            if edges.is_empty() {
                println!("  (no edges)");
            } else {
                for edge in edges {
                    println!(
                        "  {} --[{}]--> {} (conf: {:.2})",
                        edge.source, edge.relation_type, edge.target, edge.confidence_score
                    );
                }
            }
        }
    } else {
        println!("visualize failed: {}", response.text().await?);
    }
    Ok(())
}

fn render_tree(docs: &[xavier::memory::qmd::MemoryDocument]) {
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Node {
        children: BTreeMap<String, Node>,
        is_doc: bool,
    }

    let mut root = Node::default();
    for doc in docs {
        let mut curr = &mut root;
        let parts: Vec<&str> = doc.path.split('/').filter(|s| !s.is_empty()).collect();
        for (i, part) in parts.iter().enumerate() {
            curr = curr.children.entry(part.to_string()).or_default();
            if i == parts.len() - 1 {
                curr.is_doc = true;
            }
        }
    }

    fn print_node(name: &str, node: &Node, indent: &str, is_last: bool) {
        let branch = if is_last { "`-- " } else { "|-- " };
        let prefix = if node.is_doc { "DOC " } else { "DIR " };
        println!("{}{}{}{}", indent, branch, prefix, name);

        let new_indent = format!("{}{}", indent, if is_last { "    " } else { "|   " });
        let count = node.children.len();
        for (i, (child_name, child_node)) in node.children.iter().enumerate() {
            print_node(child_name, child_node, &new_indent, i == count - 1);
        }
    }

    if root.children.is_empty() {
        println!("  (no documents)");
        return;
    }

    let count = root.children.len();
    for (i, (name, node)) in root.children.iter().enumerate() {
        print_node(name, node, "", i == count - 1);
    }
}

pub async fn handle_cd(path: String) -> Result<()> {
    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CLI_HTTP_CLIENT.clone();

    let cwd = resolve_cwd();
    let target_path = if path == ".." {
        if cwd == "/" {
            "/".to_string()
        } else {
            let mut parts: Vec<&str> = cwd.split('/').filter(|s| !s.is_empty()).collect();
            if !parts.is_empty() {
                parts.pop();
            }
            if parts.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", parts.join("/"))
            }
        }
    } else if path.starts_with('/') {
        path
    } else if cwd == "/" {
        format!("/{}", path)
    } else {
        format!("{}/{}", cwd, path)
    };

    let mut normalized_target = target_path;
    if normalized_target.len() > 1 && normalized_target.ends_with('/') {
        normalized_target.pop();
    }

    let response = client
        .post(format!("{}/v1/nav/cd", base_url))
        .header("X-Xavier-Token", &token)
        .json(&serde_json::json!({ "path": normalized_target }))
        .send()
        .await?;

    if response.status().is_success() {
        save_cwd(&normalized_target)?;
        println!("Current directory changed to: {}", normalized_target);
    } else if response.status() == 404 {
        println!("cd failed: Path not found: {}", normalized_target);
    } else {
        println!("cd failed: {}", response.text().await?);
    }
    Ok(())
}

pub async fn handle_pwd() -> Result<()> {
    let cwd = resolve_cwd();
    println!("{}", cwd);
    Ok(())
}

pub async fn handle_telemetry(kind: Option<String>) -> Result<()> {
    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CLI_HTTP_CLIENT.clone();

    let hotspots_mode = matches!(
        kind.as_deref().map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("hotspots")
    );
    let top = 10usize;
    let url = format!(
        "{}/v1/nav/telemetry?hotspots={}&top={}",
        base_url, hotspots_mode, top
    );

    let response = client
        .get(url)
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if response.status().is_success() {
        let body: serde_json::Value = response.json().await?;
        if hotspots_mode {
            println!("Top {} Navigation Hotspots:", top);
            let hotspots = body["hotspots"].as_array();
            match hotspots {
                Some(entries) if !entries.is_empty() => {
                    println!("{:<40} | {:<8}", "Node", "Visits");
                    println!("{:-<40}-+-{:-<8}", "", "");
                    for entry in entries {
                        // Hotspots serialize as [node, VisitInfo] tuples.
                        let node = entry[0].as_str().unwrap_or("(unknown)");
                        let count = entry[1]["count"].as_u64().unwrap_or(0);
                        println!("{:<40} | {:<8}", node, count);
                    }
                }
                _ => println!("  (no nodes visited yet)"),
            }
        } else {
            let t = &body["telemetry"];
            println!("Navigation Telemetry Summary:");
            println!(
                "  Total visits:     {}",
                t["total_visits"].as_u64().unwrap_or(0)
            );
            println!(
                "  Unique nodes:     {}",
                t["unique_nodes"].as_u64().unwrap_or(0)
            );
            println!(
                "  Paths recorded:   {}",
                t["total_paths"].as_u64().unwrap_or(0)
            );
            println!(
                "  Avg path length:  {:.2}",
                t["avg_path_length"].as_f64().unwrap_or(0.0)
            );

            let hotspots = t["hotspots"].as_array();
            if let Some(entries) = hotspots {
                if !entries.is_empty() {
                    println!("\nTop {} Hotspots:", entries.len());
                    println!("{:<40} | {:<8}", "Node", "Visits");
                    println!("{:-<40}-+-{:-<8}", "", "");
                    for entry in entries {
                        // Hotspots serialize as [node, VisitInfo] tuples.
                        let node = entry[0].as_str().unwrap_or("(unknown)");
                        let count = entry[1]["count"].as_u64().unwrap_or(0);
                        println!("{:<40} | {:<8}", node, count);
                    }
                }
            }
        }
    } else {
        println!("telemetry failed: {}", response.text().await?);
    }
    Ok(())
}

pub async fn handle_affected(
    path: String,
    depth: usize,
    format: String,
    exclude_file_type: Option<String>,
) -> Result<()> {
    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CLI_HTTP_CLIENT.clone();

    let cwd = resolve_cwd();
    let effective_path = if path.starts_with('/') || path.contains("::") || !path.contains('/') {
        path
    } else if cwd == "/" {
        format!("/{}", path)
    } else {
        format!("{}/{}", cwd, path)
    };

    let mut url = format!(
        "{}/v1/nav/affected?path={}&depth={}",
        base_url, effective_path, depth
    );
    if let Some(exclude) = exclude_file_type {
        url.push_str(&format!("&exclude_file_type={}", exclude));
    }

    let response = client
        .get(url)
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if response.status().is_success() {
        let body: serde_json::Value = response.json().await?;
        let affected: Vec<AffectedNode> = serde_json::from_value(body["affected"].clone())?;

        if format == "json" {
            println!("{}", serde_json::to_string_pretty(&affected)?);
        } else {
            println!("Nodes affected by change in {}:", body["path"]);
            if affected.is_empty() {
                println!("  (none found within depth {})", depth);
            } else {
                println!("{:<40} | {:<20} | {:<5}", "Node", "Relation", "Depth");
                println!("{:-<40}-+-{:-<20}-+-{:-<5}", "", "", "");
                for item in affected {
                    println!(
                        "{:<40} | {:<20} | {:<5}",
                        item.node, item.relation, item.depth
                    );
                }
            }
        }
    } else {
        println!("affected failed: {}", response.text().await?);
    }
    Ok(())
}
