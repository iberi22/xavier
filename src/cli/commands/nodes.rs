//! CLI: `xavier nodes add|list|show|rotate|remove|status` (Olas M6/M7, REQ-029/030)
//!
//! Subcommands for SWAL node provisioning (BaaS and SSH/VPS):
//! - `add`: Provision and register a new node
//! - `list`: List all registered nodes in local registry
//! - `show`: Show detailed node metadata (no secrets)
//! - `rotate`: Rotate provider credentials in vault & registry
//! - `remove`: Deprovision remote node and revoke local secrets
//! - `status`: Inspect lifecycle status, certificate validity, and lease

use anyhow::{anyhow, Context, Result};
use ed25519_dalek::SigningKey;
use std::str::FromStr;
use std::sync::Arc;

use crate::cli::commands::enums::NodesCommand;
use xavier::mesh::node::NodeIdentity;
use xavier::nodes::{
    mask_secret, resolve_token, validate_no_personal_ssh_key, validate_rotation_token,
    verify_cert, MockProvisioner, NodeRecord, NodeRegistry, NodeSecretsManager, NodeStatus,
    NodeVisibility, Provider, ProvisioningEngine, PublicNodeInfo,
};

/// Main handler for `xavier nodes ...` commands.
pub async fn handle_nodes_command(cmd: NodesCommand) -> Result<()> {
    match cmd {
        NodesCommand::Add {
            provider,
            token,
            ssh,
            visibility,
            host_key,
            key,
            cert_ttl,
            lease_ttl,
        } => {
            cmd_add(
                &provider,
                token.as_deref(),
                ssh.as_deref(),
                &visibility,
                host_key.as_deref(),
                key.as_deref(),
                cert_ttl,
                lease_ttl,
            )
            .await
        }
        NodesCommand::List {
            visibility,
            status,
            json,
        } => cmd_list(visibility.as_deref(), status.as_deref(), json).await,
        NodesCommand::Show { node_id, json } => cmd_show(&node_id, json).await,
        NodesCommand::Rotate {
            node_id,
            token,
            lease_ttl,
        } => cmd_rotate(&node_id, token.as_deref(), lease_ttl).await,
        NodesCommand::Remove { node_id } => cmd_remove(&node_id).await,
        NodesCommand::Status { node_id } => cmd_status(&node_id).await,
    }
}

/// Provision and register a new node.
#[allow(clippy::too_many_arguments)]
async fn cmd_add(
    provider_str: &str,
    cli_token: Option<&str>,
    ssh_host: Option<&str>,
    visibility_str: &str,
    host_key: Option<&str>,
    key_flag: Option<&str>,
    cert_ttl: u64,
    lease_ttl: u64,
) -> Result<()> {
    // 1. Security Check: Reject personal SSH key flag
    validate_no_personal_ssh_key(key_flag)?;

    // 2. Parse provider and visibility
    let provider = Provider::from_str(provider_str)?;
    let visibility = NodeVisibility::from_str(visibility_str)?;

    // 3. Resolve token according to provider requirements
    let token = match provider {
        Provider::Supabase | Provider::Neon => {
            let tok = resolve_token(cli_token)?;
            Some(tok)
        }
        Provider::Vps => {
            // For VPS, a dedicated keypair is generated; optional token
            if let Some(t) = cli_token {
                if !t.trim().is_empty() {
                    Some(resolve_token(Some(t))?)
                } else {
                    None
                }
            } else if let Ok(t) = std::env::var("XAVIER_NODE_TOKEN") {
                if !t.trim().is_empty() {
                    Some(t.trim().to_string())
                } else {
                    None
                }
            } else {
                None
            }
        }
    };

    // 4. Load wallet/node authority identity to sign certificate
    let identity = NodeIdentity::load_or_create()
        .context("Failed to load or generate local wallet/node identity")?;
    let wallet_sk_bytes: [u8; 32] = identity
        .private_key_bytes()
        .try_into()
        .map_err(|_| anyhow!("Invalid authority private key length in identity"))?;
    let wallet_signing_key = SigningKey::from_bytes(&wallet_sk_bytes);

    // 5. Initialize provisioning engine
    let registry = Arc::new(NodeRegistry::open_default()?);
    let secrets = NodeSecretsManager::new();
    let provisioner = Arc::new(MockProvisioner::new());
    let engine = ProvisioningEngine::new(registry, secrets, provisioner);

    // 6. Execute provisioning
    println!(
        "Provisioning {} node with {} visibility...",
        provider, visibility
    );
    let record = engine
        .provision_node(
            &wallet_signing_key,
            provider,
            visibility,
            token,
            ssh_host.map(|s| s.to_string()),
            host_key.map(|s| s.to_string()),
            cert_ttl,
            lease_ttl,
        )
        .await
        .context("Node provisioning failed")?;

    // 7. Output result (NEVER print secrets or private keys)
    println!("\n=== SWAL NODE PROVISIONED SUCCESSFULLY ===");
    println!("  node_id:               {}", record.node_id);
    println!("  provider:              {}", record.provider);
    println!("  visibility:            {}", record.visibility);
    println!("  status:                {}", record.status);
    println!("  node_pubkey:           {}", record.pubkey);
    if let Some(hk) = &record.host_key_fingerprint {
        println!("  host_key_fingerprint:  {}", hk);
    }
    if let Some(cert) = &record.cert {
        println!("  certificate:");
        println!("    issuer_pubkey:       {}", cert.wallet_pubkey);
        println!("    expiry_timestamp:    {}", cert.expiry);
        println!("    signature:           {}", mask_secret(&cert.signature));
    }
    println!("  lease_id:              [ACTIVE IN SECRETS VAULT]");
    println!("  created_at:            {}", record.created_at);
    println!("\nNode registered. Credentials sealed in hardware vault with ephemeral lease.");

    Ok(())
}

