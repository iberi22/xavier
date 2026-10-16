//! Connection provider trait and implementations for database access abstraction.

use anyhow::Result;
use async_trait::async_trait;
use rusqlite::Connection;
use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

use crate::codebase::connection_manager::{ConnectionManager, PoolClass, PoolProfile};

/// Abstract connection provider trait to decouple storage implementations from ConnectionManager globals.
#[async_trait]
pub trait ConnectionProvider: Send + Sync {
    /// Connect to a database by project_id and project_root.
    fn connect(&self, project_id: &str, project_root: &str) -> Result<()>;

    /// Connect to a database with an explicit file path.
    fn connect_with_path(&self, project_id: &str, db_path: PathBuf) -> Result<()>;

    /// Connect to a database file with an explicit pool profile. Per-repo
    /// memory stores use [`PoolProfile::RepoStore`] so they do not each open
    /// the daemon-wide 10-connection pool.
    ///
    /// The default ignores the profile and falls back to
    /// [`ConnectionProvider::connect_with_path`]; providers that own a
    /// [`ConnectionManager`] override it.
    fn connect_with_path_profile(
        &self,
        project_id: &str,
        db_path: PathBuf,
        profile: PoolProfile,
    ) -> Result<()> {
        let _ = profile;
        self.connect_with_path(project_id, db_path)
    }

    /// Internal type-erased closure execution on a database connection.
    async fn execute_with_conn(
        &self,
        project_id: &str,
        f: Box<dyn for<'a> FnOnce(&'a Connection) -> Result<Box<dyn Any + Send>> + Send + 'static>,
    ) -> Result<Box<dyn Any + Send>>;
}

impl dyn ConnectionProvider {
    /// Execute a closure using a connection from a specific project pool.
    pub async fn with_conn<F, T>(&self, project_id: &str, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let boxed_f = Box::new(move |conn: &Connection| -> Result<Box<dyn Any + Send>> {
            let res = f(conn)?;
            Ok(Box::new(res))
        });
        let boxed_res = self.execute_with_conn(project_id, boxed_f).await?;
        boxed_res
            .downcast::<T>()
            .map(|b| *b)
            .map_err(|_| anyhow::anyhow!("ConnectionProvider downcast failed"))
    }
}

/// Production connection provider delegating to `ConnectionManager::global()`.
#[derive(Debug, Default, Clone)]
pub struct GlobalConnectionProvider;

impl GlobalConnectionProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectionProvider for GlobalConnectionProvider {
    fn connect(&self, project_id: &str, project_root: &str) -> Result<()> {
        ConnectionManager::global().connect(project_id, project_root)
    }

    fn connect_with_path(&self, project_id: &str, db_path: PathBuf) -> Result<()> {
        ConnectionManager::global().connect_with_path(project_id, db_path)
    }

    fn connect_with_path_profile(
        &self,
        project_id: &str,
        db_path: PathBuf,
        profile: PoolProfile,
    ) -> Result<()> {
        ConnectionManager::global().connect_with_path_profile(
            project_id,
            db_path,
            profile,
            PoolClass::Evictable,
        )
    }

    async fn execute_with_conn(
        &self,
        project_id: &str,
        f: Box<dyn for<'a> FnOnce(&'a Connection) -> Result<Box<dyn Any + Send>> + Send + 'static>,
    ) -> Result<Box<dyn Any + Send>> {
        ConnectionManager::global().with_conn(project_id, f).await
    }
}

/// In-memory / isolated connection provider for tests to prevent test pollution.
#[derive(Clone)]
pub struct InMemoryProvider {
    inner: Arc<ConnectionManager>,
}

impl Default for InMemoryProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryProvider {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ConnectionManager::new()),
        }
    }
}

#[async_trait]
impl ConnectionProvider for InMemoryProvider {
    fn connect(&self, project_id: &str, project_root: &str) -> Result<()> {
        self.inner.connect(project_id, project_root)
    }

    fn connect_with_path(&self, project_id: &str, db_path: PathBuf) -> Result<()> {
        self.inner.connect_with_path(project_id, db_path)
    }

    fn connect_with_path_profile(
        &self,
        project_id: &str,
        db_path: PathBuf,
        profile: PoolProfile,
    ) -> Result<()> {
        self.inner
            .connect_with_path_profile(project_id, db_path, profile, PoolClass::Evictable)
    }

    async fn execute_with_conn(
        &self,
        project_id: &str,
        f: Box<dyn for<'a> FnOnce(&'a Connection) -> Result<Box<dyn Any + Send>> + Send + 'static>,
    ) -> Result<Box<dyn Any + Send>> {
        self.inner.with_conn(project_id, f).await
    }
}
