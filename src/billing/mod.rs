//! Billing module for Stripe integration.
//!
//! Provides cloud tier billing for Xavier with the following pricing:
//! - Free: $0/mo - Local only
//! - Cloud: Free - 1GB storage, 3 nodes

pub mod plans;
pub mod stripe_client;
pub mod webhook;

use axum::{extract::Extension, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::workspace::WorkspaceContext;
use plans::{Plan, PlanLimits, PlanStatus, SubscriptionStatus};
use stripe_client::BillingService;

/// Billing configuration from environment
#[derive(Debug, Clone)]
pub struct BillingConfig {
    pub enabled: bool,
    pub success_url: String,
    pub cancel_url: String,
}

impl BillingConfig {
    /// From env.
    pub fn from_env() -> Self {
        Self {
            enabled: stripe_client::StripeClient::is_configured(),
            success_url: std::env::var("STRIPE_SUCCESS_URL")
                .unwrap_or_else(|_| "https://xavier.example.com/billing/success".to_string()),
            cancel_url: std::env::var("STRIPE_CANCEL_URL")
                .unwrap_or_else(|_| "https://xavier.example.com/billing/cancel".to_string()),
        }
    }
}

// ============================================================================
// HTTP Request/Response types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateCustomerRequest {
    pub email: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct CreateCustomerResponse {
    pub status: String,
    pub customer_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateCheckoutRequest {
    pub plan: String,
}

#[derive(Debug, Serialize)]
pub struct CreateCheckoutResponse {
    pub status: String,
    pub checkout_url: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePortalRequest {
    pub return_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreatePortalResponse {
    pub status: String,
    pub portal_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CancelSubscriptionResponse {
    pub status: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct BillingStatusResponse {
    pub status: String,
    #[serde(flatten)]
    pub subscription: SubscriptionStatus,
}

#[derive(Debug, Serialize)]
pub struct BillingPlansResponse {
    pub status: String,
    pub plans: Vec<PlanInfo>,
}

#[derive(Debug, Serialize)]
pub struct PlanInfo {
    pub name: String,
    pub display_name: String,
    /// Precio mensual publicado en centavos; `None` = a medida (Empresa).
    pub monthly_price_cents: Option<u32>,
    /// Estado comercial del plan en el catálogo.
    pub status: PlanStatus,
    /// `true` solo si el plan se puede contratar por autoservicio hoy.
    pub is_purchasable: bool,
    pub limits: PlanLimits,
}

// ============================================================================
// HTTP Handlers
// ============================================================================

/// Create a new Stripe customer for the workspace
pub async fn create_customer(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<CreateCustomerRequest>,
) -> impl IntoResponse {
    if !BillingService::is_available() {
        return Json(CreateCustomerResponse {
            status: "error".to_string(),
            customer_id: String::new(),
        });
    }

    match BillingService::new() {
        Ok(billing) => match billing
            .create_customer(&workspace.workspace_id, &payload.email, &payload.name)
            .await
        {
            Ok(customer_id) => Json(CreateCustomerResponse {
                status: "ok".to_string(),
                customer_id,
            }),
            Err(e) => Json(CreateCustomerResponse {
                status: format!("error: {}", e),
                customer_id: String::new(),
            }),
        },
        Err(e) => Json(CreateCustomerResponse {
            status: format!("error: {}", e),
            customer_id: String::new(),
        }),
    }
}

/// Valida el plan pedido en el camino de checkout (catálogo v1).
///
/// `Ok(plan)` solo si el plan existe y se vende por autoservicio hoy; en
/// cualquier otro caso devuelve la respuesta de error que el handler publica,
/// siempre con `checkout_url: None` (no se inicia ningún cobro).
fn resolve_checkout_plan(requested: &str) -> Result<Plan, CreateCheckoutResponse> {
    let plan = match Plan::from_catalog_id(requested) {
        Some(plan) if plan.monthly_price_cents().is_some_and(|cents| cents > 0) => plan,
        _ => {
            return Err(CreateCheckoutResponse {
                status: "error".to_string(),
                checkout_url: None,
                message: Some(
                    "Invalid plan. Use: fundador, respaldo (alias cloud), pro, equipo".to_string(),
                ),
            });
        }
    };

    // Catálogo v1: ningún plan de pago se vende todavía (cobro con Polar pendiente).
    if !plan.is_purchasable() {
        return Err(CreateCheckoutResponse {
            status: "error".to_string(),
            checkout_url: None,
            message: Some(format!(
                "Plan {} not on sale yet (waitlist / coming soon)",
                plan
            )),
        });
    }

    Ok(plan)
}

/// Create a checkout session for subscription upgrade
pub async fn create_checkout(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<CreateCheckoutRequest>,
) -> impl IntoResponse {
    if !BillingService::is_available() {
        return Json(CreateCheckoutResponse {
            status: "error".to_string(),
            checkout_url: None,
            message: Some("Billing not configured".to_string()),
        });
    }

    let plan = match resolve_checkout_plan(&payload.plan) {
        Ok(plan) => plan,
        Err(response) => return Json(response),
    };

    let config = BillingConfig::from_env();

    match BillingService::new() {
        Ok(billing) => {
            match billing
                .create_checkout(
                    &workspace.workspace_id,
                    plan,
                    &config.success_url,
                    &config.cancel_url,
                )
                .await
            {
                Ok(url) => Json(CreateCheckoutResponse {
                    status: "ok".to_string(),
                    checkout_url: Some(url),
                    message: None,
                }),
                Err(e) => Json(CreateCheckoutResponse {
                    status: format!("error: {}", e),
                    checkout_url: None,
                    message: None,
                }),
            }
        }
        Err(e) => Json(CreateCheckoutResponse {
            status: format!("error: {}", e),
            checkout_url: None,
            message: None,
        }),
    }
}

/// Create a customer portal session
pub async fn create_portal(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<CreatePortalRequest>,
) -> impl IntoResponse {
    if !BillingService::is_available() {
        return Json(CreatePortalResponse {
            status: "error".to_string(),
            portal_url: None,
        });
    }

    let config = BillingConfig::from_env();
    let return_url = payload.return_url.unwrap_or(config.cancel_url);

    match BillingService::new() {
        Ok(billing) => {
            match billing
                .create_portal(&workspace.workspace_id, &return_url)
                .await
            {
                Ok(url) => Json(CreatePortalResponse {
                    status: "ok".to_string(),
                    portal_url: Some(url),
                }),
                Err(e) => Json(CreatePortalResponse {
                    status: format!("error: {}", e),
                    portal_url: None,
                }),
            }
        }
        Err(e) => Json(CreatePortalResponse {
            status: format!("error: {}", e),
            portal_url: None,
        }),
    }
}

/// Cancel subscription
pub async fn cancel_subscription(
    Extension(workspace): Extension<WorkspaceContext>,
) -> impl IntoResponse {
    if !BillingService::is_available() {
        return Json(CancelSubscriptionResponse {
            status: "error".to_string(),
            message: "Billing not configured".to_string(),
        });
    }

    match BillingService::new() {
        Ok(billing) => match billing.cancel_subscription(&workspace.workspace_id).await {
            Ok(()) => Json(CancelSubscriptionResponse {
                status: "ok".to_string(),
                message: "Subscription cancelled".to_string(),
            }),
            Err(e) => Json(CancelSubscriptionResponse {
                status: format!("error: {}", e),
                message: e.to_string(),
            }),
        },
        Err(e) => Json(CancelSubscriptionResponse {
            status: format!("error: {}", e),
            message: e.to_string(),
        }),
    }
}

/// Get current billing status
pub async fn billing_status(
    Extension(workspace): Extension<WorkspaceContext>,
) -> impl IntoResponse {
    // If billing is not configured, return free tier status
    if !BillingService::is_available() {
        let status = SubscriptionStatus {
            plan: Plan::Free,
            limits: PlanLimits::for_plan(Plan::Free),
            ..Default::default()
        };
        return Json(BillingStatusResponse {
            status: "ok".to_string(),
            subscription: status,
        });
    }

    match BillingService::new() {
        Ok(billing) => {
            match billing.get_status(&workspace.workspace_id).await {
                Ok(subscription) => Json(BillingStatusResponse {
                    status: "ok".to_string(),
                    subscription,
                }),
                Err(e) => {
                    // Return free tier on error
                    let status = SubscriptionStatus {
                        plan: Plan::Free,
                        limits: PlanLimits::for_plan(Plan::Free),
                        ..Default::default()
                    };
                    Json(BillingStatusResponse {
                        status: format!("error: {}", e),
                        subscription: status,
                    })
                }
            }
        }
        Err(_) => {
            let status = SubscriptionStatus {
                plan: Plan::Free,
                limits: PlanLimits::for_plan(Plan::Free),
                ..Default::default()
            };
            Json(BillingStatusResponse {
                status: "error".to_string(),
                subscription: status,
            })
        }
    }
}

/// Catálogo de planes publicado por `billing_plans()`.
fn plan_catalog() -> Vec<PlanInfo> {
    Plan::ALL
        .iter()
        .map(|plan| PlanInfo {
            name: plan.catalog_id().to_string(),
            display_name: plan.display_name().to_string(),
            monthly_price_cents: plan.monthly_price_cents(),
            status: plan.status(),
            is_purchasable: plan.is_purchasable(),
            limits: PlanLimits::for_plan(*plan),
        })
        .collect()
}

/// Get available billing plans (catálogo canónico v1).
pub async fn billing_plans() -> impl IntoResponse {
    Json(BillingPlansResponse {
        status: "ok".to_string(),
        plans: plan_catalog(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_paid_plan_returns_checkout_url_none() {
        // Planes de pago del catálogo, con el alias `cloud` de Respaldo y el
        // plan a medida (Empresa): ninguno puede iniciar un checkout todavía.
        for requested in ["fundador", "respaldo", "cloud", "pro", "equipo", "empresa"] {
            let response = resolve_checkout_plan(requested)
                .expect_err("un plan no contratable no puede iniciar checkout");
            assert_eq!(response.status, "error", "{requested}");
            assert!(
                response.checkout_url.is_none(),
                "{requested} no debe devolver checkout_url"
            );
        }
    }

    #[test]
    fn paid_plans_are_blocked_by_the_purchasable_gate() {
        for requested in ["fundador", "respaldo", "cloud", "pro", "equipo"] {
            let message = resolve_checkout_plan(requested)
                .expect_err("plan de pago sin venta activa")
                .message
                .unwrap_or_default();
            assert!(
                message.contains("not on sale yet"),
                "{requested}: mensaje inesperado: {message}"
            );
        }
    }

    #[test]
    fn billing_plans_publishes_status_and_purchasable_flag() {
        let plans = plan_catalog();

        assert_eq!(plans.len(), Plan::ALL.len());
        for info in &plans {
            assert!(!info.is_purchasable, "{} no debe venderse", info.name);
        }
        // Empresa es a medida: sin precio publicado, nunca 0.
        let empresa = plans.iter().find(|p| p.name == "empresa").unwrap();
        assert_eq!(empresa.monthly_price_cents, None);
        assert_eq!(empresa.status, PlanStatus::Contact);
        let local = plans.iter().find(|p| p.name == "local").unwrap();
        assert_eq!(local.monthly_price_cents, Some(0));
        assert_eq!(local.status, PlanStatus::Available);
    }
}
