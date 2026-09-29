//! Error type for the crate.

#[derive(Debug, thiserror::Error)]
pub enum PageIndexError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid range: {0}")]
    InvalidRange(String),
    #[error("feature disabled: {0}")]
    FeatureDisabled(&'static str),
    #[error("pdfium unavailable: {0}")]
    PdfiumUnavailable(String),
    #[error("document is encrypted")]
    Encrypted,
    #[error("store error: {0}")]
    Store(String),
    #[error("build error: {0}")]
    Build(String),
}
