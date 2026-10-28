//! Agent connection information and scoped REST credentials.

use super::enums::{OnboardClient, TokenScope};
use crate::cli::config::{resolve_base_url, resolve_mcp_port};
use anyhow::Result;
use chrono::Utc;
use reqwest::{Client, Response, Url};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use xavier::security::tokens::ApiTokenMetadata;

struct Failure {
    code: i32,
    message: &'static str,
}
type AccessResult<T> = std::result::Result<T, Failure>;

fn failure(code: i32, message: &'static str) -> Failure {
    Failure { code, message }
}

fn finish(result: AccessResult<()>, as_json: bool) -> Result<()> {
    if let Err(error) = result {
        if as_json {
            println!(
                "{}",
                json!({"schema_version": 1, "error": {
                    "code": error.code, "message": error.message
                }})
            );
        }
        eprintln!("{}", error.message);
        std::process::exit(error.code);
    }
    Ok(())
}

fn base_url() -> AccessResult<String> {
    let base = resolve_base_url();
    let parsed =
        Url::parse(&base).map_err(|_| failure(2, "Configure a valid HTTP(S) Xavier base URL"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(failure(
            2,
            "Use an HTTP(S) base URL without embedded credentials, query or fragment",
        ));
    }
    Ok(base.trim_end_matches('/').to_owned())
}

fn client() -> AccessResult<Client> {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| failure(1, "Could not initialize the HTTP client"))
}

async fn response(request: reqwest::RequestBuilder) -> AccessResult<Response> {
    let response = request.send().await.map_err(|_| {
        failure(
            4,
            "Xavier server unreachable; check the configured URL and start the server",
        )
    })?;
    match response.status().as_u16() {
        200..=299 => Ok(response),
        401 | 403 => Err(failure(
            3,
            "Xavier authentication failed; supply an authorized XAVIER_TOKEN",
        )),
        _ => Err(failure(
            1,
            "Xavier request failed; check server health and permissions",
        )),
    }
}

fn token() -> AccessResult<String> {
    let value = xavier::security::auth::resolve_xavier_token();
    if value.trim().is_empty() {
        Err(failure(
            3,
            "Configure XAVIER_TOKEN in the environment or Xavier vault",
        ))
    } else {
        Ok(value)
    }
}

fn snippet(client: OnboardClient, url: Option<&str>) -> String {
    let Some(url) = url else { return String::new() };
    match client {
        OnboardClient::Codex => format!(
            "[mcp_servers.xavier]\nurl = {}\nbearer_token_env_var = \"XAVIER_TOKEN\"\n",
            json!(url)
        ),
        OnboardClient::Opencode => json!({"mcp": {"xavier": {
            "type": "remote", "url": url, "enabled": true, "oauth": false,
            "headers": {"Authorization": "Bearer {env:XAVIER_TOKEN}"}
        }}})
        .to_string(),
        OnboardClient::Claude | OnboardClient::Generic => json!({"mcpServers": {"xavier": {
            "type": "http", "url": url,
            "headers": {"Authorization": "Bearer ${XAVIER_TOKEN}"}
        }}})
        .to_string(),
    }
}

pub async fn onboard(
    client_kind: OnboardClient,
    as_json: bool,
    reveal: bool,
    confirmed: bool,
) -> Result<()> {
    finish(
        onboard_inner(client_kind, as_json, reveal, confirmed).await,
        as_json,
    )
}

async fn onboard_inner(
    client_kind: OnboardClient,
    as_json: bool,
    reveal: bool,
    confirmed: bool,
) -> AccessResult<()> {
    if reveal && (!confirmed || std::io::stdout().is_terminal()) {
        return Err(failure(
            2,
            "--reveal requires non-terminal stdout and --i-understand-output-is-not-a-tty",
        ));
    }
    let base = base_url()?;
    let port = resolve_mcp_port(None);
    let mcp_url = if port == 0 {
        None
    } else {
        let mut url = Url::parse(&base).map_err(|_| failure(2, "Invalid Xavier base URL"))?;
        url.set_port(Some(port))
            .map_err(|_| failure(2, "Invalid MCP endpoint port"))?;
        url.set_path("/mcp");
        Some(url.to_string())
    };
    let configured_token = xavier::security::auth::resolve_xavier_token();
    let token_source = if std::env::var("XAVIER_TOKEN").is_ok_and(|v| !v.trim().is_empty()) {
        json!({"kind": "env", "reference": "XAVIER_TOKEN"})
    } else if !configured_token.is_empty() {
        json!({"kind": "vault", "reference": "vault:xavier/XAVIER_TOKEN"})
    } else {
        json!({"kind": "unset", "reference": null})
    };
    if reveal && configured_token.is_empty() {
        return Err(failure(
            3,
            "Configure XAVIER_TOKEN before exporting a credential",
        ));
    }
    let health_response = response(
        client()?
            .get(format!("{base}/health"))
            .bearer_auth(&configured_token),
    )
    .await?;
    let http_status = health_response.status().as_u16();
    let health: Value = health_response
        .json()
        .await
        .map_err(|_| failure(1, "Invalid Xavier health response"))?;
    let status = health["status"]
        .as_str()
        .filter(|value| {
            matches!(
                *value,
                "ok" | "healthy" | "degraded" | "unhealthy" | "starting"
            )
        })
        .unwrap_or("unknown");
    let client_name = match client_kind {
        OnboardClient::Claude => "claude",
        OnboardClient::Codex => "codex",
        OnboardClient::Opencode => "opencode",
        OnboardClient::Generic => "generic",
    };
    let config = snippet(client_kind, mcp_url.as_deref());
    let mut report = json!({
        "schema_version": 1, "base_url": base, "mcp_url": mcp_url,
        "health": {"status": status, "http_status": http_status},
        "token_source": token_source, "client": client_name, "mcp_config": config,
    });
    if reveal {
        report["token"] = json!(configured_token);
    }
    if as_json {
        println!("{report}");
    } else {
        println!(
            "Base URL: {base}\nMCP URL: {}\nHealth: {status}\nToken source: {}\n\n{config}",
            mcp_url.as_deref().unwrap_or("disabled"),
            token_source["reference"].as_str().unwrap_or("unset")
        );
        if reveal {
            println!("{configured_token}");
        }
    }
    Ok(())
}

