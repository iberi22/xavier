//! CLI navigation command handlers (ls, cd, pwd)

use crate::cli::commands::enums::CLI_HTTP_CLIENT;
use crate::cli::config::{
    auth_failed_error, auth_failed_message, is_auth_failure, require_xavier_token, resolve_base_url,
    resolve_cwd, save_cwd,
};
use crate::memory::graph_traversal::AffectedNode;
use crate::memory::qmd::types::NavEntry;
use anyhow::Result;

fn nav_auth_or_fail(status: reqwest::StatusCode, body: &str, op: &str) -> Result<()> {
    if is_auth_failure(status) {
        eprintln!("{}", auth_failed_message(status.as_u16()));
        Err(auth_failed_error(status.as_u16()))
    } else {
        println!("{op} failed: {body}");
        Err(anyhow::anyhow!("{op} failed with HTTP {}", status.as_u16()))
    }
}

/// Handle ls.
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
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        nav_auth_or_fail(status, &text, "ls")
    }
}

/// Handle visualize.
pub async fn handle_visualize(
    format: String,
    show_hotspots: bool,
    show_tree: bool,
    output_file: Option<std::path::PathBuf>,
) -> Result<()> {
    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .get(format!("{}/v1/nav/visualize", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return nav_auth_or_fail(status, &text, "visualize");
    }

    let body: serde_json::Value = response.json().await?;
    let output_text = if format == "json" {
        serde_json::to_string_pretty(&body)?
    } else {
        let mut lines = Vec::new();

        lines.push(format!(
            "Memory Graph Visualization for workspace: {}",
            body["workspace_id"]
        ));

        let hotspots_val = body["hotspots"].as_array();

        // --hotspots: show top hotspots section prominently
        if show_hotspots {
            lines.push("\n[🔥 Hotspots]".to_string());
            if let Some(hs) = hotspots_val {
                if hs.is_empty() {
                    lines.push("  (none)".to_string());
                } else {
                    for (i, entry) in hs.iter().enumerate() {
                        let node = entry[0].as_str().unwrap_or("(unknown)");
                        let count = entry[1]["count"].as_u64().unwrap_or(0);
                        lines.push(format!("  {}. {} ({} visits)", i + 1, node, count));
                    }
                }
            } else {
                lines.push("  (none)".to_string());
            }
        }

        lines.push("\n[Navigation Weights]".to_string());
        lines.push(format!(
            "  Working:  {:.4}",
            body["weights"]["working"].as_f64().unwrap_or(0.0)
        ));
        lines.push(format!(
            "  Episodic: {:.4}",
            body["weights"]["episodic"].as_f64().unwrap_or(0.0)
        ));
        lines.push(format!(
            "  Semantic: {:.4}",
            body["weights"]["semantic"].as_f64().unwrap_or(0.0)
        ));

        lines.push("\n[Traversal Weights]".to_string());
        let tw = &body["traversal_weights"];
        lines.push(format!(
            "  Semantic Sim: {:.4}",
            tw["semantic_similarity"].as_f64().unwrap_or(0.0)
        ));
        lines.push(format!(
            "  Confidence:   {:.4}",
            tw["confidence"].as_f64().unwrap_or(0.0)
        ));
        lines.push(format!(
            "  Edge Weight:  {:.4}",
            tw["edge_weight"].as_f64().unwrap_or(0.0)
        ));
        lines.push(format!(
            "  Recency:      {:.4}",
            tw["recency"].as_f64().unwrap_or(0.0)
        ));
        lines.push(format!(
            "  Cross-Layer:  {:.4}",
            tw["cross_layer"].as_f64().unwrap_or(0.0)
        ));
        lines.push(format!(
            "  Cross-Dir:    {:.4}",
            tw["cross_dir"].as_f64().unwrap_or(0.0)
        ));
        lines.push(format!(
            "  Periph-Hub:   {:.4}",
            tw["peripheral_hub"].as_f64().unwrap_or(0.0)
        ));

        // Build hotspot map from the response
        use std::collections::HashMap;
        let mut hotspot_map = HashMap::new();
        if let Some(hs) = hotspots_val {
            for entry in hs {
                if let (Some(node), Some(count)) = (entry[0].as_str(), entry[1]["count"].as_u64()) {
                    hotspot_map.insert(node.to_string(), count);
                }
            }
        }

        // Extract HORMER scores from the response (if the API returns them)
        let hormer_scores: HashMap<String, f64> = body["hormer_scores"]
            .as_object()
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(0.0)))
                    .collect()
            })
            .unwrap_or_default();

        if show_tree {
            lines.push("\n[Directory Tree — HORMER Scores]".to_string());
        } else {
            lines.push("\n[Directory Structure]".to_string());
        }

        let docs: Vec<xavier::memory::qmd::MemoryDocument> =
            serde_json::from_value(body["documents"].clone())?;

        // --tree flag: pass HORMER scores for enriched rendering
        let tree_lines = render_tree_lines(&docs, &hotspot_map, &hormer_scores, show_tree);
        lines.extend(tree_lines);

        lines.push("\n[Belief Graph Edges]".to_string());
        let edges: Vec<xavier::domain::memory::belief::BeliefEdge> =
            serde_json::from_value(body["edges"].clone())?;
        if edges.is_empty() {
            lines.push("  (no edges)".to_string());
        } else {
            for edge in &edges {
                lines.push(format!(
                    "  {} --[{}]--> {} (conf: {:.2})",
                    edge.source, edge.relation_type, edge.target, edge.confidence_score
                ));
            }
        }

        // Add Telemetry Metrics section to visualize if available
        if let Some(metrics) = body.get("metrics") {
            lines.push("\n[Navigation Telemetry]".to_string());
            lines.push(format!(
                "  Total Visits:     {}",
                metrics["total_visits"].as_u64().unwrap_or(0)
            ));
            lines.push(format!(
                "  Unique Nodes:     {}",
                metrics["unique_nodes"].as_u64().unwrap_or(0)
            ));
            lines.push(format!(
                "  Avg Path Length:  {:.2}",
                metrics["avg_path_length"].as_f64().unwrap_or(0.0)
            ));
            if let Some(hist) = metrics["nav_score_histogram"].as_array() {
                lines.push("  Nav Score Hist:   ".to_string());
                let mut hist_str = String::new();
                for (i, val) in hist.iter().enumerate() {
                    if i > 0 {
                        hist_str.push_str(", ");
                    }
                    hist_str.push_str(&format!("{}:{}", i, val.as_u64().unwrap_or(0)));
                }
                lines.push(format!("    [{}]", hist_str));
            }
        }

        lines.join("\n")
    };

    // --output <file>: write to file instead of stdout
    match output_file {
        Some(path) => {
            std::fs::write(&path, &output_text)?;
            println!("✅ visualize output written to: {}", path.display());
        }
        None => {
            println!("{}", output_text);
        }
    }

    Ok(())
}

