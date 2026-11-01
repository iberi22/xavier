use crate::cli::commands::enums::{TenantsCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{resolve_base_url, resolve_http_token};
use crate::enterprise::persistence::EnterpriseDb;
use crate::enterprise::tenant::Plan;
use crate::enterprise::tenant::TenantId;
use reqwest::StatusCode;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TenantResponse {
    pub id: TenantId,
    pub name: String,
    pub plan: Plan,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn handle_tenants_command(cmd: TenantsCommand) -> anyhow::Result<()> {
    match cmd {
        TenantsCommand::List { json, local } => {
            let base_url = resolve_base_url();
            let token = resolve_http_token().unwrap_or_default();

            let mut use_local = local;
            if !use_local {
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
                            std::process::exit(3);
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
                        return Ok(());
                    }
                    Err(_) => {
                        use_local = true; // Fallback
                    }
                }
            }

            if use_local {
                let data_dir = crate::settings::XavierSettings::resolve_data_dir();
                let db_path = data_dir.join("enterprise.db");
                let db = EnterpriseDb::open_or_create(&db_path)?;
                let tenants_map = db.load_all_tenants()?;
                let mut data = Vec::new();
                for t in tenants_map.values() {
                    data.push(TenantResponse {
                        id: t.id,
                        name: t.name.clone(),
                        plan: t.plan,
                        created_at: t.created_at,
                    });
                }
                if json {
                    println!("{}", serde_json::to_string_pretty(&data).unwrap());
                } else {
                    print_tenants_table(&data);
                }
            }
        }
        TenantsCommand::Show { id, json, local } => {
            let base_url = resolve_base_url();
            let token = resolve_http_token().unwrap_or_default();

            let mut use_local = local;
            if !use_local {
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
                            std::process::exit(3);
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
                        return Ok(());
                    }
                    Err(_) => {
                        use_local = true; // Fallback
                    }
                }
            }
            if use_local {
                let data_dir = crate::settings::XavierSettings::resolve_data_dir();
                let db_path = data_dir.join("enterprise.db");
                let db = EnterpriseDb::open_or_create(&db_path)?;
                let tenant_id = id.parse::<TenantId>()?;
                let tenants_map = db.load_all_tenants()?;
                let t = tenants_map
                    .get(&tenant_id)
                    .ok_or_else(|| anyhow::anyhow!("Tenant not found: id={}", id))?;
                let data = TenantResponse {
                    id: t.id,
                    name: t.name.clone(),
                    plan: t.plan,
                    created_at: t.created_at,
                };
                if json {
                    println!("{}", serde_json::to_string_pretty(&data).unwrap());
                } else {
                    print_tenant_table(&data);
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
            t.id,
            t.name,
            format!("{:?}", t.plan)
        );
    }
}

fn print_tenant_table(t: &TenantResponse) {
    println!("{:<15}: {}", "ID", t.id);
    println!("{:<15}: {}", "Name", t.name);
    println!("{:<15}: {:?}", "Plan", t.plan);
}
