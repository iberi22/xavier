//! CLI: `xavier node create|recover|status|anchor|anchor-pack`

use crate::cli::commands::enums::NodeCommand;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use xavier::crypto::hex_encode;
use xavier::node_identity::{
    CheckCodes, NodeBootstrap, NodeStore, NodeStorePaths, OrderMode, OrderedChallenge,
    PublicNodeIdentity, ShamirShare, ShamirSplit,
};
use xavier::polygon_anchor::{
    anchor_node_identity, anchor_sealed_pack, AnchorRegistry, EnvAnchorTransport,
};
use xavier::settings::XavierSettings;

const BRICK_WARNING: &str = "\
CRITICAL — BRICK RISK: Losing ≥2 of 3 Shamir shares (or the BIP39 mnemonic) permanently \
bricks this node. There is NO central recovery, NO Stripe reset, NO support backdoor. \
Store shares offline in separate locations before clearing the screen.";

/// Handle `xavier node …`.
pub async fn handle_node_command(cmd: NodeCommand) -> Result<()> {
    crate::cli::config::validate_xavier_data_dir_env()?;
    match cmd {
        NodeCommand::Create {
            pin,
            passphrase,
            device_key_hex,
            force,
            data_dir,
            shares_out,
        } => cmd_create(pin, passphrase, device_key_hex, force, data_dir, shares_out).await,
        NodeCommand::Recover {
            pin,
            passphrase,
            device_key_hex,
            shares_file,
            challenge_mode,
            response,
            force,
            data_dir,
        } => {
            cmd_recover(
                pin,
                passphrase,
                device_key_hex,
                shares_file,
                Some(challenge_mode),
                Some(response),
                force,
                data_dir,
            )
            .await
        }
        NodeCommand::Status {
            pin,
            device_key_hex,
            data_dir,
            unlock,
        } => cmd_status(pin, device_key_hex, data_dir, unlock).await,
        NodeCommand::Anchor { data_dir, dry_run } => cmd_anchor(data_dir, dry_run).await,
        NodeCommand::AnchorPack {
            ciphertext_hex,
            cipher_file,
            meta,
            dry_run,
            data_dir,
        } => cmd_anchor_pack(ciphertext_hex, cipher_file, meta, dry_run, data_dir).await,
    }
}

fn store_for(data_dir: Option<PathBuf>) -> NodeStore {
    match data_dir {
        Some(p) => NodeStore::new(NodeStorePaths::from_data_dir(p)),
        None => NodeStore::default_from_env(),
    }
}

