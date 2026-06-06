//! Outbound port for schema initialization
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
pub trait SchemaInitializer: Send + Sync {
    fn init_schema(&self) -> anyhow::Result<()>;
}
