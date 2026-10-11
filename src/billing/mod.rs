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

/// Error de catálogo para un plan que no existe: enumera TODO el catálogo,
/// incluidos los planes que no se contratan por checkout (Empresa).
fn invalid_plan_error() -> CreateCheckoutResponse {
    let names: Vec<&str> = Plan::ALL.iter().map(|p| p.catalog_id()).collect();
    CreateCheckoutResponse {
        status: "error".to_string(),
        checkout_url: None,
        message: Some(format!("Invalid plan. Use: {}", names.join(", "))),
    }
}

/// Error de un plan conocido, con precio publicado y sin venta por
/// autoservicio hoy (incluido Local, que es gratuito: no hay nada que cobrar).
fn not_on_sale_error(plan: Plan) -> CreateCheckoutResponse {
    let message = match plan.status() {
        PlanStatus::Available => format!("Plan {plan} is free: no checkout required"),
        _ => format!("Plan {plan} not on sale yet (waitlist / coming soon)"),
    };
    CreateCheckoutResponse {
        status: "error".to_string(),
        checkout_url: None,
        message: Some(message),
    }
}

/// Valida el plan pedido en el camino de checkout (catálogo v1).
///
/// `Ok(plan)` solo si el plan existe, publica precio y se vende por
/// autoservicio hoy; en cualquier otro caso devuelve la respuesta de error que
/// el handler publica, siempre con `checkout_url: None` (no se inicia ningún
/// cobro).
///
/// Existencia, vendibilidad comercial y precio publicado son cosas distintas:
/// Empresa existe y se vende, pero por contacto (no publica precio), así que
/// nunca es "Invalid plan" ni "not on sale".
fn resolve_checkout_plan(requested: &str) -> Result<Plan, CreateCheckoutResponse> {
    resolve_checkout_plan_gated(requested, None)
}

/// Núcleo de [`resolve_checkout_plan`].
///
/// `purchasable_override` inyecta el resultado del gate de venta: `None` usa el
/// del catálogo; `Some(true)` permite a los tests alcanzar la rama
/// `Ok(plan)` sin abrir la venta real de ningún plan.
fn resolve_checkout_plan_gated(
    requested: &str,
    purchasable_override: Option<bool>,
) -> Result<Plan, CreateCheckoutResponse> {
    let Some(plan) = Plan::from_catalog_id(requested) else {
        return Err(invalid_plan_error());
    };

    // Plan a medida: existe y se vende, pero por contacto, no en checkout.
    if plan.monthly_price_cents().is_none() {
        return Err(CreateCheckoutResponse {
            status: "error".to_string(),
            checkout_url: None,
            message: Some(format!(
                "Plan {plan} is contact-only: contact sales to subscribe"
            )),
        });
    }

    // Catálogo v1: ningún plan de pago se vende todavía (cobro con Polar pendiente).
    let purchasable = purchasable_override.unwrap_or_else(|| plan.is_purchasable());
    if !purchasable {
        return Err(not_on_sale_error(plan));
    }

    Ok(plan)
}

/// Igual que [`resolve_checkout_plan`] pero forzando el plan como vendible.
///
/// Gancho de test: el catálogo v1 no vende ningún plan de pago, así que sin
/// esta inyección ningún test podría llegar al camino `Ok(plan)`.
#[cfg(test)]
fn resolve_checkout_plan_sellable(requested: &str) -> Result<Plan, CreateCheckoutResponse> {
    resolve_checkout_plan_gated(requested, Some(true))
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
    fn unknown_plan_error_names_the_whole_catalog() {
        let response =
            resolve_checkout_plan("diamante").expect_err("un plan fuera del catálogo no existe");
        assert_eq!(response.status, "error");
        let message = response.message.expect("el error debe explicarse");
        assert!(message.starts_with("Invalid plan."), "{message}");
        // Los seis planes del catálogo, en orden, incluido Empresa: antes el
        // mensaje se lo saltaba y además lo clasificaba como inexistente.
        assert!(
            message.ends_with("local, fundador, respaldo, pro, equipo, empresa"),
            "{message}"
        );
        for plan in Plan::ALL {
            assert!(message.contains(plan.catalog_id()), "{message} sin {plan}");
        }
    }

    #[test]
    fn empresa_is_contact_only_not_invalid() {
        let response =
            resolve_checkout_plan("empresa").expect_err("Empresa no se contrata en checkout");
        assert_eq!(response.status, "error");
        let message = response.message.expect("el error debe explicarse");
        assert!(message.contains("contact-only"), "{message}");
        assert!(message.contains("contact sales"), "{message}");
        // Que no caiga en ninguna de las otras dos ramas.
        assert!(!message.contains("Invalid plan"), "{message}");
        assert!(!message.contains("not on sale yet"), "{message}");
    }

    #[test]
    fn every_catalog_plan_is_a_known_plan() {
        // Cada id del catálogo (más los alias `free`/`cloud`) debe resolverse a
        // un plan real; el único error posible hoy es el de no poder cobrarse.
        for id in [
            "local", "free", "fundador", "respaldo", "cloud", "pro", "equipo", "empresa",
        ] {
            let message = resolve_checkout_plan(id)
                .expect_err("ningún plan se vende todavía")
                .message
                .unwrap_or_default();
            assert!(!message.contains("Invalid plan"), "{id}: {message}");
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
    fn free_plan_is_not_a_checkout_either() {
        // Local existe y publica precio (0): error propio, no el de espera.
        let message = resolve_checkout_plan("local")
            .expect_err("Local no se contrata por checkout")
            .message
            .unwrap_or_default();
        assert!(message.contains("free"), "{message}");
        assert!(!message.contains("not on sale yet"), "{message}");
    }

    #[test]
    fn a_sellable_plan_resolves_to_ok() {
        for (requested, expected) in [
            ("local", Plan::Free),
            ("fundador", Plan::Fundador),
            ("respaldo", Plan::Cloud),
            ("cloud", Plan::Cloud),
            ("pro", Plan::Pro),
            ("equipo", Plan::Equipo),
        ] {
            let resolved = resolve_checkout_plan_sellable(requested)
                .unwrap_or_else(|e| panic!("{requested} debe resolver: {e:?}"));
            assert_eq!(resolved, expected, "{requested}");
        }
        // El gate inyectado no convierte un plan a medida en autoservicio: el
        // gate de contacto va antes.
        assert!(resolve_checkout_plan_sellable("empresa").is_err());
        // Y un plan fuera del catálogo sigue siendo inválido.
        assert!(resolve_checkout_plan_sellable("diamante").is_err());
    }

    #[test]
    fn billing_plans_publishes_status_and_purchasable_flag() {
        let plans = plan_catalog();

        // Nombres y orden exactos del catálogo publicado (no solo el tamaño).
        let names: Vec<&str> = plans.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["local", "fundador", "respaldo", "pro", "equipo", "empresa"]
        );
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
