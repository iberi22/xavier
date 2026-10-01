//! Egress policy for requests that carry a node secret.
//!
//! Two independent guards live here. [`is_forbidden_egress_ip`] rejects
//! addresses that are never a legitimate secret destination, whatever name
//! resolved to them, and [`pinned_client`] pins one HTTP client to a single
//! vetted answer so the name cannot be re-resolved under the check.
//! [`SecretEgressPolicy`] binds each secret to the exact hosts it may reach.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use reqwest::Url;
use url::Host;

/// `fc00::/7`, unique-local. std has no stable predicate for it.
const IPV6_UNIQUE_LOCAL_MASK: u8 = 0xfe;
const IPV6_UNIQUE_LOCAL_PREFIX: u8 = 0xfc;
/// `fe80::/10`, link-local. `Ipv6Addr::is_unicast_link_local` is not stable.
const IPV6_LINK_LOCAL_MASK: u16 = 0xffc0;
const IPV6_LINK_LOCAL_PREFIX: u16 = 0xfe80;
/// Link-local metadata endpoint published by some cloud providers.
const CLOUD_METADATA_V4: [u8; 4] = [100, 100, 100, 200];

/// Public/multicast destinations a node secret must never reach.
///
/// Covers IPv4 loopback/unspecified/link-local/private/multicast/broadcast,
/// their IPv6 equivalents (loopback, unspecified, link-local, unique-local,
/// multicast) and any IPv4-mapped IPv6 address in those ranges.
pub fn is_forbidden_egress_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_unspecified()
                || v4.is_link_local() // 169.254.0.0/16
                || v4.is_private() // 10/8, 172.16/12, 192.168/16
                || v4.is_multicast() // 224.0.0.0/4
                || v4.is_broadcast()
                || v4.octets() == CLOUD_METADATA_V4
        }
        IpAddr::V6(v6) => {
            // `::ffff:127.0.0.1` must be judged as the IPv4 loopback it is.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_forbidden_egress_ip(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast() // ff00::/8
                || (first & IPV6_LINK_LOCAL_MASK) == IPV6_LINK_LOCAL_PREFIX
                || (v6.octets()[0] & IPV6_UNIQUE_LOCAL_MASK) == IPV6_UNIQUE_LOCAL_PREFIX
        }
    }
}

/// Redirect policy installed on every pinned client.
///
/// A 3xx would carry the secret headers to a host the policy never saw, so
/// redirects are refused rather than followed. Exposed as a named function so
/// the behaviour is assertable: `reqwest::Client` has no getter for it, and a
/// test that builds its own client proves nothing about this one.
#[must_use]
pub fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::none()
}

/// Builds a client pinned to `url`'s host, with redirects disabled.
///
/// Redirects are off because a 3xx would carry secret headers to a host the
/// policy never saw. DNS is resolved once here and the connection is pinned to
/// that answer, so a second, different answer cannot move the request.
pub async fn pinned_client(url: &Url) -> Result<reqwest::Client, String> {
    let port = url.port_or_known_default().ok_or("URL has no known port")?;
    let builder = reqwest::Client::builder().redirect(redirect_policy());

    match url.host().ok_or("URL has no host")? {
        // A literal address needs no resolver: vet it, then connect.
        Host::Ipv4(ip) => literal_client(builder, ip.into()),
        Host::Ipv6(ip) => literal_client(builder, ip.into()),
        Host::Domain(domain) => {
            // `host_str()` keeps the brackets of an IPv6 literal, so only a
            // domain goes to the resolver verbatim.
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((domain, port))
                .await
                .map_err(|e| format!("DNS resolution failed for {domain}: {e}"))?
                .collect();
            if addrs.is_empty() {
                return Err(format!("Host did not resolve: {domain}"));
            }
            if let Some(bad) = addrs.iter().find(|a| is_forbidden_egress_ip(a.ip())) {
                return Err(format!(
                    "Host {domain} resolves to a forbidden address: {}",
                    bad.ip()
                ));
            }
            builder
                .resolve(domain, addrs[0])
                .build()
                .map_err(|e| format!("HTTP client build failed for {domain}: {e}"))
        }
    }
}

/// Rejects a forbidden literal, then hands back a client for a vetted one.
fn literal_client(builder: reqwest::ClientBuilder, ip: IpAddr) -> Result<reqwest::Client, String> {
    if is_forbidden_egress_ip(ip) {
        return Err(format!("Forbidden destination address: {ip}"));
    }
    builder
        .build()
        .map_err(|e| format!("HTTP client build failed for {ip}: {e}"))
}

/// Which hosts a given secret is allowed to be sent to.
///
/// Format: `SECRET_NAME=host1,host2;OTHER=host3`. Hosts match exactly: no
/// wildcards, no suffix matching. A secret with no entry can never be injected
/// into a client-chosen destination.
#[derive(Debug, Clone, Default)]
pub struct SecretEgressPolicy {
    /// Lowercased secret name to the exact lowercase hosts bound to it.
    bindings: HashMap<String, Vec<String>>,
}

impl SecretEgressPolicy {
    /// Parses a `NAME=host[,host];...` spec. Malformed entries are skipped.
    pub fn parse(spec: &str) -> Self {
        let mut bindings: HashMap<String, Vec<String>> = HashMap::new();
        for entry in spec.split(';') {
            let Some((name, hosts)) = entry.split_once('=') else {
                continue;
            };
            let name = name.trim().to_ascii_lowercase();
            let hosts: Vec<String> = hosts
                .split(',')
                .map(|h| h.trim().to_ascii_lowercase())
                .filter(|h| !h.is_empty())
                .collect();
            if !name.is_empty() && !hosts.is_empty() {
                bindings.insert(name, hosts);
            }
        }
        Self { bindings }
    }

