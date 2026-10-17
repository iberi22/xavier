use thiserror::Error;

pub const MIN_K: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privacy {
    P0,
    P1,
    P2,
    P3,
    P4,
}

impl Privacy {
    pub fn is_local_only(self) -> bool {
        matches!(self, Self::P4)
    }
}

pub struct ListingPrivacyReport {
    pub level: Privacy,
    pub residual_pii_total: usize,
    pub k: usize,
    pub k_satisfied: bool,
    pub consent_ref: Option<String>,
}

#[derive(Debug, Error)]
pub enum AdmissionError {
    #[error("level P0 and P1 are not accepted; must be P2 or above")]
    BelowP2,
    #[error("level P4 is local-only and cannot be admitted")]
    LocalOnly,
    #[error("listing contains {0} residual PII items")]
    ResidualPii(usize),
    #[error("k-anonymity level {k} is too low, minimum is {min}")]
    KTooLow { k: usize, min: usize },
    #[error("k-anonymity equivalence class check not satisfied")]
    KNotSatisfied,
    #[error("missing or blank consent reference")]
    MissingConsent,
}

pub fn admit(report: &ListingPrivacyReport) -> Result<(), AdmissionError> {
    if report.level.is_local_only() {
        return Err(AdmissionError::LocalOnly);
    }
    if matches!(report.level, Privacy::P0 | Privacy::P1) {
        return Err(AdmissionError::BelowP2);
    }
    if report.residual_pii_total > 0 {
        return Err(AdmissionError::ResidualPii(report.residual_pii_total));
    }
    if report.k < MIN_K {
        return Err(AdmissionError::KTooLow {
            k: report.k,
            min: MIN_K,
        });
    }
    if !report.k_satisfied {
        return Err(AdmissionError::KNotSatisfied);
    }
    match &report.consent_ref {
        Some(c) if !c.trim().is_empty() => {}
        _ => return Err(AdmissionError::MissingConsent),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_report(level: Privacy) -> ListingPrivacyReport {
        ListingPrivacyReport {
            level,
            residual_pii_total: 0,
            k: 5,
            k_satisfied: true,
            consent_ref: Some("consent_123".to_string()),
        }
    }

    #[test]
    fn test_admit_p2() {
        let report = valid_report(Privacy::P2);
        assert!(admit(&report).is_ok());
    }

    #[test]
    fn test_admit_p3() {
        let report = valid_report(Privacy::P3);
        assert!(admit(&report).is_ok());
    }

    #[test]
    fn test_refuse_local_only() {
        let report = valid_report(Privacy::P4);
        assert!(matches!(admit(&report), Err(AdmissionError::LocalOnly)));
    }

    #[test]
    fn test_refuse_below_p2() {
        let report = valid_report(Privacy::P0);
        assert!(matches!(admit(&report), Err(AdmissionError::BelowP2)));
        let report = valid_report(Privacy::P1);
        assert!(matches!(admit(&report), Err(AdmissionError::BelowP2)));
    }

    #[test]
    fn test_refuse_residual_pii() {
        let mut report = valid_report(Privacy::P2);
        report.residual_pii_total = 1;
        assert!(matches!(
            admit(&report),
            Err(AdmissionError::ResidualPii(1))
        ));
    }

    #[test]
    fn test_refuse_k_too_low() {
        let mut report = valid_report(Privacy::P2);
        report.k = 4;
        assert!(matches!(
            admit(&report),
            Err(AdmissionError::KTooLow { k: 4, min: 5 })
        ));
    }

    #[test]
    fn test_refuse_k_not_satisfied() {
        let mut report = valid_report(Privacy::P2);
        report.k_satisfied = false;
        assert!(matches!(admit(&report), Err(AdmissionError::KNotSatisfied)));
    }

    #[test]
    fn test_refuse_missing_consent() {
        let mut report = valid_report(Privacy::P2);
        report.consent_ref = None;
        assert!(matches!(
            admit(&report),
            Err(AdmissionError::MissingConsent)
        ));
    }

    #[test]
    fn test_refuse_blank_consent() {
        let mut report = valid_report(Privacy::P2);
        report.consent_ref = Some("   ".to_string());
        assert!(matches!(
            admit(&report),
            Err(AdmissionError::MissingConsent)
        ));
    }
}
