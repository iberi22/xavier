//! HTTP client for the billing services Xavier talks to.
//!
//! * **swal-billing** (`XAVIER_BILLING_URL`): public plan catalog
//!   (`GET /v1/xavier/plans`) and checkout (`POST /v1/xavier/checkout`).
//! * **xavier-cloud** (`PGHEART_URL` / `XAVIER_PGHEART_URL`): per-tenant usage and
//!   plan (`GET /v1/cloud/usage`), authenticated with the tenant API key
//!   (`PGHEART_TOKEN` / `XAVIER_PGHEART_TOKEN`).
//!
//! Nothing here invents data: every value shown to the user comes from one of
//! those servers. When a server is unreachable or rejects the credentials the
//! caller gets a typed [`BillingClientError`] with an actionable message.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// Env var with the swal-billing base URL.
pub const BILLING_URL_ENV: &str = "XAVIER_BILLING_URL";
/// Default swal-billing URL: the `wrangler dev` address. swal-billing has no
/// public deployment yet; set [`BILLING_URL_ENV`] to point somewhere else.
pub const DEFAULT_BILLING_URL: &str = "http://127.0.0.1:8787";
/// Default xavier-cloud URL (production) when neither `PGHEART_URL` nor
/// `XAVIER_PGHEART_URL` is set.
pub const DEFAULT_CLOUD_URL: &str = "https://xavier-cloud.swal.network";

/// Env vars for the xavier-cloud URL, in priority order.
pub const CLOUD_URL_ENVS: [&str; 2] = ["PGHEART_URL", "XAVIER_PGHEART_URL"];
/// Env vars for the xavier-cloud tenant API key, in priority order.
pub const CLOUD_TOKEN_ENVS: [&str; 2] = ["PGHEART_TOKEN", "XAVIER_PGHEART_TOKEN"];

/// Where the billing client sends its requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillingEndpoints {
    /// swal-billing base URL (no trailing slash).
    pub billing_url: String,
    /// xavier-cloud base URL (no trailing slash).
    pub cloud_url: String,
    /// Tenant API key for xavier-cloud, if configured.
    pub cloud_token: Option<String>,
}

fn first_non_empty(lookup: &dyn Fn(&str) -> Option<String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|k| lookup(k))
        .map(|v| v.trim().to_string())
        .find(|v| !v.is_empty())
}

fn normalize_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

impl BillingEndpoints {
    /// Resolve endpoints from an arbitrary key lookup (env vars in production,
    /// a map in tests).
    pub fn resolve_with(lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let billing_url = first_non_empty(lookup, &[BILLING_URL_ENV])
            .unwrap_or_else(|| DEFAULT_BILLING_URL.to_string());
        let cloud_url = first_non_empty(lookup, &CLOUD_URL_ENVS)
            .unwrap_or_else(|| DEFAULT_CLOUD_URL.to_string());
        Self {
            billing_url: normalize_url(&billing_url),
            cloud_url: normalize_url(&cloud_url),
            cloud_token: first_non_empty(lookup, &CLOUD_TOKEN_ENVS),
        }
    }

    /// Resolve endpoints from the process environment.
    pub fn from_env() -> Self {
        Self::resolve_with(&|k| std::env::var(k).ok())
    }
}