    /// Reads the policy from `XAVIER_SECRET_EGRESS`; unset means no bindings.
    pub fn from_env() -> Self {
        Self::parse(&std::env::var("XAVIER_SECRET_EGRESS").unwrap_or_default())
    }

    /// True when `secret_name` is bound to exactly `host`, compared
    /// case-insensitively. No binding means no injection.
    pub fn allows(&self, secret_name: &str, host: &str) -> bool {
        let Some(hosts) = self.bindings.get(&secret_name.to_ascii_lowercase()) else {
            return false;
        };
        let host = host.to_ascii_lowercase();
        hosts.contains(&host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_forbidden_category_is_rejected() {
        let forbidden = [
            "127.0.0.1",       // IPv4 loopback
            "::1",             // IPv6 loopback
            "0.0.0.0",         // IPv4 unspecified
            "::",              // IPv6 unspecified
            "169.254.169.254", // IPv4 link-local
            "fe80::1",         // IPv6 link-local
            "10.1.2.3",        // IPv4 private 10/8
            "172.20.0.1",      // IPv4 private 172.16/12
            "192.168.1.1",     // IPv4 private 192.168/16
            "fd00::1",         // IPv6 unique-local
            "224.0.0.1",       // IPv4 multicast
            "ff02::1",         // IPv6 multicast
            "255.255.255.255", // broadcast
        ];
        for ip in forbidden {
            assert!(is_forbidden_egress_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn public_addresses_are_allowed() {
        for ip in ["1.1.1.1", "8.8.8.8", "2001:4860:4860::8888"] {
            assert!(!is_forbidden_egress_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn ipv4_mapped_loopback_is_forbidden() {
        assert!(is_forbidden_egress_ip(
            "::ffff:127.0.0.1".parse::<IpAddr>().unwrap()
        ));
        assert!(is_forbidden_egress_ip(
            "::ffff:192.168.0.1".parse::<IpAddr>().unwrap()
        ));
        assert!(!is_forbidden_egress_ip(
            "::ffff:8.8.8.8".parse::<IpAddr>().unwrap()
        ));
    }

    #[test]
    fn parse_binds_only_the_named_secret_and_exact_host() {
        let policy = SecretEgressPolicy::parse("FOO=api.openai.com");
        assert!(policy.allows("FOO", "api.openai.com"));
        assert!(!policy.allows("BAR", "api.openai.com"));
        assert!(!policy.allows("FOO", "api.openai.com.evil.com"));
        assert!(!policy.allows("FOO", "evil-example.com"));
        assert!(policy.allows("foo", "API.OpenAI.com"));
    }

    #[test]
    fn unlisted_secret_with_empty_spec_is_denied() {
        let policy = SecretEgressPolicy::parse("");
        assert!(!policy.allows("FOO", "api.openai.com"));
        let whitespace = SecretEgressPolicy::parse("   \t  ");
        assert!(!whitespace.allows("FOO", "api.openai.com"));
        assert!(!SecretEgressPolicy::default().allows("FOO", "api.openai.com"));
    }

    #[tokio::test]
    async fn pinned_client_refuses_forbidden_literals() {
        for bad in ["http://127.0.0.1/", "http://[::1]/", "http://10.0.0.1/"] {
            let url = Url::parse(bad).unwrap();
            assert!(pinned_client(&url).await.is_err(), "{bad} must be refused");
        }
        let no_host = Url::parse("mailto:a@b.c").unwrap();
        assert!(pinned_client(&no_host).await.is_err());
    }

    #[tokio::test]
    async fn pinned_client_refuses_a_domain_that_resolves_into_a_forbidden_range() {
        // `localhost` resolves through the hosts file, so this needs no network.
        let url = Url::parse("http://localhost:8080/").unwrap();
        let err = pinned_client(&url).await.unwrap_err();
        assert!(err.contains("forbidden"), "{err}");
    }

    #[test]
    fn malformed_entries_bind_nothing() {
        let policy = SecretEgressPolicy::parse("NOHOSTS;=api.openai.com;FOO=");
        assert!(!policy.allows("FOO", "api.openai.com"));
        assert!(!policy.allows("NOHOSTS", "api.openai.com"));
    }

    /// Regression: removing the redirect guard from `pinned_client` left every
    /// other test in this module green.
    ///
    /// `reqwest::Client` exposes no getter for its redirect policy, so this is
    /// asserted against the named constant the client is actually built with.
    /// An earlier version of this test constructed its own client with an
    /// explicit `Policy::none()`, which proved nothing: mutating the one inside
    /// `pinned_client` still left it green.
    #[test]
    fn redirects_are_refused_not_followed() {
        // `Policy::none()` is the zero-redirect policy; anything that follows
        // redirects is not equivalent to it.
        assert_eq!(
            format!("{:?}", redirect_policy()),
            format!("{:?}", reqwest::redirect::Policy::none()),
            "the policy must refuse redirects entirely"
        );
        // And it must differ from the default, which follows up to 10 hops.
        assert_ne!(
            format!("{:?}", redirect_policy()),
            format!("{:?}", reqwest::redirect::Policy::default()),
            "a default policy would follow a 3xx to an unvetted host"
        );
    }

    /// The IPv6 bracket trap: `Url::host_str()` yields `[::1]` for a literal,
    /// which would poison both the resolver and the pin. Matching on `url::Host`
    /// avoids it, and the refusal must come from the address check.
    #[tokio::test]
    async fn pinned_client_refuses_a_bracketed_ipv6_literal() {
        let err = pinned_client(&Url::parse("http://[::1]:9000/").expect("url"))
            .await
            .expect_err("::1 must be refused");
        assert!(
            err.contains("Forbidden destination"),
            "expected the address to be refused as forbidden, got: {err}"
        );
    }
}