fn parse_device_key(hex: Option<&str>) -> Result<Option<[u8; 32]>> {
    let Some(h) = hex.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let bytes = xavier::crypto::hex_decode(h).context("device_key_hex decode")?;
    if bytes.len() != 32 {
        bail!("device_key must be 32 bytes (WebAuthn PRF / OS keystore export)");
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(Some(arr))
}

fn resolve_pin(pin: Option<String>, prompt: &str) -> Result<String> {
    if let Some(p) = pin {
        if p.is_empty() {
            bail!("PIN must not be empty");
        }
        return Ok(p);
    }
    let entered: String = dialoguer::Password::new()
        .with_prompt(prompt)
        .interact()
        .context("read PIN")?;
    if entered.is_empty() {
        bail!("PIN must not be empty");
    }
    Ok(entered)
}

fn resolve_pin_for_create(pin: Option<String>) -> Result<String> {
    if pin.is_some() {
        return resolve_pin(pin, "PIN");
    }
    let entered = resolve_pin(None, "Choose a PIN to seal the vault")?;
    let confirm = resolve_pin(None, "Confirm PIN")?;
    if entered != confirm {
        bail!("PIN confirmation mismatch");
    }
    Ok(entered)
}

async fn cmd_create(
    pin: Option<String>,
    passphrase: Option<String>,
    device_key_hex: Option<String>,
    force: bool,
    data_dir: Option<PathBuf>,
    shares_out: Option<PathBuf>,
) -> Result<()> {
    let store = store_for(data_dir.clone());
    if store.vault_exists() && !force {
        bail!(
            "vault already exists at {}. Use --force to overwrite (destroys previous recovery material).",
            store.paths.vault.display()
        );
    }

    let pin = resolve_pin_for_create(pin)?;
    let device_key = parse_device_key(device_key_hex.as_deref())?;

    eprintln!("Generating BIP39-24 node identity (offline, no Stripe, no central login)…");
    let bundle = NodeBootstrap::create(passphrase.as_deref(), &pin, device_key.as_ref())?;
    let pub_id = PublicNodeIdentity::from_keys(&bundle.keys);

    store.save_vault(&bundle.vault)?;
    store.save_public_identity(&pub_id)?;

    println!("=== SWAL NODE CREATE — SAVE THIS OFFLINE (shown once) ===");
    println!("mnemonic (BIP39-24):");
    println!("{}", bundle.mnemonic);
    if bundle.passphrase_used {
        println!("(passphrase was set — remember it; it is NOT printed)");
    }
    if device_key.is_some() {
        println!("(device_key mixed into vault KDF — required on unlock; not printed)");
    }
    println!();
    println!("Shamir shares 2-of-3 (need any 2 to recover):");
    for share in &bundle.shares {
        println!("  share x={} ys_hex={}", share.x, hex_encode(&share.ys));
    }
    println!();
    println!("recovery check-codes (6×3 digits):");
    println!("  {}", bundle.check_codes.display_joined());
    println!();
    println!("=== PUBLIC (safe to share / log) ===");
    println!("  node_id:            {}", pub_id.node_id);
    println!("  ed25519_public:     {}", pub_id.ed25519_public_hex);
    println!("  ml_dsa_commitment:  {}", pub_id.ml_dsa_commitment_hex);
    println!("  vault:              {}", store.paths.vault.display());
    println!(
        "  identity.public:    {}",
        store.paths.public_identity.display()
    );
    let data_dir_display = data_dir.unwrap_or_else(XavierSettings::resolve_data_dir);
    println!("  data_dir:           {}", data_dir_display.display());
    eprintln!();
    eprintln!("{BRICK_WARNING}");

    if let Some(path) = shares_out {
        let export = serde_json::json!({
            "version": 1,
            "warning": "SECRET — store offline; 2-of-3 threshold; brick if lost",
            "shares": bundle.shares.iter().map(|s| serde_json::json!({
                "x": s.x,
                "ys_hex": hex_encode(&s.ys),
            })).collect::<Vec<_>>(),
            "check_codes": bundle.check_codes.triplets,
        });
        write_secret_file(&path, serde_json::to_string_pretty(&export)?.as_bytes())?;
        eprintln!(
            "Shares export written to {} (mode 0600). Delete after backup.",
            path.display()
        );
    }

    Ok(())
}

async fn cmd_recover(
    pin: Option<String>,
    passphrase: Option<String>,
    device_key_hex: Option<String>,
    shares_file: PathBuf,
    challenge_mode: Option<String>,
    response: Option<String>,
    force: bool,
    data_dir: Option<PathBuf>,
) -> Result<()> {
    let store = store_for(data_dir);
    if store.vault_exists() && !force {
        bail!(
            "vault already exists at {}. Use --force to overwrite.",
            store.paths.vault.display()
        );
    }

    let shares = load_shares_file(&shares_file)?;
    if shares.len() < 2 {
        bail!("need at least 2 Shamir shares in {}", shares_file.display());
    }

    let entropy = ShamirSplit::combine(&shares)?;
    let phrase = xavier::node_identity::SeedPhrase::from_entropy(&entropy, passphrase.as_deref())?;
    let codes = CheckCodes::from_seed_bytes(&phrase.seed_bytes);

    let mode = match challenge_mode.as_deref() {
        Some("asc") | Some("ASC") => OrderMode::Asc,
        Some("desc") | Some("DESC") => OrderMode::Desc,
        None => {
            if rand::random::<bool>() {
                OrderMode::Asc
            } else {
                OrderMode::Desc
            }
        }
        Some(other) => bail!("invalid --challenge-mode '{other}' (use asc|desc)"),
    };
    let challenge = OrderedChallenge::new(mode, &codes);

    eprintln!(
        "Recovery challenge: re-enter the 6 check-codes in {:?} order.",
        challenge.mode
    );
    eprintln!(
        "Displayed (shuffled): {}",
        challenge
            .displayed
            .iter()
            .map(|t| format!("{t:03}"))
            .collect::<Vec<_>>()
            .join(" ")
    );

    let response_triplets = if let Some(raw) = response {
        parse_triplets(&raw)?
    } else {
        let line: String = dialoguer::Input::new()
            .with_prompt("Enter 6 triplets (comma or space separated, required order)")
            .interact_text()
            .context("read challenge response")?;
        parse_triplets(&line)?
    };

    if !challenge.verify(&response_triplets, &codes) {
        bail!("ordered check-code challenge failed");
    }

    let pin = resolve_pin(pin, "Choose a new PIN to seal the recovered vault")?;
    let device_key = parse_device_key(device_key_hex.as_deref())?;
    let bundle = NodeBootstrap::recover_from_shares(
        &shares,
        passphrase.as_deref(),
        &response_triplets,
        &challenge,
        &pin,
        device_key.as_ref(),
    )?;
    let pub_id = PublicNodeIdentity::from_keys(&bundle.keys);

    store.save_vault(&bundle.vault)?;
    store.save_public_identity(&pub_id)?;

    println!("=== RECOVERY OK ===");
    println!("  node_id:            {}", pub_id.node_id);
    println!("  ed25519_public:     {}", pub_id.ed25519_public_hex);
    println!("  ml_dsa_commitment:  {}", pub_id.ml_dsa_commitment_hex);
    println!("  vault:              {}", store.paths.vault.display());
    eprintln!("Mnemonic is NOT re-printed. Keep your Shamir shares offline.");
    eprintln!("{BRICK_WARNING}");
    Ok(())
}

async fn cmd_status(
    pin: Option<String>,
    device_key_hex: Option<String>,
    data_dir: Option<PathBuf>,
    unlock: bool,
) -> Result<()> {
    let store = store_for(data_dir);
    println!("Node store:");
    println!("  root:    {}", store.paths.root.display());
    println!(
        "  vault:   {} (exists={})",
        store.paths.vault.display(),
        store.vault_exists()
    );
    println!(
        "  public:  {} (exists={})",
        store.paths.public_identity.display(),
        store.paths.public_identity.exists()
    );

    if store.paths.public_identity.exists() {
        let pub_id = store.load_public_identity()?;
        println!("Public identity:");
        println!("  node_id:           {}", pub_id.node_id);
        println!("  ed25519_public:    {}", pub_id.ed25519_public_hex);
        println!("  ml_dsa_commitment: {}", pub_id.ml_dsa_commitment_hex);
        println!("  created_at:        {}", pub_id.created_at);
    } else {
        println!("No public identity file. Run `xavier node create`.");
    }

    if unlock {
        let pin = resolve_pin(pin, "PIN to unlock vault")?;
        let device_key = parse_device_key(device_key_hex.as_deref())?;
        match store.unlock(&pin, device_key.as_ref()) {
            Ok((_opened, keys, _codes)) => {
                println!("Unlock: OK");
                println!("  derived node_id: {}", keys.node_id);
            }
            Err(e) => bail!("unlock failed: {e}"),
        }
    }
    Ok(())
}

async fn cmd_anchor(data_dir: Option<PathBuf>, dry_run: bool) -> Result<()> {
    let store = store_for(data_dir.clone());
    let pub_id = store
        .load_public_identity()
        .context("load identity.public.json — run `xavier node create` first")?;
    let reg = match &data_dir {
        Some(d) => AnchorRegistry::under_data_dir(d),
        None => AnchorRegistry::default_from_env(),
    };
    let transport = EnvAnchorTransport::from_env().with_dry_run(dry_run);
    let (payload, receipt) = anchor_node_identity(
        &transport,
        &pub_id.node_id,
        &pub_id.ed25519_public_hex,
        &pub_id.ml_dsa_commitment_hex,
        Some(&reg),
    )?;
    println!("=== POLYGON IDENTITY ANCHOR ===");
    println!("  content_hash: {}", payload.content_hash_hex);
    println!("  tx_hash:      {}", receipt.tx_hash);
    println!("  dry_run:      {}", receipt.dry_run);
    println!("  chain_id:     {}", receipt.chain_id);
    if let Some(c) = &receipt.contract {
        println!("  contract:     {c}");
    }
    if let Some(cd) = &receipt.calldata_hex {
        println!("  calldata:     {cd}");
    }
    println!(
        "  receipt:      {}/{}.json",
        reg.root.display(),
        payload.content_hash_hex
    );
    eprintln!("Only metadata hash is anchored. Seed/vault never leave this machine.");
    Ok(())
}

async fn cmd_anchor_pack(
    ciphertext_hex: Option<String>,
    cipher_file: Option<PathBuf>,
    meta: String,
    dry_run: bool,
    data_dir: Option<PathBuf>,
) -> Result<()> {
    let cipher = if let Some(path) = cipher_file {
        std::fs::read(&path).with_context(|| format!("read {}", path.display()))?
    } else if let Some(h) = ciphertext_hex {
        xavier::crypto::hex_decode(&h).context("ciphertext_hex")?
    } else {
        bail!("provide --ciphertext-hex or --cipher-file");
    };
    let reg = match &data_dir {
        Some(d) => AnchorRegistry::under_data_dir(d),
        None => AnchorRegistry::default_from_env(),
    };
    let transport = EnvAnchorTransport::from_env_kind(xavier::polygon_anchor::AnchorKind::Pack)
        .with_dry_run(dry_run);
    let (hash, receipt) = anchor_sealed_pack(&transport, &cipher, &meta, Some(&reg))?;
    println!("=== POLYGON PACK ANCHOR ===");
    println!("  content_hash: {hash}");
    println!("  tx_hash:      {}", receipt.tx_hash);
    println!("  dry_run:      {}", receipt.dry_run);
    eprintln!("Ciphertext stays off-chain; only content_hash is registered.");
    Ok(())
}

fn parse_triplets(raw: &str) -> Result<[u16; 6]> {
    let parts: Vec<&str> = raw
        .split(|c: char| c == ',' || c.is_whitespace() || c == '-')
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() != 6 {
        bail!("expected 6 triplets, got {}", parts.len());
    }
    let mut out = [0u16; 6];
    for (i, p) in parts.iter().enumerate() {
        let v: u16 = p
            .parse()
            .with_context(|| format!("invalid triplet '{p}'"))?;
        if v > 999 {
            bail!("triplet out of range: {v}");
        }
        out[i] = v;
    }
    Ok(out)
}

fn load_shares_file(path: &PathBuf) -> Result<Vec<ShamirShare>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read shares file {}", path.display()))?;
    let v: serde_json::Value = serde_json::from_str(&raw).context("parse shares JSON")?;
    let arr = if let Some(a) = v.get("shares").and_then(|x| x.as_array()) {
        a.clone()
    } else if let Some(a) = v.as_array() {
        a.clone()
    } else {
        bail!("shares file must be an array or {{ \"shares\": [...] }}");
    };
    let mut shares = Vec::new();
    for item in arr {
        let x = item
            .get("x")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("share missing x"))? as u8;
        let ys_hex = item
            .get("ys_hex")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("share missing ys_hex"))?;
        let ys_vec = xavier::crypto::hex_decode(ys_hex).context("ys_hex decode")?;
        if ys_vec.len() != 32 {
            bail!("share ys must be 32 bytes, got {}", ys_vec.len());
        }
        let mut ys = [0u8; 32];
        ys.copy_from_slice(&ys_vec);
        shares.push(ShamirShare { x, ys });
    }
    Ok(shares)
}

fn write_secret_file(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_triplets_ok() {
        assert_eq!(
            parse_triplets("001, 020 300-400 500 600").unwrap(),
            [1, 20, 300, 400, 500, 600]
        );
    }

    #[test]
    fn parse_device_key_len() {
        assert!(parse_device_key(Some(&"aa".repeat(32))).unwrap().is_some());
        assert!(parse_device_key(Some("dead")).is_err());
        assert!(parse_device_key(None).unwrap().is_none());
    }
}