/// Errors with a message the CLI can print as-is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BillingClientError {
    /// TCP/DNS/TLS/timeout failure: the server could not be reached.
    Offline {
        service: &'static str,
        url: String,
        detail: String,
    },
    /// The server rejected the credentials (HTTP 401/403).
    Unauthenticated { service: &'static str, status: u16 },
    /// No tenant API key configured for xavier-cloud.
    MissingToken,
    /// The plan id is not in the catalog served by swal-billing (HTTP 400).
    UnknownPlan { plan: String },
    /// Request rejected as invalid (HTTP 400) for another reason.
    BadRequest { message: String },
    /// The endpoint does not exist on that server (HTTP 404).
    NotFound { service: &'static str, url: String },
    /// Any other non-success HTTP status.
    Http {
        service: &'static str,
        status: u16,
        body: String,
    },
    /// The response body was not what the API contract says.
    InvalidResponse {
        service: &'static str,
        detail: String,
    },
}

impl fmt::Display for BillingClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Offline {
                service,
                url,
                detail,
            } => write!(
                f,
                "cannot reach {service} at {url} ({detail}). Check your connection{}",
                if *service == "swal-billing" {
                    format!(", or start swal-billing locally (`pnpm dev`) / set {BILLING_URL_ENV}")
                } else {
                    " or PGHEART_URL / XAVIER_PGHEART_URL".to_string()
                }
            ),
            Self::Unauthenticated { service, status } => write!(
                f,
                "{service} rejected the credentials (HTTP {status}). \
Check that PGHEART_TOKEN (or XAVIER_PGHEART_TOKEN) is a valid tenant API key"
            ),
            Self::MissingToken => write!(
                f,
                "not authenticated: set PGHEART_TOKEN (or XAVIER_PGHEART_TOKEN) to your \
xavier-cloud tenant API key"
            ),
            Self::UnknownPlan { plan } => write!(
                f,
                "unknown plan '{plan}'. Run `xavier billing plans` to see the catalog"
            ),
            Self::BadRequest { message } => write!(f, "request rejected: {message}"),
            Self::NotFound { service, url } => write!(
                f,
                "{url} does not expose this endpoint (HTTP 404). Is it really {service}?"
            ),
            Self::Http {
                service,
                status,
                body,
            } => {
                write!(f, "{service} answered HTTP {status}: {body}")
            }
            Self::InvalidResponse { service, detail } => {
                write!(f, "unexpected response from {service}: {detail}")
            }
        }
    }
}

impl std::error::Error for BillingClientError {}

/// Price block of a catalog plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanPrice {
    pub monthly_usd: Option<f64>,
    #[serde(default)]
    pub yearly_usd: Option<f64>,
    #[serde(default)]
    pub extra_seat_monthly_usd: Option<f64>,
}

/// Limits block of a catalog plan (`None` = unlimited / custom).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanLimitsView {
    #[serde(default)]
    pub storage_bytes: Option<u64>,
    #[serde(default)]
    pub devices: Option<u64>,
    #[serde(default)]
    pub ai_tokens_per_day: Option<u64>,
    #[serde(default)]
    pub seats_included: Option<u64>,
}

/// One plan as served by `GET /v1/xavier/plans`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogPlanView {
    pub id: String,
    pub name: String,
    pub status: String,
    pub price: PlanPrice,
    #[serde(default)]
    pub cloud: bool,
    pub limits: PlanLimitsView,
    #[serde(default)]
    pub features: Vec<String>,
}

/// Body of `GET /v1/xavier/plans`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlansResponse {
    pub version: String,
    pub currency: String,
    pub payment_provider: String,
    pub plans: Vec<CatalogPlanView>,
}

/// Result of `POST /v1/xavier/checkout`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CheckoutOutcome {
    /// A payment page was created.
    Url { url: String },
    /// The plan exists but cannot be bought yet (HTTP 409: waitlist / coming soon / contact).
    NotAvailable {
        plan: String,
        status: String,
        message: Option<String>,
    },
    /// The plan is purchasable but the payment provider is not configured (HTTP 501).
    NotConfigured { message: String },
}

/// Body of `POST /v1/xavier/checkout`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckoutRequest {
    pub plan: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// `Some(None)` when the field is present and `null`, `None` when it is absent.
fn present_or_null<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(d).map(Some)
}

/// Body of xavier-cloud `GET /v1/cloud/usage`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudUsage {
    pub tenant: String,
    /// `None`: the server does not report plans (xavier-cloud older than the
    /// per-plan quotas). `Some(None)`: no plan assigned (legacy quota).
    #[serde(
        default,
        deserialize_with = "present_or_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub plan: Option<Option<String>>,
    pub bytes: u64,
    #[serde(default)]
    pub chunks: Option<u64>,
    pub quota: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_tokens_per_day: Option<u64>,
    #[serde(default)]
    pub percent: Option<f64>,
}

/// Client for swal-billing and xavier-cloud.
#[derive(Debug, Clone)]
pub struct BillingClient {
    http: reqwest::Client,
    endpoints: BillingEndpoints,
}

