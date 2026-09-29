//! PDF ingestion (feature-gated).

#[cfg(feature = "pdf-outline")]
pub mod cascade;
#[cfg(feature = "pdf-layout")]
pub mod layout;
#[cfg(feature = "pdf-outline")]
pub mod outline;
#[cfg(feature = "pdf-layout")]
pub mod pdfium_loader;