fn expiry(ttl: &str) -> AccessResult<chrono::DateTime<Utc>> {
    let invalid = || {
        failure(
            2,
            "--ttl must be a positive integer with optional s, m, h, d or w suffix",
        )
    };
    let (number, multiplier) = match ttl.as_bytes().last() {
        Some(b's') => (&ttl[..ttl.len() - 1], 1),
        Some(b'm') => (&ttl[..ttl.len() - 1], 60),
        Some(b'h') => (&ttl[..ttl.len() - 1], 3600),
        Some(b'd') => (&ttl[..ttl.len() - 1], 86400),
        Some(b'w') => (&ttl[..ttl.len() - 1], 604800),
        _ => (ttl, 1),
    };
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    let seconds = number
        .parse::<i64>()
        .ok()
        .and_then(|n| n.checked_mul(multiplier))
        .filter(|&n| n > 0)
        .ok_or_else(invalid)?;
    let duration = chrono::Duration::try_seconds(seconds).ok_or_else(invalid)?;
    Utc::now().checked_add_signed(duration).ok_or_else(invalid)
}

struct SecretFile {
    file: File,
    path: PathBuf,
    complete: bool,
}

impl SecretFile {
    fn create(path: &Path) -> AccessResult<Self> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(|_| {
            failure(
                1,
                "Could not create --out-file; choose a new path in an existing writable directory",
            )
        })?;
        Ok(Self {
            file,
            path: path.to_owned(),
            complete: false,
        })
    }

    fn save(&mut self, value: &str) -> AccessResult<()> {
        writeln!(self.file, "{value}")
            .and_then(|_| self.file.sync_all())
            .map_err(|_| {
                failure(
                    1,
                    "Could not save issued token; revoke it through token administration",
                )
            })?;
        self.complete = true;
        Ok(())
    }
}

impl Drop for SecretFile {
    fn drop(&mut self) {
        if !self.complete {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[derive(Deserialize)]
struct IssuedToken {
    token: String,
    metadata: ApiTokenMetadata,
}

fn metadata(meta: &ApiTokenMetadata) -> Value {
    json!({"id": meta.id, "agent": meta.name, "scope": meta.scopes.first(),
        "scopes": meta.scopes, "expires_at": meta.expires_at})
}

pub async fn issue(
    agent: &str,
    scope: TokenScope,
    ttl: &str,
    as_json: bool,
    out_file: Option<&Path>,
    confirmed: bool,
) -> Result<()> {
    finish(
        issue_inner(agent, scope, ttl, as_json, out_file, confirmed).await,
        as_json,
    )
}

async fn issue_inner(
    agent: &str,
    scope: TokenScope,
    ttl: &str,
    as_json: bool,
    out_file: Option<&Path>,
    confirmed: bool,
) -> AccessResult<()> {
    if out_file.is_none() && (as_json || !confirmed || std::io::stdout().is_terminal()) {
        return Err(failure(2, "Use --out-file, or non-terminal stdout with --i-understand-output-is-not-a-tty; --json requires --out-file"));
    }
    xavier::secrets::keygen::validate_name(agent).map_err(|_| {
        failure(
            2,
            "--agent requires 1-64 letters, digits, underscores or hyphens",
        )
    })?;
    let expires_at = expiry(ttl)?;
    let base = base_url()?;
    let credential = token()?;
    let mut output = out_file.map(SecretFile::create).transpose()?;
    let scope = match scope {
        TokenScope::Read => "read",
        TokenScope::Write => "write",
    };
    let issued: IssuedToken = response(
        client()?
            .post(format!("{base}/security/tokens"))
            .bearer_auth(credential)
            .json(&json!({"name": agent, "scopes": [scope], "expires_at": expires_at})),
    )
    .await?
    .json()
    .await
    .map_err(|_| failure(1, "Invalid token issuance response"))?;
    if issued.token.is_empty() {
        return Err(failure(1, "Server returned an empty token"));
    }
    let report = json!({"schema_version": 1, "status": "ok",
        "metadata": metadata(&issued.metadata), "out_file": out_file});
    if let Some(output) = &mut output {
        output.save(&issued.token)?;
        println!("{report}");
    } else {
        println!("{}", issued.token);
        eprintln!("{report}");
    }
    Ok(())
}

pub async fn list_keys(as_json: bool) -> Result<()> {
    finish(list_keys_inner(as_json).await, as_json)
}

async fn list_keys_inner(as_json: bool) -> AccessResult<()> {
    #[derive(Deserialize)]
    struct TokenList {
        tokens: Vec<ApiTokenMetadata>,
    }
    let base = base_url()?;
    let tokens: TokenList = response(
        client()?
            .get(format!("{base}/security/tokens"))
            .bearer_auth(token()?),
    )
    .await?
    .json()
    .await
    .map_err(|_| failure(1, "Invalid token listing response"))?;
    let keys: Vec<Value> = tokens.tokens.iter().map(metadata).collect();
    if as_json {
        println!("{}", json!({"schema_version": 1, "keys": keys}));
    } else {
        for key in keys {
            println!("{key}");
        }
    }
    Ok(())
}