/// Build a tree as a list of text lines.
/// When `show_hormer` is true, HORMER scores are appended after each doc/dir name.
fn render_tree_lines(
    docs: &[xavier::memory::qmd::MemoryDocument],
    hotspots: &std::collections::HashMap<String, u64>,
    hormer_scores: &std::collections::HashMap<String, f64>,
    show_hormer: bool,
) -> Vec<String> {
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Node {
        children: BTreeMap<String, Node>,
        is_doc: bool,
        full_path: String,
    }

    let mut root = Node::default();
    for doc in docs {
        let mut curr = &mut root;
        let parts: Vec<&str> = doc.path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_full_path = String::new();
        for (i, part) in parts.iter().enumerate() {
            if !current_full_path.is_empty() {
                current_full_path.push('/');
            }
            current_full_path.push_str(part);
            curr = curr.children.entry(part.to_string()).or_default();
            curr.full_path = current_full_path.clone();
            if i == parts.len() - 1 {
                curr.is_doc = true;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn print_node_lines(
        name: &str,
        node: &Node,
        indent: &str,
        is_last: bool,
        hotspots: &std::collections::HashMap<String, u64>,
        hormer_scores: &std::collections::HashMap<String, f64>,
        show_hormer: bool,
        lines: &mut Vec<String>,
    ) {
        let branch = if is_last { "`-- " } else { "|-- " };
        let prefix = if node.is_doc { "DOC " } else { "DIR " };
        let visits = hotspots.get(&node.full_path).copied().unwrap_or(0);
        let hormer = hormer_scores.get(&node.full_path).copied().unwrap_or(0.0);

        let snippet = if show_hormer && hormer > 0.0 {
            format!(
                "{}{}{}{} ({} visits, H={:.4})",
                indent, branch, prefix, name, visits, hormer
            )
        } else if visits > 0 {
            format!("{}{}{}{} ({} visits)", indent, branch, prefix, name, visits)
        } else if show_hormer && hormer_scores.contains_key(&node.full_path) {
            format!("{}{}{}{} (H={:.4})", indent, branch, prefix, name, hormer)
        } else {
            format!("{}{}{}{}", indent, branch, prefix, name)
        };
        lines.push(snippet);

        let new_indent = format!("{}{}", indent, if is_last { "    " } else { "|   " });
        let count = node.children.len();
        for (i, (child_name, child_node)) in node.children.iter().enumerate() {
            print_node_lines(
                child_name,
                child_node,
                &new_indent,
                i == count - 1,
                hotspots,
                hormer_scores,
                show_hormer,
                lines,
            );
        }
    }

    let mut lines = Vec::new();
    if root.children.is_empty() {
        lines.push("  (no documents)".to_string());
        return lines;
    }

    let count = root.children.len();
    for (i, (name, node)) in root.children.iter().enumerate() {
        print_node_lines(
            name,
            node,
            "",
            i == count - 1,
            hotspots,
            hormer_scores,
            show_hormer,
            &mut lines,
        );
    }

    lines
}

fn render_tree(
    docs: &[xavier::memory::qmd::MemoryDocument],
    hotspots: &std::collections::HashMap<String, u64>,
) {
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Node {
        children: BTreeMap<String, Node>,
        is_doc: bool,
        full_path: String,
        score: Option<f32>,
    }

    let mut root = Node::default();
    for doc in docs {
        let mut curr = &mut root;
        let parts: Vec<&str> = doc.path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_full_path = String::new();
        for (i, part) in parts.iter().enumerate() {
            if !current_full_path.is_empty() {
                current_full_path.push('/');
            }
            current_full_path.push_str(part);
            curr = curr.children.entry(part.to_string()).or_default();
            curr.full_path = current_full_path.clone();
            if i == parts.len() - 1 {
                curr.is_doc = true;
                curr.score = doc
                    .metadata
                    .get("memory_importance")
                    .or_else(|| doc.metadata.get("quality").and_then(|q| q.get("overall")))
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32);
            }
        }
    }

    fn print_node(
        name: &str,
        node: &Node,
        indent: &str,
        is_last: bool,
        hotspots: &std::collections::HashMap<String, u64>,
    ) {
        let branch = if is_last { "`-- " } else { "|-- " };
        let prefix = if node.is_doc { "DOC " } else { "DIR " };
        let visits = hotspots.get(&node.full_path).copied().unwrap_or(0);

        let mut meta_str = String::new();
        if let Some(s) = node.score {
            meta_str.push_str(&format!(" [score: {:.2}]", s));
        }
        if visits > 0 {
            meta_str.push_str(&format!(" ({} visits)", visits));
            if visits > 5 {
                meta_str.push_str(" 🔥"); // Hotspot highlight
            }
        }

        println!("{}{}{}{}{}", indent, branch, prefix, name, meta_str);

        let new_indent = format!("{}{}", indent, if is_last { "    " } else { "|   " });
        let count = node.children.len();
        for (i, (child_name, child_node)) in node.children.iter().enumerate() {
            print_node(
                child_name,
                child_node,
                &new_indent,
                i == count - 1,
                hotspots,
            );
        }
    }

    if root.children.is_empty() {
        println!("  (no documents)");
        return;
    }

    let count = root.children.len();
    for (i, (name, node)) in root.children.iter().enumerate() {
        print_node(name, node, "", i == count - 1, hotspots);
    }
}

/// Handle cd.
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
        Ok(())
    } else if response.status() == 404 {
        println!("cd failed: Path not found: {}", normalized_target);
        Err(anyhow::anyhow!("cd path not found: {normalized_target}"))
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        nav_auth_or_fail(status, &text, "cd")
    }
}

/// Handle pwd.
pub async fn handle_pwd() -> Result<()> {
    let cwd = resolve_cwd();
    println!("{}", cwd);
    Ok(())
}

/// Handle telemetry.
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
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        nav_auth_or_fail(status, &text, "telemetry")
    }
}

/// Handle affected.
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
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        nav_auth_or_fail(status, &text, "affected")
    }
}
