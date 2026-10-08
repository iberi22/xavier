//! Plan definitions and limits for Xavier billing tiers.
//!
//! Los precios y límites salen del catálogo canónico
//! `iberi22/xavier-cloud` → `plans/catalog.v1.json` (versión 2026-10-08.1).
//! `catalog.v1.json` en esta carpeta es una copia exacta; los tests de abajo
//! fallan si este archivo y la copia dejan de coincidir.

use serde::{Deserialize, Serialize};

/// Versión del catálogo canónico que implementa este archivo.
pub const CATALOG_VERSION: &str = "2026-10-08.1";

const GIB: u64 = 1024 * 1024 * 1024;

/// Billing plan tiers (catálogo v1).
///
/// Los nombres de variante `Free` y `Cloud` se conservan por compatibilidad;
/// en el catálogo se llaman `local` y `respaldo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    /// Local: Xavier completo en la máquina del usuario, sin nube. 0 USD.
    #[serde(rename = "local", alias = "free")]
    Free,
    /// Fundador: igual a Respaldo, 10 USD/mes con precio congelado (máx. 50 clientes).
    Fundador,
    /// Respaldo: 10 USD/mes o 100 USD/año. 5 GiB, 3 equipos, IA 50k tokens/día.
    #[serde(rename = "respaldo", alias = "cloud")]
    Cloud,
    /// Pro: 19 USD/mes, añade MCP alojado. Próximamente.
    Pro,
    /// Equipo: 49 USD/mes con 3 asientos, +12 USD por asiento extra. Próximamente.
    Equipo,
    /// Empresa: a medida (self-host, SLA, licencia comercial).
    Empresa,
}

/// Estado comercial del plan en el catálogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    /// Disponible hoy.
    Available,
    /// Lista de espera, sin cobro activo.
    Waitlist,
    /// Próximamente, no se vende todavía.
    ComingSoon,
    /// A medida, por contacto.
    Contact,
}

impl Plan {
    /// Todos los planes del catálogo, en orden de la escalera.
    pub const ALL: [Plan; 6] = [
        Plan::Free,
        Plan::Fundador,
        Plan::Cloud,
        Plan::Pro,
        Plan::Equipo,
        Plan::Empresa,
    ];

