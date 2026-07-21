// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Secrets store abstraction
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::secrets::SecretResult;
use std::future::Future;
use std::pin::Pin;

pub trait SecretStore: Send + Sync {
    fn get<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = SecretResult<String>> + Send + 'a>>;
    fn set<'a>(
        &'a self,
        key: &'a str,
        value: &'a str,
    ) -> Pin<Box<dyn Future<Output = SecretResult<()>> + Send + 'a>>;
    fn delete<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = SecretResult<()>> + Send + 'a>>;
}