const SWAL_BILLING: &str = "swal-billing";
const XAVIER_CLOUD: &str = "xavier-cloud";

fn offline(service: &'static str, url: &str, err: &reqwest::Error) -> BillingClientError {
    let detail = if err.is_timeout() {
        "timed out".to_string()
    } else if err.is_connect() {
        "connection failed".to_string()
    } else {
        err.to_string()
    };
    BillingClientError::Offline {
        service,
        url: url.to_string(),
        detail,
    }
}

fn error_field(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("error")?
        .as_str()
        .map(str::to_string)
}

fn truncate(body: &str) -> String {
    const MAX: usize = 300;
    if body.len() <= MAX {
        body.to_string()
    } else {
        let mut end = MAX;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &body[..end])
    }
}

fn generic_status_error(
    service: &'static str,
    url: &str,
    status: u16,
    body: &str,
) -> BillingClientError {
    match status {
        401 | 403 => BillingClientError::Unauthenticated { service, status },
        404 => BillingClientError::NotFound {
            service,
            url: url.to_string(),
        },
        _ => BillingClientError::Http {
            service,
            status,
            body: truncate(body),
        },
    }
}

impl BillingClient {
    /// Build a client with an explicit HTTP client and endpoints.
    pub fn new(http: reqwest::Client, endpoints: BillingEndpoints) -> Self {
        Self { http, endpoints }
    }

    /// Endpoints this client uses.
    pub fn endpoints(&self) -> &BillingEndpoints {
        &self.endpoints
    }