    /// Identificador del plan en el catálogo canónico.
    pub fn catalog_id(&self) -> &'static str {
        match self {
            Self::Free => "local",
            Self::Fundador => "fundador",
            Self::Cloud => "respaldo",
            Self::Pro => "pro",
            Self::Equipo => "equipo",
            Self::Empresa => "empresa",
        }
    }

    /// Nombre visible.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Free => "Local",
            Self::Fundador => "Fundador",
            Self::Cloud => "Respaldo",
            Self::Pro => "Pro",
            Self::Equipo => "Equipo",
            Self::Empresa => "Empresa",
        }
    }

    /// Busca un plan por id del catálogo (acepta los alias `free` y `cloud`).
    pub fn from_catalog_id(id: &str) -> Option<Self> {
        match id.to_ascii_lowercase().as_str() {
            "local" | "free" => Some(Self::Free),
            "fundador" => Some(Self::Fundador),
            "respaldo" | "cloud" => Some(Self::Cloud),
            "pro" => Some(Self::Pro),
            "equipo" => Some(Self::Equipo),
            "empresa" => Some(Self::Empresa),
            _ => None,
        }
    }

    /// Estado comercial según el catálogo.
    pub fn status(&self) -> PlanStatus {
        match self {
            Self::Free => PlanStatus::Available,
            Self::Fundador | Self::Cloud => PlanStatus::Waitlist,
            Self::Pro | Self::Equipo => PlanStatus::ComingSoon,
            Self::Empresa => PlanStatus::Contact,
        }
    }

    /// `true` solo si el plan se puede contratar por autoservicio hoy.
    /// Ninguno de pago lo está todavía (cobro con Polar pendiente).
    pub fn is_purchasable(&self) -> bool {
        matches!(self.status(), PlanStatus::Available) && self.monthly_price_cents() > 0
    }

    /// Variable de entorno con el id de precio remoto (Stripe/Polar), si aplica.
    fn price_env(&self) -> Option<&'static str> {
        match self {
            Self::Free | Self::Empresa => None,
            Self::Fundador => Some("XAVIER_PRICE_FUNDADOR"),
            Self::Cloud => Some("XAVIER_PRICE_RESPALDO"),
            Self::Pro => Some("XAVIER_PRICE_PRO"),
            Self::Equipo => Some("XAVIER_PRICE_EQUIPO"),
        }
    }

    /// Get plan from a remote price ID.
    ///
    /// `STRIPE_PRICE_CLOUD` se sigue aceptando como alias de Respaldo.
    pub fn from_price_id(price_id: &str) -> Option<Self> {
        if price_id.is_empty() {
            return None;
        }
        for plan in Self::ALL {
            if let Some(var) = plan.price_env() {
                if std::env::var(var).ok().as_deref() == Some(price_id) {
                    return Some(plan);
                }
            }
        }
        if std::env::var("STRIPE_PRICE_CLOUD").ok().as_deref() == Some(price_id) {
            return Some(Self::Cloud);
        }
        None
    }

    /// Convert plan to a remote price ID.
    pub fn price_id(&self) -> Option<String> {
        let var = self.price_env()?;
        std::env::var(var).ok().or_else(|| {
            if *self == Self::Cloud {
                std::env::var("STRIPE_PRICE_CLOUD").ok()
            } else {
                None
            }
        })
    }

    /// Monthly price in cents (0 = gratis o a medida).
    pub fn monthly_price_cents(&self) -> u32 {
        match self {
            Self::Free => 0,
            Self::Fundador => 1_000,
            Self::Cloud => 1_000,
            Self::Pro => 1_900,
            Self::Equipo => 4_900,
            Self::Empresa => 0,
        }
    }

    /// Yearly price in cents, si el plan tiene precio anual.
    pub fn yearly_price_cents(&self) -> Option<u32> {
        match self {
            Self::Cloud => Some(10_000),
            _ => None,
        }
    }

    /// Precio por asiento extra en centavos (solo Equipo).
    pub fn extra_seat_monthly_cents(&self) -> Option<u32> {
        match self {
            Self::Equipo => Some(1_200),
            _ => None,
        }
    }
}

impl std::fmt::Display for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.catalog_id())
    }
}

/// Plan limits and features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanLimits {
    /// Maximum storage in GB (GiB). 0 = sin nube o a medida (ver `storage_bytes`).
    pub max_storage_gb: usize,
    /// Almacenamiento exacto en bytes; `None` = a medida.
    pub storage_bytes: Option<u64>,
    /// Maximum number of devices/nodes (0 for unlimited or custom)
    pub max_nodes: usize,
    /// Tokens de IA en la nube por día; `None` = a medida.
    pub ai_tokens_per_day: Option<u64>,
    /// Clientes MCP conectados al endpoint alojado; `None` = no aplica o sin tope.
    pub mcp_clients: Option<u32>,
    /// Asientos incluidos; `None` = a medida.
    pub seats_included: Option<u32>,
    /// List of feature flags enabled for this plan
    pub features: Vec<String>,
}

