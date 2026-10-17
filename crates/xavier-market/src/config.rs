//! Marketplace config keys and their v1 defaults.
//!
//! The key names are fixed here so the store and the HTTP adapter agree on
//! them. Values come from a caller-supplied lookup, so this module reads no
//! environment variables itself; the adapter maps each dotted key to an env var.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const KEY_CURRENCY: &str = "market.currency";
pub const KEY_SETTLEMENT: &str = "market.settlement";
pub const KEY_LICENSES: &str = "market.licenses";
pub const KEY_LEGAL_TEXT_STATUS: &str = "market.legal_text_status";
pub const KEY_PRICE_FLOOR: &str = "market.price_floor_credits";

/// How listings are paid for. Only internal credits are usable in v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Currency {
    /// Non-transferable credits earned by contributing data and spent on data.
    #[default]
    InternalCredits,
    /// USD through the merchant of record. Requires reviewed legal text.
    UsdPolar,
    /// `$SWAL` settlement. Needs the canonical-token ADR and a legal opinion.
    SwalToken,
}

impl Currency {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InternalCredits => "internal_credits",
            Self::UsdPolar => "usd_polar",
            Self::SwalToken => "swal_token",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ConfigError> {
        match value.trim() {
            "internal_credits" => Ok(Self::InternalCredits),
            "usd_polar" => Ok(Self::UsdPolar),
            "swal_token" => Ok(Self::SwalToken),
            other => Err(ConfigError::UnknownValue {
                key: KEY_CURRENCY,
                value: other.to_string(),
            }),
        }
    }
}

/// Where balances and settlements are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Settlement {
    #[default]
    OffChain,
    /// Not implemented in v1; rejected by [`MarketConfig::validate`].
    OnChain,
}

impl Settlement {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OffChain => "off_chain",
            Self::OnChain => "on_chain",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ConfigError> {
        match value.trim() {
            "off_chain" => Ok(Self::OffChain),
            "on_chain" => Ok(Self::OnChain),
            other => Err(ConfigError::UnknownValue {
                key: KEY_SETTLEMENT,
                value: other.to_string(),
            }),
        }
    }
}

/// The fixed license menu. Free-text licenses are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum License {
    #[serde(rename = "CC-BY-4.0")]
    CcBy40,
    #[serde(rename = "CC-BY-NC-4.0")]
    CcByNc40,
    #[serde(rename = "SWAL Data License v0")]
    SwalDataLicenseV0,
}

impl License {
    /// Every license on the fixed menu, in ADR order.
    pub const MENU: [License; 3] = [
        License::CcBy40,
        License::CcByNc40,
        License::SwalDataLicenseV0,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CcBy40 => "CC-BY-4.0",
            Self::CcByNc40 => "CC-BY-NC-4.0",
            Self::SwalDataLicenseV0 => "SWAL Data License v0",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ConfigError> {
        let value = value.trim();
        Self::MENU
            .into_iter()
            .find(|license| license.as_str() == value)
            .ok_or_else(|| ConfigError::UnknownLicense(value.to_string()))
    }
}

/// Whether the legal text of the SWAL license and seller warranty has been reviewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LegalTextStatus {
    #[default]
    Placeholder,
    Reviewed,
}