    /// `GET {billing}/v1/xavier/plans`.
    pub async fn plans(&self) -> Result<PlansResponse, BillingClientError> {
        let url = format!("{}/v1/xavier/plans", self.endpoints.billing_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| offline(SWAL_BILLING, &self.endpoints.billing_url, &e))?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| offline(SWAL_BILLING, &self.endpoints.billing_url, &e))?;
        if !(200..300).contains(&status) {
            return Err(generic_status_error(SWAL_BILLING, &url, status, &body));
        }
        serde_json::from_str(&body).map_err(|e| BillingClientError::InvalidResponse {
            service: SWAL_BILLING,
            detail: e.to_string(),
        })
    }

    /// `POST {billing}/v1/xavier/checkout`.
    pub async fn checkout(
        &self,
        req: &CheckoutRequest,
    ) -> Result<CheckoutOutcome, BillingClientError> {
        let url = format!("{}/v1/xavier/checkout", self.endpoints.billing_url);
        let resp = self
            .http
            .post(&url)
            .json(req)
            .send()
            .await
            .map_err(|e| offline(SWAL_BILLING, &self.endpoints.billing_url, &e))?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| offline(SWAL_BILLING, &self.endpoints.billing_url, &e))?;
        let json: Option<serde_json::Value> = serde_json::from_str(&body).ok();
        let field = |k: &str| {
            json.as_ref()
                .and_then(|v| v.get(k))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };
        match status {
            200..=299 => match field("url") {
                Some(url) => Ok(CheckoutOutcome::Url { url }),
                None => Err(BillingClientError::InvalidResponse {
                    service: SWAL_BILLING,
                    detail: "checkout succeeded without a `url`".to_string(),
                }),
            },
            409 => Ok(CheckoutOutcome::NotAvailable {
                plan: field("plan").unwrap_or_else(|| req.plan.clone()),
                status: field("status").unwrap_or_else(|| "not_available".to_string()),
                message: field("message"),
            }),
            501 => Ok(CheckoutOutcome::NotConfigured {
                message: field("error").unwrap_or_else(|| "checkout not configured".to_string()),
            }),
            400 => match error_field(&body).as_deref() {
                Some("unknown plan") => Err(BillingClientError::UnknownPlan {
                    plan: req.plan.clone(),
                }),
                Some(other) => Err(BillingClientError::BadRequest {
                    message: other.to_string(),
                }),
                None => Err(BillingClientError::BadRequest {
                    message: truncate(&body),
                }),
            },
            _ => Err(generic_status_error(SWAL_BILLING, &url, status, &body)),
        }
    }

    /// `GET {cloud}/v1/cloud/usage` with `Authorization: Bearer <tenant key>`.
    pub async fn cloud_usage(&self) -> Result<CloudUsage, BillingClientError> {
        let token = self
            .endpoints
            .cloud_token
            .as_deref()
            .ok_or(BillingClientError::MissingToken)?;
        let url = format!("{}/v1/cloud/usage", self.endpoints.cloud_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| offline(XAVIER_CLOUD, &self.endpoints.cloud_url, &e))?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| offline(XAVIER_CLOUD, &self.endpoints.cloud_url, &e))?;
        if !(200..300).contains(&status) {
            return Err(generic_status_error(XAVIER_CLOUD, &url, status, &body));
        }
        serde_json::from_str(&body).map_err(|e| BillingClientError::InvalidResponse {
            service: XAVIER_CLOUD,
            detail: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn endpoints(billing: &str, cloud: &str, token: Option<&str>) -> BillingEndpoints {
        BillingEndpoints {
            billing_url: billing.to_string(),
            cloud_url: cloud.to_string(),
            cloud_token: token.map(str::to_string),
        }
    }

    fn client(e: BillingEndpoints) -> BillingClient {
        BillingClient::new(reqwest::Client::new(), e)
    }

    /// Body that the real swal-billing route serves (it is the catalog file).
    fn catalog_body() -> String {
        include_str!("catalog.v1.json").to_string()
    }

    #[test]
    fn resolve_defaults_when_nothing_is_set() {
        let e = BillingEndpoints::resolve_with(&|_| None);
        assert_eq!(e.billing_url, DEFAULT_BILLING_URL);
        assert_eq!(e.cloud_url, DEFAULT_CLOUD_URL);
        assert_eq!(e.cloud_token, None);
    }

    #[test]
    fn resolve_prefers_pgheart_and_falls_back_to_xavier_alias() {
        let vars: HashMap<&str, &str> = [
            ("XAVIER_BILLING_URL", "https://billing.test/ "),
            ("XAVIER_PGHEART_URL", "https://alias.test/"),
            ("XAVIER_PGHEART_TOKEN", "alias-token"),
            ("PGHEART_TOKEN", "primary-token"),
            ("PGHEART_URL", "  "),
        ]
        .into_iter()
        .collect();
        let e = BillingEndpoints::resolve_with(&|k| vars.get(k).map(|v| v.to_string()));
        assert_eq!(e.billing_url, "https://billing.test");
        // Empty PGHEART_URL is ignored -> alias wins.
        assert_eq!(e.cloud_url, "https://alias.test");
        assert_eq!(e.cloud_token.as_deref(), Some("primary-token"));
    }

    #[tokio::test]
    async fn plans_parses_the_catalog_served_by_swal_billing() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/v1/xavier/plans")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(catalog_body())
            .create_async()
            .await;
        let c = client(endpoints(&server.url(), "http://unused", None));
        let plans = c.plans().await.expect("plans");
        m.assert_async().await;
        assert_eq!(plans.version, crate::billing::plans::CATALOG_VERSION);
        assert_eq!(plans.payment_provider, "polar");
        let ids: Vec<_> = plans.plans.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            ["local", "fundador", "respaldo", "pro", "equipo", "empresa"]
        );
        let equipo = plans.plans.iter().find(|p| p.id == "equipo").unwrap();
        assert_eq!(equipo.price.extra_seat_monthly_usd, Some(12.0));
        assert_eq!(equipo.limits.seats_included, Some(3));
    }

    #[tokio::test]
    async fn plans_reports_invalid_json() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/v1/xavier/plans")
            .with_status(200)
            .with_body("<html>not json</html>")
            .create_async()
            .await;
        let err = client(endpoints(&server.url(), "http://unused", None))
            .plans()
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            BillingClientError::InvalidResponse {
                service: "swal-billing",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn plans_offline_is_a_clear_error() {
        // Bind and drop a listener to get a port nobody is listening on.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let url = format!("http://127.0.0.1:{port}");
        let err = client(endpoints(&url, "http://unused", None))
            .plans()
            .await
            .unwrap_err();
        match &err {
            BillingClientError::Offline {
                service, url: u, ..
            } => {
                assert_eq!(*service, "swal-billing");
                assert_eq!(u, &url);
            }
            other => panic!("expected Offline, got {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("cannot reach swal-billing"), "{msg}");
        assert!(msg.contains("XAVIER_BILLING_URL"), "{msg}");
    }

    #[tokio::test]
    async fn checkout_waitlist_409_is_not_an_error() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("POST", "/v1/xavier/checkout")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "plan": "fundador",
                "tenant": "groku-test",
                "email": "me@example.com"
            })))
            .with_status(409)
            .with_body(
                r#"{"error":"not_available","plan":"fundador","status":"waitlist","message":"Fundador está en lista de espera."}"#,
            )
            .create_async()
            .await;
        let out = client(endpoints(&server.url(), "http://unused", None))
            .checkout(&CheckoutRequest {
                plan: "fundador".into(),
                tenant: Some("groku-test".into()),
                email: Some("me@example.com".into()),
            })
            .await
            .expect("409 is an outcome, not an error");
        m.assert_async().await;
        assert_eq!(
            out,
            CheckoutOutcome::NotAvailable {
                plan: "fundador".into(),
                status: "waitlist".into(),
                message: Some("Fundador está en lista de espera.".into()),
            }
        );
    }

    #[tokio::test]
    async fn checkout_omits_unset_tenant_and_email() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("POST", "/v1/xavier/checkout")
            .match_body(mockito::Matcher::Json(serde_json::json!({ "plan": "pro" })))
            .with_status(409)
            .with_body(r#"{"error":"not_available","plan":"pro","status":"coming_soon"}"#)
            .create_async()
            .await;
        let out = client(endpoints(&server.url(), "http://unused", None))
            .checkout(&CheckoutRequest {
                plan: "pro".into(),
                tenant: None,
                email: None,
            })
            .await
            .unwrap();
        m.assert_async().await;
        assert!(
            matches!(out, CheckoutOutcome::NotAvailable { ref status, message: None, .. } if status == "coming_soon")
        );
    }

    #[tokio::test]
    async fn checkout_maps_unknown_plan_bad_request_501_and_url() {
        let mut server = mockito::Server::new_async().await;
        let c = client(endpoints(&server.url(), "http://unused", None));
        let req = |p: &str| CheckoutRequest {
            plan: p.into(),
            tenant: None,
            email: None,
        };

        let _a = server
            .mock("POST", "/v1/xavier/checkout")
            .match_body(mockito::Matcher::PartialJson(
                serde_json::json!({"plan": "gold"}),
            ))
            .with_status(400)
            .with_body(r#"{"error":"unknown plan"}"#)
            .create_async()
            .await;
        let _b = server
            .mock("POST", "/v1/xavier/checkout")
            .match_body(mockito::Matcher::PartialJson(
                serde_json::json!({"plan": "bad-email"}),
            ))
            .with_status(400)
            .with_body(r#"{"error":"invalid email"}"#)
            .create_async()
            .await;
        let _c = server
            .mock("POST", "/v1/xavier/checkout")
            .match_body(mockito::Matcher::PartialJson(
                serde_json::json!({"plan": "nc"}),
            ))
            .with_status(501)
            .with_body(r#"{"error":"polar checkout not configured"}"#)
            .create_async()
            .await;
        let _d = server
            .mock("POST", "/v1/xavier/checkout")
            .match_body(mockito::Matcher::PartialJson(
                serde_json::json!({"plan": "ok"}),
            ))
            .with_status(200)
            .with_body(r#"{"url":"https://pay.test/session"}"#)
            .create_async()
            .await;

        assert_eq!(
            c.checkout(&req("gold")).await.unwrap_err(),
            BillingClientError::UnknownPlan {
                plan: "gold".into()
            }
        );
        assert_eq!(
            c.checkout(&req("bad-email")).await.unwrap_err(),
            BillingClientError::BadRequest {
                message: "invalid email".into()
            }
        );
        assert_eq!(
            c.checkout(&req("nc")).await.unwrap(),
            CheckoutOutcome::NotConfigured {
                message: "polar checkout not configured".into()
            }
        );
        assert_eq!(
            c.checkout(&req("ok")).await.unwrap(),
            CheckoutOutcome::Url {
                url: "https://pay.test/session".into()
            }
        );
    }

    #[tokio::test]
    async fn usage_sends_bearer_and_parses_plan() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/v1/cloud/usage")
            .match_header("authorization", "Bearer tenant-key")
            .with_status(200)
            .with_body(
                r#"{"tenant":"acme","plan":"respaldo","bytes":1024,"chunks":2,"quota":5368709120,"ai_tokens_per_day":50000,"percent":0}"#,
            )
            .create_async()
            .await;
        let u = client(endpoints(
            "http://unused",
            &server.url(),
            Some("tenant-key"),
        ))
        .cloud_usage()
        .await
        .unwrap();
        m.assert_async().await;
        assert_eq!(u.tenant, "acme");
        assert_eq!(u.plan, Some(Some("respaldo".into())));
        assert_eq!(u.quota, 5_368_709_120);
        assert_eq!(u.ai_tokens_per_day, Some(50_000));
    }

    #[tokio::test]
    async fn usage_distinguishes_null_plan_from_missing_plan() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/v1/cloud/usage")
            .with_status(200)
            .with_body(r#"{"tenant":"a","plan":null,"bytes":0,"chunks":0,"quota":1000,"ai_tokens_per_day":100000,"percent":0}"#)
            .expect(1)
            .create_async()
            .await;
        let c = client(endpoints("http://unused", &server.url(), Some("k")));
        assert_eq!(c.cloud_usage().await.unwrap().plan, Some(None));

        let mut old = mockito::Server::new_async().await;
        let _o = old
            .mock("GET", "/v1/cloud/usage")
            .with_status(200)
            .with_body(r#"{"tenant":"a","bytes":10,"chunks":1,"quota":1000,"percent":1}"#)
            .create_async()
            .await;
        let u = client(endpoints("http://unused", &old.url(), Some("k")))
            .cloud_usage()
            .await
            .unwrap();
        assert_eq!(u.plan, None);
        assert_eq!(u.ai_tokens_per_day, None);
        // `--json` keeps "absent" absent instead of turning it into `null`.
        let out = serde_json::to_value(&u).unwrap();
        assert!(out.get("plan").is_none() && out.get("ai_tokens_per_day").is_none());
        let null_plan: CloudUsage =
            serde_json::from_str(r#"{"tenant":"a","plan":null,"bytes":0,"quota":1}"#).unwrap();
        assert_eq!(
            serde_json::to_value(&null_plan).unwrap()["plan"],
            serde_json::Value::Null
        );
    }

    #[tokio::test]
    async fn usage_without_token_does_not_call_the_server() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/v1/cloud/usage")
            .expect(0)
            .create_async()
            .await;
        let err = client(endpoints("http://unused", &server.url(), None))
            .cloud_usage()
            .await
            .unwrap_err();
        m.assert_async().await;
        assert_eq!(err, BillingClientError::MissingToken);
        assert!(err.to_string().contains("PGHEART_TOKEN"));
    }

    #[tokio::test]
    async fn usage_401_is_unauthenticated_and_404_is_wrong_server() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/v1/cloud/usage")
            .match_header("authorization", "Bearer bad")
            .with_status(401)
            .with_body(r#"{"error":"unauthorized"}"#)
            .create_async()
            .await;
        let err = client(endpoints("http://unused", &server.url(), Some("bad")))
            .cloud_usage()
            .await
            .unwrap_err();
        assert_eq!(
            err,
            BillingClientError::Unauthenticated {
                service: "xavier-cloud",
                status: 401
            }
        );
        assert!(err.to_string().contains("XAVIER_PGHEART_TOKEN"));

        let mut other = mockito::Server::new_async().await;
        let _n = other
            .mock("GET", "/v1/cloud/usage")
            .with_status(404)
            .create_async()
            .await;
        let err = client(endpoints("http://unused", &other.url(), Some("k")))
            .cloud_usage()
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            BillingClientError::NotFound {
                service: "xavier-cloud",
                ..
            }
        ));
    }
}