fn feats(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

impl PlanLimits {
    /// Get limits for a specific plan
    pub fn for_plan(plan: Plan) -> Self {
        let respaldo_feats = [
            "encrypted_backup",
            "restore",
            "sync_devices",
            "cloud_ai_capped",
            "email_support",
        ];
        match plan {
            Plan::Free => Self {
                max_storage_gb: 0,
                storage_bytes: Some(0),
                max_nodes: 0,
                ai_tokens_per_day: Some(0),
                mcp_clients: None,
                seats_included: Some(1),
                features: feats(&[
                    "xavier_local_agpl",
                    "mcp_local",
                    "cli",
                    "rest",
                    "codegraph",
                    "session_importers",
                    "mesh_own_devices",
                ]),
            },
            Plan::Fundador => {
                let mut f = feats(&respaldo_feats);
                f.push("founder_channel".to_string());
                Self {
                    max_storage_gb: 5,
                    storage_bytes: Some(5 * GIB),
                    max_nodes: 3,
                    ai_tokens_per_day: Some(50_000),
                    mcp_clients: None,
                    seats_included: Some(1),
                    features: f,
                }
            }
            Plan::Cloud => Self {
                max_storage_gb: 5,
                storage_bytes: Some(5 * GIB),
                max_nodes: 3,
                ai_tokens_per_day: Some(50_000),
                mcp_clients: None,
                seats_included: Some(1),
                features: feats(&respaldo_feats),
            },
            Plan::Pro => {
                let mut f = feats(&respaldo_feats);
                f.extend(feats(&["hosted_mcp", "embeddings_included"]));
                Self {
                    max_storage_gb: 10,
                    storage_bytes: Some(10 * GIB),
                    max_nodes: 3,
                    ai_tokens_per_day: Some(100_000),
                    mcp_clients: Some(5),
                    seats_included: Some(1),
                    features: f,
                }
            }
            Plan::Equipo => Self {
                max_storage_gb: 25,
                storage_bytes: Some(25 * GIB),
                max_nodes: 0,
                ai_tokens_per_day: Some(300_000),
                mcp_clients: None,
                seats_included: Some(3),
                features: feats(&[
                    "encrypted_backup",
                    "restore",
                    "sync_devices",
                    "cloud_ai_capped",
                    "hosted_mcp",
                    "embeddings_included",
                    "spaces_per_client",
                    "acl",
                    "audit_log",
                    "priority_support",
                ]),
            },
            Plan::Empresa => Self {
                max_storage_gb: 0,
                storage_bytes: None,
                max_nodes: 0,
                ai_tokens_per_day: None,
                mcp_clients: None,
                seats_included: None,
                features: feats(&["self_host", "sla", "commercial_license"]),
            },
        }
    }

    /// Check if a feature is enabled for this plan
    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

/// Current subscription status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionStatus {
    /// Current plan
    pub plan: Plan,
    /// Stripe customer ID
    pub stripe_customer_id: Option<String>,
    /// Stripe subscription ID
    pub stripe_subscription_id: Option<String>,
    /// Subscription status from Stripe
    pub subscription_status: String,
    /// Current period end timestamp
    pub current_period_end: Option<i64>,
    /// Whether the subscription is active
    pub is_active: bool,
    /// Plan limits
    pub limits: PlanLimits,
}

impl Default for SubscriptionStatus {
    fn default() -> Self {
        Self {
            plan: Plan::Free,
            stripe_customer_id: None,
            stripe_subscription_id: None,
            subscription_status: "none".to_string(),
            current_period_end: None,
            is_active: false,
            limits: PlanLimits::for_plan(Plan::Free),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = include_str!("catalog.v1.json");

    fn catalog() -> serde_json::Value {
        serde_json::from_str(CATALOG).expect("catalog.v1.json válido")
    }

    fn entry(id: &str) -> serde_json::Value {
        catalog()["plans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("plan {id} no está en el catálogo"))
    }

    fn opt_u64(v: &serde_json::Value) -> Option<u64> {
        v.as_u64()
    }

    #[test]
    fn test_plan_from_price_id() {
        // This test requires env vars to be set
        let result = Plan::from_price_id("price_nonexistent");
        assert!(result.is_none());
        assert!(Plan::from_price_id("").is_none());
    }

    #[test]
    fn test_plan_limits() {
        let free_limits = PlanLimits::for_plan(Plan::Free);
        assert_eq!(free_limits.max_storage_gb, 0);
        assert!(free_limits.has_feature("mcp_local"));

        let cloud_limits = PlanLimits::for_plan(Plan::Cloud);
        assert_eq!(cloud_limits.max_storage_gb, 5);
        assert!(cloud_limits.has_feature("encrypted_backup"));
        assert!(!cloud_limits.has_feature("hosted_mcp"));
        assert!(PlanLimits::for_plan(Plan::Pro).has_feature("hosted_mcp"));
    }

    #[test]
    fn test_subscription_status_default() {
        let status = SubscriptionStatus::default();
        assert_eq!(status.plan, Plan::Free);
        assert!(!status.is_active);
    }

    #[test]
    fn catalog_version_matches() {
        assert_eq!(catalog()["version"], CATALOG_VERSION);
        assert_eq!(
            catalog()["plans"].as_array().unwrap().len(),
            Plan::ALL.len()
        );
    }

    #[test]
    fn prices_match_catalog() {
        for plan in Plan::ALL {
            let e = entry(plan.catalog_id());
            let monthly = e["price"]["monthly_usd"].as_u64().unwrap_or(0) as u32;
            assert_eq!(plan.monthly_price_cents(), monthly * 100, "{plan}");
            let yearly = e["price"]["yearly_usd"].as_u64().map(|y| y as u32 * 100);
            assert_eq!(plan.yearly_price_cents(), yearly, "{plan}");
            let seat = e["price"]["extra_seat_monthly_usd"]
                .as_u64()
                .map(|y| y as u32 * 100);
            assert_eq!(plan.extra_seat_monthly_cents(), seat, "{plan}");
            assert_eq!(e["name"], plan.display_name());
            let status: PlanStatus = serde_json::from_value(e["status"].clone()).unwrap();
            assert_eq!(plan.status(), status, "{plan}");
        }
        assert_eq!(entry("fundador")["max_customers"], 50);
        assert_eq!(entry("fundador")["price"]["price_locked_for_life"], true);
    }

    #[test]
    fn limits_match_catalog() {
        for plan in Plan::ALL {
            let e = entry(plan.catalog_id());
            let l = PlanLimits::for_plan(plan);
            assert_eq!(
                l.storage_bytes,
                opt_u64(&e["limits"]["storage_bytes"]),
                "{plan}"
            );
            assert_eq!(
                l.ai_tokens_per_day,
                opt_u64(&e["limits"]["ai_tokens_per_day"]),
                "{plan}"
            );
            assert_eq!(
                l.mcp_clients.map(u64::from),
                opt_u64(&e["limits"]["mcp_clients"]),
                "{plan}"
            );
            assert_eq!(
                l.seats_included.map(u64::from),
                opt_u64(&e["limits"]["seats_included"]),
                "{plan}"
            );
            let devices = opt_u64(&e["limits"]["devices"]).unwrap_or(0) as usize;
            assert_eq!(l.max_nodes, devices, "{plan}");
            let feats: Vec<String> = serde_json::from_value(e["features"].clone()).unwrap();
            assert_eq!(l.features, feats, "{plan}");
        }
    }

    #[test]
    fn nothing_paid_is_purchasable_yet() {
        for plan in Plan::ALL {
            assert!(!plan.is_purchasable(), "{plan} no debe venderse todavía");
        }
    }

    #[test]
    fn serde_ids_and_aliases() {
        assert_eq!(serde_json::to_string(&Plan::Free).unwrap(), "\"local\"");
        assert_eq!(serde_json::to_string(&Plan::Cloud).unwrap(), "\"respaldo\"");
        let old: Plan = serde_json::from_str("\"cloud\"").unwrap();
        assert_eq!(old, Plan::Cloud);
        let old: Plan = serde_json::from_str("\"free\"").unwrap();
        assert_eq!(old, Plan::Free);
        for plan in Plan::ALL {
            assert_eq!(Plan::from_catalog_id(plan.catalog_id()), Some(plan));
            let s = serde_json::to_string(&plan).unwrap();
            assert_eq!(s, format!("\"{}\"", plan.catalog_id()));
        }
    }
}