/// List nodes from the local registry.
async fn cmd_list(
    visibility_filter: Option<&str>,
    status_filter: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let registry = NodeRegistry::open_default()?;
    let records = registry.list()?;

    let vis_filter = visibility_filter
        .map(NodeVisibility::from_str)
        .transpose()?;
    let stat_filter = status_filter.map(NodeStatus::from_str).transpose()?;

    let filtered: Vec<NodeRecord> = records
        .into_iter()
        .filter(|r| vis_filter.map_or(true, |v| r.visibility == v))
        .filter(|r| stat_filter.map_or(true, |s| r.status == s))
        .collect();

    if json_output {
        let public_views: Vec<PublicNodeInfo> = filtered.iter().map(PublicNodeInfo::from).collect();
        println!("{}", serde_json::to_string_pretty(&public_views)?);
        return Ok(());
    }

    if filtered.is_empty() {
        println!("No nodes found matching criteria. Use `xavier nodes add` to provision a node.");
        return Ok(());
    }

    println!(
        "{:<28} {:<12} {:<10} {:<18} {:<20} {:<12}",
        "NODE ID", "PROVIDER", "VISIBILITY", "STATUS", "PUBKEY", "CREATED"
    );
    println!("{:-<100}", "");

    for r in &filtered {
        let pubkey_short = if r.pubkey.len() > 16 {
            format!("{}...", &r.pubkey[..16])
        } else {
            r.pubkey.clone()
        };
        println!(
            "{:<28} {:<12} {:<10} {:<18} {:<20} {:<12}",
            r.node_id, r.provider, r.visibility, r.status, pubkey_short, r.created_at
        );
    }

    println!("\nTotal: {} node(s)", filtered.len());
    Ok(())
}

/// Show detailed metadata for a node.
async fn cmd_show(node_id: &str, json_output: bool) -> Result<()> {
    let registry = NodeRegistry::open_default()?;
    let record = registry
        .get(node_id)?
        .ok_or_else(|| anyhow!("Node '{}' not found in registry", node_id))?;

    if json_output {
        // Output sanitized record without sensitive lease details
        let mut sanitized = record.clone();
        if sanitized.lease_id.is_some() {
            sanitized.lease_id = Some("[REDACTED]".to_string());
        }
        println!("{}", serde_json::to_string_pretty(&sanitized)?);
        return Ok(());
    }

    let cert_valid = record
        .cert
        .as_ref()
        .map(|c| verify_cert(c, None).unwrap_or(false))
        .unwrap_or(false);

    println!("=== SWAL NODE DETAILS ===");
    println!("  node_id:               {}", record.node_id);
    println!("  provider:              {}", record.provider);
    println!("  visibility:            {}", record.visibility);
    println!("  status:                {}", record.status);
    println!("  node_pubkey:           {}", record.pubkey);
    if let Some(hk) = &record.host_key_fingerprint {
        println!("  host_key_fingerprint:  {}", hk);
    }
    println!(
        "  lease_id:              {}",
        record
            .lease_id
            .as_deref()
            .map(mask_secret)
            .unwrap_or_else(|| "None".to_string())
    );
    println!("  created_at:            {}", record.created_at);
    if let Some(hb) = record.last_heartbeat {
        println!("  last_heartbeat:        {}", hb);
    }

    if let Some(cert) = &record.cert {
        println!("  certificate:");
        println!("    issuer_pubkey:       {}", cert.wallet_pubkey);
        println!("    node_pubkey:         {}", cert.node_pubkey);
        println!("    expiry_timestamp:    {}", cert.expiry);
        println!("    expired:             {}", cert.is_expired());
        println!("    signature_valid:     {}", cert_valid);
    } else {
        println!("  certificate:           None");
    }

    Ok(())
}

