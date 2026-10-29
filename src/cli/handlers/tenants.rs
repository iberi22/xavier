use crate::cli::commands::enums::{TenantsCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{resolve_base_url, resolve_http_token};
use crate::enterprise::tenant::Plan;
use crate::enterprise::tenant::TenantId;
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TenantResponse {
    pub id: TenantId,
    pub name: String,
    pub plan: Plan,
}
use reqwest::StatusCode;

pub async fn handle_tenants_command(cmd: TenantsCommand) -> anyhow::Result<()> {
    match cmd {
        TenantsCommand::List { json } => {
            let base_url = resolve_base_url();
            let token = resolve_http_token().unwrap_or_default();

            let resp = CLI_HTTP_CLIENT
                .get(format!("{}/v1/tenants", base_url))
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await;

            match resp {
                Ok(r) => {
                    let status = r.status();
                    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                        if json {
                            println!("{}", serde_json::json!({"error": "Unauthorized"}));
                        } else {
                            eprintln!("Error: Unauthorized. Check your XAVIER_TOKEN.");
                        }
                        std::process::exit(3); // auth exit code
                    } else if !status.is_success() {
                        let msg = format!("HTTP {}", status);
                        if json {
                            println!("{}", serde_json::json!({"error": msg}));
                        } else {
                            eprintln!("Error: {}", msg);
                        }
                        std::process::exit(1);
                    }

                    let data: Vec<TenantResponse> = match r.json().await {
                        Ok(d) => d,
                        Err(e) => {
                            let msg = format!("Failed to parse response: {}", e);
                            if json {
                                println!("{}", serde_json::json!({"error": msg}));
                            } else {
                                eprintln!("Error: {}", msg);
                            }
                            std::process::exit(1);
                        }
                    };

                    if json {
                        println!("{}", serde_json::to_string_pretty(&data).unwrap());
                    } else {
                        print_tenants_table(&data);
                    }
                }
                Err(_) => {
                    if json {
                        println!("{}", serde_json::json!({"error": "unreachable"}));
                    } else {
                        eprintln!("Error: Daemon is unreachable.");
                    }
                    std::process::exit(4); // unreachable exit code
                }
            }
        }
        TenantsCommand::Show { id, json } => {
            let base_url = resolve_base_url();
            let token = resolve_http_token().unwrap_or_default();

            let resp = CLI_HTTP_CLIENT
                .get(format!("{}/v1/tenants/{}", base_url, id))
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await;

            match resp {
                Ok(r) => {
                    let status = r.status();
                    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                        if json {
                            println!("{}", serde_json::json!({"error": "Unauthorized"}));
                        } else {
                            eprintln!("Error: Unauthorized. Check your XAVIER_TOKEN.");
                        }
                        std::process::exit(3); // auth exit code
                    } else if status == StatusCode::NOT_FOUND {
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({"error": format!("Tenant not found: id={}", id)})
                            );
                        } else {
                            eprintln!("Error: tenant not found: id={}", id);
                        }
                        std::process::exit(1);
                    } else if !status.is_success() {
                        let msg = format!("HTTP {}", status);
                        if json {
                            println!("{}", serde_json::json!({"error": msg}));
                        } else {
                            eprintln!("Error: {}", msg);
                        }
                        std::process::exit(1);
                    }

                    let data: TenantResponse = match r.json().await {
                        Ok(d) => d,
                        Err(e) => {
                            let msg = format!("Failed to parse response: {}", e);
                            if json {
                                println!("{}", serde_json::json!({"error": msg}));
                            } else {
                                eprintln!("Error: {}", msg);
                            }
                            std::process::exit(1);
                        }
                    };

                    if json {
                        println!("{}", serde_json::to_string_pretty(&data).unwrap());
                    } else {
                        print_tenant_table(&data);
                    }
                }
                Err(_) => {
                    if json {
                        println!("{}", serde_json::json!({"error": "unreachable"}));
                    } else {
                        eprintln!("Error: Daemon is unreachable.");
                    }
                    std::process::exit(4); // unreachable exit code
                }
            }
        }
    }
    Ok(())
}

fn print_tenants_table(tenants: &[TenantResponse]) {
    if tenants.is_empty() {
        println!("No tenants found.");
        return;
    }
    println!("{:<36} | {:<20} | {:<15}", "ID", "Name", "Plan");
    println!("{:-<36}-+-{:-<20}-+-{:-<15}", "", "", "");
    for t in tenants {
        println!(
            "{:<36} | {:<20} | {:<15}",
            t.id.to_string(),
            t.name,
            format!("{:?}", t.plan)
        );
    }
}

fn print_tenant_table(t: &TenantResponse) {
    println!("{:<15}: {}", "ID", t.id.to_string());
    println!("{:<15}: {}", "Name", t.name);
    println!("{:<15}: {:?}", "Plan", t.plan);
}