impl LegalTextStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Placeholder => "placeholder",
            Self::Reviewed => "reviewed",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ConfigError> {
        match value.trim() {
            "placeholder" => Ok(Self::Placeholder),
            "reviewed" => Ok(Self::Reviewed),
            other => Err(ConfigError::UnknownValue {
                key: KEY_LEGAL_TEXT_STATUS,
                value: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("unknown value {value:?} for {key}")]
    UnknownValue { key: &'static str, value: String },
    #[error("unknown license {0:?}: only the fixed menu is accepted")]
    UnknownLicense(String),
    #[error("market.licenses must list at least one license")]
    NoLicenses,
    #[error("market.licenses lists {0} more than once")]
    DuplicateLicense(&'static str),
    #[error("market.settlement=on_chain is not implemented in v1")]
    OnChainNotImplemented,
    #[error("market.currency=swal_token needs the canonical-token ADR and a legal opinion")]
    SwalTokenNotEnabled,
    #[error("market.currency=usd_polar needs market.legal_text_status=reviewed")]
    UsdRequiresReviewedTerms,
}

/// Validated marketplace configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketConfig {
    pub currency: Currency,
    pub settlement: Settlement,
    pub licenses: Vec<License>,
    pub legal_text_status: LegalTextStatus,
    pub price_floor_credits: u64,
}

impl Default for MarketConfig {
    /// The v1 defaults: internal credits, off-chain, the full license menu,
    /// placeholder legal text.
    fn default() -> Self {
        Self {
            currency: Currency::InternalCredits,
            settlement: Settlement::OffChain,
            licenses: License::MENU.to_vec(),
            legal_text_status: LegalTextStatus::Placeholder,
            price_floor_credits: 0,
        }
    }
}

impl MarketConfig {
    /// Builds the config from a lookup of dotted keys (`market.currency`, ...).
    /// A key the lookup does not return keeps its default. A returned value
    /// that does not parse is an error, never a silent default.
    pub fn from_lookup<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let mut config = Self::default();
        if let Some(value) = lookup(KEY_CURRENCY) {
            config.currency = Currency::parse(&value)?;
        }
        if let Some(value) = lookup(KEY_SETTLEMENT) {
            config.settlement = Settlement::parse(&value)?;
        }
        if let Some(value) = lookup(KEY_LICENSES) {
            config.licenses = value
                .split(',')
                .filter(|item| !item.trim().is_empty())
                .map(License::parse)
                .collect::<Result<Vec<_>, _>>()?;
        }
        if let Some(value) = lookup(KEY_LEGAL_TEXT_STATUS) {
            config.legal_text_status = LegalTextStatus::parse(&value)?;
        }
        if let Some(value) = lookup(KEY_PRICE_FLOOR) {
            config.price_floor_credits =
                value
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| ConfigError::UnknownValue {
                        key: KEY_PRICE_FLOOR,
                        value: value.clone(),
                    })?;
        }
        config.validate()?;
        Ok(config)
    }

    /// Rejects combinations that v1 does not allow.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.settlement == Settlement::OnChain {
            return Err(ConfigError::OnChainNotImplemented);
        }
        match self.currency {
            Currency::SwalToken => return Err(ConfigError::SwalTokenNotEnabled),
            Currency::UsdPolar if self.legal_text_status != LegalTextStatus::Reviewed => {
                return Err(ConfigError::UsdRequiresReviewedTerms)
            }
            _ => {}
        }
        if self.licenses.is_empty() {
            return Err(ConfigError::NoLicenses);
        }
        for (index, license) in self.licenses.iter().enumerate() {
            if self.licenses[..index].contains(license) {
                return Err(ConfigError::DuplicateLicense(license.as_str()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn v1_defaults_are_internal_credits_off_chain_full_menu() {
        let config = MarketConfig::from_lookup(|_| None).unwrap();
        assert_eq!(config, MarketConfig::default());
        assert_eq!(config.currency.as_str(), "internal_credits");
        assert_eq!(config.settlement.as_str(), "off_chain");
        assert_eq!(config.legal_text_status.as_str(), "placeholder");
        assert_eq!(config.price_floor_credits, 0);
        assert_eq!(
            config.licenses,
            vec![
                License::CcBy40,
                License::CcByNc40,
                License::SwalDataLicenseV0
            ]
        );
    }

    #[test]
    fn license_strings_round_trip_through_serde() {
        for license in License::MENU {
            let json = serde_json::to_string(&license).unwrap();
            assert_eq!(json, format!("\"{}\"", license.as_str()));
            assert_eq!(License::parse(license.as_str()).unwrap(), license);
        }
    }

    #[test]
    fn free_text_license_is_rejected() {
        assert_eq!(
            License::parse("MIT"),
            Err(ConfigError::UnknownLicense("MIT".to_string()))
        );
        let err = MarketConfig::from_lookup(lookup(&[(KEY_LICENSES, "CC-BY-4.0,MIT")]));
        assert_eq!(err, Err(ConfigError::UnknownLicense("MIT".to_string())));
    }

    #[test]
    fn licenses_key_accepts_a_subset_of_the_menu() {
        let config = MarketConfig::from_lookup(lookup(&[(
            KEY_LICENSES,
            " CC-BY-4.0 , SWAL Data License v0 ",
        )]))
        .unwrap();
        assert_eq!(
            config.licenses,
            vec![License::CcBy40, License::SwalDataLicenseV0]
        );
    }

    #[test]
    fn empty_and_duplicate_licenses_are_rejected() {
        assert_eq!(
            MarketConfig::from_lookup(lookup(&[(KEY_LICENSES, " , ")])),
            Err(ConfigError::NoLicenses)
        );
        assert_eq!(
            MarketConfig::from_lookup(lookup(&[(KEY_LICENSES, "CC-BY-4.0,CC-BY-4.0")])),
            Err(ConfigError::DuplicateLicense("CC-BY-4.0"))
        );
    }

    #[test]
    fn on_chain_settlement_is_rejected() {
        assert_eq!(
            MarketConfig::from_lookup(lookup(&[(KEY_SETTLEMENT, "on_chain")])),
            Err(ConfigError::OnChainNotImplemented)
        );
        assert!(MarketConfig::from_lookup(lookup(&[(KEY_SETTLEMENT, "off_chain")])).is_ok());
    }

    #[test]
    fn swal_token_currency_is_not_enabled_in_v1() {
        assert_eq!(
            MarketConfig::from_lookup(lookup(&[(KEY_CURRENCY, "swal_token")])),
            Err(ConfigError::SwalTokenNotEnabled)
        );
    }

    #[test]
    fn usd_needs_reviewed_legal_text() {
        assert_eq!(
            MarketConfig::from_lookup(lookup(&[(KEY_CURRENCY, "usd_polar")])),
            Err(ConfigError::UsdRequiresReviewedTerms)
        );
        let config = MarketConfig::from_lookup(lookup(&[
            (KEY_CURRENCY, "usd_polar"),
            (KEY_LEGAL_TEXT_STATUS, "reviewed"),
        ]))
        .unwrap();
        assert_eq!(config.currency, Currency::UsdPolar);
    }

    #[test]
    fn unparseable_value_is_an_error_not_a_default() {
        assert!(matches!(
            MarketConfig::from_lookup(lookup(&[(KEY_CURRENCY, "bitcoin")])),
            Err(ConfigError::UnknownValue {
                key: KEY_CURRENCY,
                ..
            })
        ));
    }

    #[test]
    fn unparseable_price_floor_is_an_error() {
        assert!(matches!(
            MarketConfig::from_lookup(lookup(&[(KEY_PRICE_FLOOR, "abc")])),
            Err(ConfigError::UnknownValue {
                key: KEY_PRICE_FLOOR,
                ..
            })
        ));
    }
}
