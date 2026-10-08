//! `xavier billing` handlers.
//!
//! * `plans`    → swal-billing `GET /v1/xavier/plans` (public catalog).
//! * `checkout` → swal-billing `POST /v1/xavier/checkout` (409 = waitlist / not on sale).
//! * `status`   → xavier-cloud `GET /v1/cloud/usage` (tenant plan, storage, AI quota).
//!
//! Everything printed comes from those servers; there is no offline fallback
//! with made-up values. Configuration: `XAVIER_BILLING_URL`,
//! `PGHEART_URL`/`XAVIER_PGHEART_URL`, `PGHEART_TOKEN`/`XAVIER_PGHEART_TOKEN`.

use anyhow::{anyhow, Result};

use crate::billing::client::{
    BillingClient, BillingEndpoints, CatalogPlanView, CheckoutOutcome, CheckoutRequest, CloudUsage,
    PlansResponse,
};
use crate::billing::plans::{Plan, CATALOG_VERSION};
use crate::cli::commands::enums::{BillingCommand, CLI_HTTP_CLIENT};

/// Dispatch `xavier billing [subcommand]`. Without a subcommand it runs `status`.
pub async fn handle_billing_command(cmd: Option<BillingCommand>) -> Result<()> {
    let client = BillingClient::new(CLI_HTTP_CLIENT.clone(), BillingEndpoints::from_env());
    match cmd.unwrap_or(BillingCommand::Status { json: false }) {
        BillingCommand::Plans { json } => plans(&client, json).await,
        BillingCommand::Checkout {
            plan,
            email,
            tenant,
            json,
        } => checkout(&client, plan, email, tenant, json).await,
        BillingCommand::Status { json } => status(&client, json).await,
    }
}

async fn plans(client: &BillingClient, json: bool) -> Result<()> {
    let catalog = client.plans().await.map_err(|e| anyhow!(e))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&catalog)?);
    } else {
        print!(
            "{}",
            render_plans(&catalog, &client.endpoints().billing_url)
        );
    }
    Ok(())
}

async fn checkout(
    client: &BillingClient,
    plan: String,
    email: Option<String>,
    tenant: Option<String>,
    json: bool,
) -> Result<()> {
    // The tenant is whatever xavier-cloud says our API key belongs to, unless
    // the user passed one explicitly.
    let tenant = match tenant {
        Some(t) => Some(t),
        None if client.endpoints().cloud_token.is_some() => match client.cloud_usage().await {
            Ok(u) => Some(u.tenant),
            Err(e) => {
                eprintln!("note: checkout sent without tenant ({e})");
                None
            }
        },
        None => None,
    };
    let req = CheckoutRequest {
        plan,
        tenant,
        email,
    };
    let outcome = client.checkout(&req).await.map_err(|e| anyhow!(e))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome)?);
        return match outcome {
            CheckoutOutcome::NotConfigured { message } => Err(anyhow!(message)),
            _ => Ok(()),
        };
    }
    match outcome {
        CheckoutOutcome::NotConfigured { message } => Err(anyhow!(
            "plan '{}' is on sale but swal-billing cannot create the payment yet: {message}",
            req.plan
        )),
        other => {
            println!("{}", render_checkout(&other));
            Ok(())
        }
    }
}

async fn status(client: &BillingClient, json: bool) -> Result<()> {
    let usage = client.cloud_usage().await.map_err(|e| anyhow!(e))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&usage)?);
    } else {
        print!("{}", render_status(&usage, &client.endpoints().cloud_url));
    }
    Ok(())
}

/// Human-readable byte size (binary units).
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value.fract() == 0.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn usd(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("${v:.0}")
    } else {
        format!("${v:.2}")
    }
}

fn price_label(p: &CatalogPlanView) -> String {
    match p.price.monthly_usd {
        None => "custom".to_string(),
        Some(0.0) => "free".to_string(),
        Some(m) => {
            let mut s = format!("{}/mo", usd(m));
            if let Some(y) = p.price.yearly_usd {
                s.push_str(&format!(" or {}/yr", usd(y)));
            }
            if let Some(seat) = p.price.extra_seat_monthly_usd {
                s.push_str(&format!(" +{}/seat", usd(seat)));
            }
            s
        }
    }
}

fn opt_num(v: Option<u64>, unlimited: &str) -> String {
    v.map(|n| n.to_string())
        .unwrap_or_else(|| unlimited.to_string())
}

fn status_label(status: &str) -> &str {
    match status {
        "available" => "available",
        "waitlist" => "waitlist",
        "coming_soon" => "coming soon",
        "contact" => "contact us",
        other => other,
    }
}

/// Table for `xavier billing plans`.
pub fn render_plans(c: &PlansResponse, source: &str) -> String {
    let mut out = format!(
        "Xavier plans — catalog {} ({}, payments via {}) from {source}\n",
        c.version, c.currency, c.payment_provider
    );
    if c.version != CATALOG_VERSION {
        out.push_str(&format!(
            "warning: this CLI was built with catalog {CATALOG_VERSION}; the server has {}\n",
            c.version
        ));
    }
    out.push_str(&format!(
        "\n{:<10} {:<10} {:<12} {:<26} {:>9} {:>14} {:>8}\n",
        "ID", "NAME", "STATUS", "PRICE", "STORAGE", "AI TOKENS/DAY", "DEVICES"
    ));
    for p in &c.plans {
        let storage = match p.limits.storage_bytes {
            Some(0) => "local".to_string(),
            Some(b) => human_bytes(b),
            None => "custom".to_string(),
        };
        out.push_str(&format!(
            "{:<10} {:<10} {:<12} {:<26} {:>9} {:>14} {:>8}\n",
            p.id,
            p.name,
            status_label(&p.status),
            price_label(p),
            storage,
            opt_num(p.limits.ai_tokens_per_day, "custom"),
            opt_num(p.limits.devices, "-"),
        ));
    }
    let buyable: Vec<&str> = c
        .plans
        .iter()
        .filter(|p| p.status == "available" && p.price.monthly_usd.unwrap_or(0.0) > 0.0)
        .map(|p| p.id.as_str())
        .collect();
    out.push_str(&format!(
        "\nPaid plans on sale now: {}\n",
        if buyable.is_empty() {
            "none".to_string()
        } else {
            buyable.join(", ")
        }
    ));
    out
}

