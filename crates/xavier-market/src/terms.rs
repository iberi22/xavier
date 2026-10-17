use crate::config::{LegalTextStatus, License};
use crate::model::Receipt;
use sha2::{Digest, Sha256};

pub const MAX_TIME_LIMIT_DAYS: u32 = 3650;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TermsError {
    #[error("purpose cannot be blank")]
    BlankPurpose,
    #[error("time limit {0} days is out of bounds (1..=3650)")]
    TimeLimit(u32),
    #[error("terms were not accepted or the acceptance hash is invalid")]
    NotAccepted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenseTerms {
    pub license: License,
    pub purpose: String,
    pub time_limit_days: u32,
    pub legal_text_status: LegalTextStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermsAcceptance {
    pub buyer_id: String,
    pub accepted_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermsBoundReceipt {
    pub receipt: Receipt,
    pub terms_hash: String,
}

pub fn new_terms(
    license: License,
    purpose: &str,
    time_limit_days: u32,
    status: LegalTextStatus,
) -> Result<LicenseTerms, TermsError> {
    let purpose = purpose.trim();
    if purpose.is_empty() {
        return Err(TermsError::BlankPurpose);
    }
    if time_limit_days == 0 || time_limit_days > MAX_TIME_LIMIT_DAYS {
        return Err(TermsError::TimeLimit(time_limit_days));
    }
    Ok(LicenseTerms {
        license,
        purpose: purpose.to_string(),
        time_limit_days,
        legal_text_status: status,
    })
}

pub fn terms_hash(terms: &LicenseTerms) -> String {
    let payload = format!(
        "swal-license-terms-v1:{}|{}|{}|{}",
        terms.license.as_str(),
        terms.purpose,
        terms.time_limit_days,
        terms.legal_text_status.as_str()
    );
    hex::encode(Sha256::digest(payload.as_bytes()))
}

pub fn require_acceptance(
    terms: &LicenseTerms,
    accepted: Option<&TermsAcceptance>,
) -> Result<(), TermsError> {
    let acceptance = accepted.ok_or(TermsError::NotAccepted)?;
    if acceptance.buyer_id.trim().is_empty() {
        return Err(TermsError::NotAccepted);
    }
    if acceptance.accepted_hash != terms_hash(terms) {
        return Err(TermsError::NotAccepted);
    }
    Ok(())
}

pub fn bind(receipt: Receipt, terms: &LicenseTerms) -> TermsBoundReceipt {
    TermsBoundReceipt {
        receipt,
        terms_hash: terms_hash(terms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Currency, LegalTextStatus, License};

    fn base_terms() -> LicenseTerms {
        new_terms(
            License::CcByNc40,
            "Research and analysis",
            30,
            LegalTextStatus::Placeholder,
        )
        .unwrap()
    }

    fn base_receipt() -> Receipt {
        Receipt {
            id: "r1".to_string(),
            listing_id: "l1".to_string(),
            buyer_id: "b1".to_string(),
            amount_credits: 100,
            currency: Currency::InternalCredits,
            created_at_ms: 123456789,
        }
    }

    #[test]
    fn deterministic_hash() {
        let terms = base_terms();
        let hash = terms_hash(&terms);
        assert_eq!(hash, terms_hash(&terms));
    }

    #[test]
    fn different_purpose_changes_hash() {
        let terms1 = base_terms();
        let mut terms2 = base_terms();
        terms2.purpose = "Commercial usage".to_string();
        assert_ne!(terms_hash(&terms1), terms_hash(&terms2));
    }

    #[test]
    fn different_status_changes_hash() {
        let terms1 = base_terms();
        let mut terms2 = base_terms();
        terms2.legal_text_status = LegalTextStatus::Reviewed;
        assert_ne!(terms_hash(&terms1), terms_hash(&terms2));
    }

    #[test]
    fn no_acceptance_is_refused() {
        let terms = base_terms();
        assert_eq!(
            require_acceptance(&terms, None),
            Err(TermsError::NotAccepted)
        );
    }

    #[test]
    fn wrong_hash_is_refused() {
        let terms = base_terms();
        let acc = TermsAcceptance {
            buyer_id: "b1".to_string(),
            accepted_hash: "wrong_hash".to_string(),
        };
        assert_eq!(
            require_acceptance(&terms, Some(&acc)),
            Err(TermsError::NotAccepted)
        );
    }

    #[test]
    fn correct_acceptance_passes() {
        let terms = base_terms();
        let hash = terms_hash(&terms);
        let acc = TermsAcceptance {
            buyer_id: "b1".to_string(),
            accepted_hash: hash,
        };
        assert_eq!(require_acceptance(&terms, Some(&acc)), Ok(()));
    }

    #[test]
    fn bind_puts_terms_hash_on_receipt() {
        let terms = base_terms();
        let receipt = base_receipt();
        let bound = bind(receipt.clone(), &terms);
        assert_eq!(bound.receipt, receipt);
        assert_eq!(bound.terms_hash, terms_hash(&terms));
    }

    #[test]
    fn time_limit_zero_is_refused() {
        let err = new_terms(
            License::CcByNc40,
            "Research",
            0,
            LegalTextStatus::Placeholder,
        );
        assert_eq!(err, Err(TermsError::TimeLimit(0)));
    }
}
