// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Mesh Command Handlers — CLI implementation for Xavier Mesh

use crate::cli::commands::MeshCommand;
use crate::cli::config::resolve_http_token;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use xavier::mesh::{
    pairing_registry::PairingSecretRegistry, MeshTransport, NodeId, NodeIdentity, PeerInfo,
    PeerRegistry,
};
use xavier::sync::SyncTransport;

pub async fn handle_mesh_command(cmd: MeshCommand) -> Result<()> {
    // License check for Mesh features
    let settings = xavier::settings::XavierSettings::current();
    xavier::security::license::require_mesh_license(&settings).map_err(|e| anyhow::anyhow!(e))?;

    match cmd {
        MeshCommand::Id => {
            let identity = NodeIdentity::load_or_create()?;
            println!("Xavier Mesh Identity:");
            println!("  NodeID:     {}", identity.node_id);
            println!(
                "  Public Key: {}",
                xavier::crypto::hex_encode(&identity.public_key)
            );
            println!("  Version:    {}", env!("CARGO_PKG_VERSION"));
        }
        MeshCommand::AddPeer {
            node_id,
            endpoint,
            alias,
            cloud,
        } => {
            let node_id = NodeId::parse(&node_id)?;
            let mut registry = PeerRegistry::load()?;

            let peer = PeerInfo {
                node_id: node_id.clone(),
                alias: alias.clone(),
                endpoint_url: endpoint.clone(),
                public_key_hex: String::new(), // Will be filled on first handshake
                added_at: chrono::Utc::now().timestamp(),
                last_seen_at: None,
                sync_enabled: true,
                is_cloud: cloud,
                iroh_addr: None,
                shared_workspace_ids: Vec::new(),
                shared_workspace_tokens: std::collections::HashMap::new(),
            };

            registry.add_peer(peer)?;
            println!("✅ Added peer {} ({})", node_id, endpoint);
        }
        MeshCommand::List => {
            let registry = PeerRegistry::load()?;
            let peers = registry.list_peers();

            if peers.is_empty() {
                println!("No peers registered. Use 'xavier mesh add-peer' to add one.");
                return Ok(());
            }

            println!("{:<30} {:<30} {:<10}", "NodeID", "Endpoint", "Status");
            println!("{}", "-".repeat(70));
            for peer in peers {
                let status = if peer.sync_enabled {
                    "Enabled"
                } else {
                    "Disabled"
                };
                println!(
                    "{:<30} {:<30} {:<10}",
                    peer.node_id.as_str(),
                    peer.endpoint_url,
                    status
                );
            }
        }
        MeshCommand::RemovePeer { node_id } => {
            let node_id = NodeId::parse(&node_id)?;
            let mut registry = PeerRegistry::load()?;
            registry.remove_peer(&node_id)?;
            println!("✅ Removed peer {}", node_id);
        }
        MeshCommand::Ping { node_id } => {
            let node_id = NodeId::parse(&node_id)?;
            let registry = PeerRegistry::load()?;
            let peer = registry
                .get_peer(&node_id)
                .context("Peer not found in registry")?;

            let identity = Arc::new(NodeIdentity::load_or_create()?);
            let transport = SyncTransport::for_peer(peer, identity)?;
            let token = resolve_http_token().unwrap_or_default();

            println!("Pinging {} at {}...", node_id, peer.endpoint_url);
            match transport.handshake(&peer.endpoint_url, &token).await {
                Ok(resp) => {
                    println!("✅ Pong! Remote node identity verified.");
                    println!("   Remote NodeID: {}", resp.node_id);

                    // Update registry with public key if it was missing
                    let mut registry = PeerRegistry::load()?;
                    if let Some(p) = registry.get_peer(&node_id) {
                        let mut updated_peer = p.clone();
                        updated_peer.public_key_hex = resp.public_key_hex;
                        updated_peer.last_seen_at = Some(chrono::Utc::now().timestamp());
                        registry.add_peer(updated_peer)?;
                    }
                }
                Err(e) => {
                    println!("❌ Ping failed: {}", e);
                }
            }
        }
        MeshCommand::Sync { node_id, mode } => {
            let node_id = NodeId::parse(&node_id)?;
            let registry = PeerRegistry::load()?;
            let peer = registry
                .get_peer(&node_id)
                .context("Peer not found in registry")?;

            let identity = Arc::new(NodeIdentity::load_or_create()?);
            let transport = SyncTransport::for_peer(peer, identity.clone())?;

            let token = resolve_http_token().unwrap_or_default();

            println!("Starting sync with {} (mode: {})...", node_id, mode);

            // Phase 1: Simple full pull/push
            if mode == "pull" || mode == "bidirectional" {
                println!("Fetching manifest...");
                let manifest = transport.fetch_manifest(peer, &token).await?;
                let hashes: Vec<String> = manifest.chunks.iter().map(|c| c.hash.clone()).collect();

                if hashes.is_empty() {
                    println!("Remote node has no chunks.");
                } else {
                    println!("Fetching {} chunks...", hashes.len());
                    let chunks: HashMap<String, Vec<u8>> =
                        transport.fetch_chunks(peer, &token, &hashes).await?;

                    println!("Importing chunks...");
                    // Using the same logic as v1_mesh_chunks_push but locally
                    let _identity = NodeIdentity::load_or_create()?;
                    let config = xavier::workspace::WorkspaceConfig::from_env();
                    let runtime_config = xavier::agents::RuntimeConfig::from_env();
                    let workspace_dir = dirs::config_dir()
                        .context("Could not determine config directory")?
                        .join("xavier")
                        .join("workspaces")
                        .join(&config.id);

                    let workspace = xavier::workspace::WorkspaceState::new(
                        config,
                        runtime_config,
                        workspace_dir,
                    )
                    .await?;

                    let sync_dir = workspace
                        .usage_state_path
                        .parent()
                        .unwrap_or(&workspace.usage_state_path)
                        .join("sync");
                    let chunks_dir = sync_dir.join("chunks");
                    std::fs::create_dir_all(&chunks_dir)?;

                    let mut imported_count = 0;
                    for (hash, data) in chunks {
                        let chunk_path = chunks_dir.join(format!("{}.jsonl.gz", hash));
                        if std::fs::write(&chunk_path, &data).is_ok() {
                            if let Ok(docs) =
                                xavier::sync::chunks::import_from_chunk(&sync_dir, &hash)
                            {
                                for doc in docs {
                                    if let Err(e) = workspace
                                        .memory
                                        .add_document_typed(
                                            doc.path,
                                            doc.content,
                                            doc.metadata,
                                            None,
                                        )
                                        .await
                                    {
                                        eprintln!("Failed to import document: {}", e);
                                    } else {
                                        imported_count += 1;
                                    }
                                }
                            }
                        }
                    }
                    println!(
                        "✅ Pull sync complete. Imported {} documents from {} chunks.",
                        imported_count,
                        hashes.len()
                    );
                }
            }

            if mode == "push" || mode == "bidirectional" {
                println!("Preparing local chunks for push...");
                let config = xavier::workspace::WorkspaceConfig::from_env();
                let runtime_config = xavier::agents::RuntimeConfig::from_env();
                let workspace_dir = dirs::config_dir()
                    .context("Could not determine config directory")?
                    .join("xavier")
                    .join("workspaces")
                    .join(&config.id);

                let workspace =
                    xavier::workspace::WorkspaceState::new(config, runtime_config, workspace_dir)
                        .await?;

                let sync_dir = workspace
                    .usage_state_path
                    .parent()
                    .unwrap_or(&workspace.usage_state_path)
                    .join("sync");

                // Export local docs to chunks
                let docs = workspace.memory.all_documents().await;
                if docs.is_empty() {
                    println!("No local memories to push.");
                } else {
                    let mut manifest = xavier::sync::chunks::load_manifest(&sync_dir)?;
                    let hash =
                        xavier::sync::chunks::export_to_chunk(&sync_dir, &docs, &mut manifest)?;

                    let chunk_path = sync_dir.join("chunks").join(format!("{}.jsonl.gz", hash));
                    let data = std::fs::read(chunk_path)?;

                    println!("Pushing 1 chunk ({} docs) to {}...", docs.len(), node_id);
                    let pushed = transport.push_chunks(peer, &token, &[(hash, data)]).await?;

                    // Publish manifest if in cloud mode
                    let mesh_manifest = xavier::mesh::protocol::MeshManifest {
                        node_id: identity.node_id.clone(),
                        chunks: manifest
                            .chunks
                            .values()
                            .map(|c| xavier::mesh::protocol::ChunkRef {
                                hash: c.hash.clone(),
                                document_count: c.document_ids.len(),
                                created_at: c.created_at,
                            })
                            .collect(),
                        generated_at: chrono::Utc::now().timestamp(),
                    };
                    transport.publish_manifest(&mesh_manifest).await?;

                    println!(
                        "✅ Push sync complete. Remote accepted {} chunks.",
                        pushed.len()
                    );
                }
            }
        }
        MeshCommand::PairingCode { endpoint } => {
            let identity = NodeIdentity::load_or_create()?;
            let endpoint = endpoint.unwrap_or_else(|| "http://localhost:8006".to_string());
            let (code, secret) = xavier::mesh::pairing::generate_pairing_code(
                identity.node_id.clone(),
                endpoint,
                xavier::crypto::hex_encode(&identity.public_key),
            );

            // Register the secret locally
            let mut secret_registry = PairingSecretRegistry::load()?;
            let expires_at = xavier::mesh::pairing::decode_pairing_code(&code)?.expires_at;
            secret_registry.register_secret(secret.clone(), expires_at)?;

            println!("✨ Xavier Mesh Pairing Code generated (valid for 1 hour):");
            println!("\n  {}\n", code);
            println!("Verification Secret (share separately): {}", secret);
            println!("\nInstructions:");
            println!("  On the other node, run: xavier mesh join <CODE>");
        }
        MeshCommand::Join { code } => {
            let data = xavier::mesh::pairing::decode_pairing_code(&code)?;
            println!(
                "🔗 Joining Xavier Mesh node: {} at {}",
                data.node_id, data.endpoint
            );

            let mut registry = PeerRegistry::load()?;
            let peer = PeerInfo {
                node_id: data.node_id.clone(),
                alias: None,
                endpoint_url: data.endpoint.clone(),
                public_key_hex: data.public_key_hex.clone(),
                added_at: chrono::Utc::now().timestamp(),
                last_seen_at: None,
                sync_enabled: true,
                is_cloud: false,
                iroh_addr: None,
                shared_workspace_ids: Vec::new(),
                shared_workspace_tokens: std::collections::HashMap::new(),
            };

            registry.add_peer(peer)?;
            println!("✅ Node {} added as a trusted peer.", data.node_id);

            // Optional: immediately perform a handshake to verify
            let identity = Arc::new(NodeIdentity::load_or_create()?);
            let transport = MeshTransport::new(identity);
            let token = resolve_http_token().unwrap_or_default();

            println!("Verifying connection with pairing secret...");
            match transport
                .handshake_with_secret(&data.endpoint, &token, Some(data.secret))
                .await
            {
                Ok(_) => println!("✅ Connection verified and node registered!"),
                Err(e) => println!("⚠️ Could not verify connection immediately: {}", e),
            }
        }
        MeshCommand::Status => {
            let identity = NodeIdentity::load_or_create()?;
            let registry = PeerRegistry::load()?;
            println!("Xavier Mesh Status:");
            println!("  Local NodeID:   {}", identity.node_id);
            println!("  Trusted Peers:  {}", registry.list_peers().len());
        }
    }
    Ok(())
}