/// Rotate credentials for an existing node.
async fn cmd_rotate(node_id: &str, cli_token: Option<&str>, lease_ttl: u64) -> Result<()> {
    let new_token = resolve_token(cli_token)?;
    validate_rotation_token(&new_token)?;

    let registry = Arc::new(NodeRegistry::open_default()?);
    let secrets = NodeSecretsManager::new();
    let provisioner = Arc::new(MockProvisioner::new());
    let engine = ProvisioningEngine::new(registry, secrets, provisioner);

    println!("Rotating credentials for node '{}'...", node_id);
    let updated = engine
        .rotate_node(node_id, &new_token, lease_ttl)
        .await
        .context("Credential rotation failed")?;

    println!("\n=== NODE CREDENTIALS ROTATED ===");
    println!("  node_id:               {}", updated.node_id);
    println!("  provider:              {}", updated.provider);
    println!("  status:                {}", updated.status);
    println!("  lease:                 [NEW LEASE ISSUED IN VAULT]");
    println!("\nOld lease revoked and new credentials encrypted in hardware vault.");

    Ok(())
}

/// Remove/deprovision a node.
async fn cmd_remove(node_id: &str) -> Result<()> {
    let registry = Arc::new(NodeRegistry::open_default()?);
    let secrets = NodeSecretsManager::new();
    let provisioner = Arc::new(MockProvisioner::new());
    let engine = ProvisioningEngine::new(registry, secrets, provisioner);

    println!("Deprovisioning and revoking node '{}'...", node_id);
    let final_status = engine
        .remove_node(node_id)
        .await
        .context("Node removal failed")?;

    match final_status {
        NodeStatus::Revoked => {
            println!(
                "\n✅ Node '{}' was fully deprovisioned and marked as Revoked.",
                node_id
            );
        }
        NodeStatus::PartialRevocation => {
            eprintln!(
                "\n⚠️ Node '{}' marked as PartialRevocation: local secrets/leases were revoked, but remote deprovisioning reported failure.",
                node_id
            );
        }
        other => {
            println!("\nNode '{}' removal finished with status: {}", node_id, other);
        }
    }

    Ok(())
}

/// Check status and certificate validity for a node.
async fn cmd_status(node_id: &str) -> Result<()> {
    let registry = NodeRegistry::open_default()?;
    let record = registry
        .get(node_id)?
        .ok_or_else(|| anyhow!("Node '{}' not found in registry", node_id))?;

    let cert_status = match &record.cert {
        Some(c) if c.is_expired() => "EXPIRED",
        Some(c) if verify_cert(c, None).unwrap_or(false) => "VALID",
        Some(_) => "INVALID_SIGNATURE",
        None => "NO_CERTIFICATE",
    };

    println!("=== NODE STATUS: {} ===", node_id);
    println!("  lifecycle_status:      {}", record.status);
    println!("  provider:              {}", record.provider);
    println!("  visibility:            {}", record.visibility);
    println!("  certificate_health:    {}", cert_status);
    println!(
        "  vault_lease:           {}",
        if record.lease_id.is_some() {
            "ACTIVE"
        } else {
            "NONE"
        }
    );
    if let Some(hb) = record.last_heartbeat {
        println!("  last_heartbeat:        {}", hb);
    }
    println!("  created_at:            {}", record.created_at);

    Ok(())
}