/// Message for a successful `xavier billing checkout` call.
pub fn render_checkout(o: &CheckoutOutcome) -> String {
    match o {
        CheckoutOutcome::Url { url } => format!("Open this link to complete the payment:\n{url}"),
        CheckoutOutcome::NotAvailable {
            plan,
            status,
            message,
        } => {
            let mut s = match status.as_str() {
                "available" => {
                    format!("Plan '{plan}' is free and needs no checkout. Nothing was charged.")
                }
                other => {
                    let why = match other {
                        "waitlist" => "it is on the waitlist",
                        "coming_soon" => "it is coming soon",
                        "contact" => "it is sold by direct contact only",
                        _ => "it is not available",
                    };
                    format!(
                        "Plan '{plan}' cannot be purchased yet: {why}. No payment was created and nothing was charged."
                    )
                }
            };
            if let Some(m) = message {
                s.push_str(&format!("\n{m}"));
            }
            s
        }
        CheckoutOutcome::NotConfigured { message } => format!("Checkout not configured: {message}"),
    }
}

/// Text for `xavier billing status`.
pub fn render_status(u: &CloudUsage, source: &str) -> String {
    let plan = match &u.plan {
        Some(Some(id)) => match Plan::from_catalog_id(id) {
            Some(p) => format!("{} ({id})", p.display_name()),
            None => id.clone(),
        },
        Some(None) => "none assigned (legacy per-tenant quota)".to_string(),
        None => "not reported by this xavier-cloud (needs per-plan quotas)".to_string(),
    };
    let percent = match u.percent {
        Some(p) => format!(" ({p:.2}%)"),
        None if u.quota > 0 => format!(" ({:.2}%)", u.bytes as f64 * 100.0 / u.quota as f64),
        None => String::new(),
    };
    let mut out = format!("xavier-cloud: {source}\n");
    out.push_str(&format!("Tenant:        {}\n", u.tenant));
    out.push_str(&format!("Plan:          {plan}\n"));
    out.push_str(&format!(
        "Storage:       {} of {}{percent}\n",
        human_bytes(u.bytes),
        human_bytes(u.quota)
    ));
    if let Some(chunks) = u.chunks {
        out.push_str(&format!("Chunks:        {chunks}\n"));
    }
    out.push_str(&format!(
        "AI tokens/day: {}\n",
        u.ai_tokens_per_day
            .map(|n| n.to_string())
            .unwrap_or_else(|| "not reported".to_string())
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> PlansResponse {
        serde_json::from_str(include_str!("../../billing/catalog.v1.json")).unwrap()
    }

    #[test]
    fn human_bytes_units() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1000), "1000 B");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(5 * 1024 * 1024 * 1024), "5 GiB");
    }

    #[test]
    fn plans_table_comes_from_the_catalog() {
        let out = render_plans(&catalog(), "http://127.0.0.1:8787");
        assert!(out.contains("catalog 2026-10-08.1 (USD, payments via polar)"));
        assert!(!out.contains("warning:"));
        assert!(out.contains("fundador"));
        assert!(out.contains("$10/mo or $100/yr"));
        assert!(out.contains("$49/mo +$12/seat"));
        assert!(out.contains("coming soon"));
        assert!(out.contains("Paid plans on sale now: none"));
    }

    #[test]
    fn plans_table_warns_on_catalog_mismatch() {
        let mut c = catalog();
        c.version = "2099-01-01.1".into();
        assert!(render_plans(&c, "x").contains("warning: this CLI was built with catalog"));
    }

    #[test]
    fn checkout_waitlist_message_is_friendly() {
        let s = render_checkout(&CheckoutOutcome::NotAvailable {
            plan: "fundador".into(),
            status: "waitlist".into(),
            message: None,
        });
        assert!(s.contains("Plan 'fundador' cannot be purchased yet: it is on the waitlist"));
        assert!(s.contains("nothing was charged"));

        let free = render_checkout(&CheckoutOutcome::NotAvailable {
            plan: "local".into(),
            status: "available".into(),
            message: None,
        });
        assert_eq!(
            free,
            "Plan 'local' is free and needs no checkout. Nothing was charged."
        );
    }

    #[test]
    fn status_explains_missing_and_null_plan() {
        let mut u = CloudUsage {
            tenant: "groku-test".into(),
            plan: None,
            bytes: 1536,
            chunks: Some(2),
            quota: 1000 * 1024,
            ai_tokens_per_day: None,
            percent: None,
        };
        let s = render_status(&u, "https://xavier-cloud.swal.network");
        assert!(s.contains("Tenant:        groku-test"));
        assert!(s.contains("not reported by this xavier-cloud"));
        assert!(s.contains("AI tokens/day: not reported"));
        assert!(s.contains("(0.15%)"), "{s}");

        u.plan = Some(None);
        assert!(render_status(&u, "x").contains("none assigned"));

        u.plan = Some(Some("respaldo".into()));
        u.ai_tokens_per_day = Some(50_000);
        let s = render_status(&u, "x");
        assert!(s.contains("Plan:          Respaldo (respaldo)"), "{s}");
        assert!(s.contains("AI tokens/day: 50000"));
    }
}
