//! Async libSQL connection pool
//!
//! A lightweight, high-performance, fully asynchronous connection pool
//! for libSQL database connections. Reduces network/resource overhead in
//! highly concurrent agent environments.

use anyhow::{anyhow, Context, Result};
use libsql::{Connection, Database};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Configuration for the libSQL connection pool
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections in the pool
    pub max_size: usize,
    /// Connection timeout
    pub connection_timeout: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_size: 16,
            connection_timeout: Duration::from_secs(30),
        }
    }
}

/// A highly-optimized, fully asynchronous connection pool for libSQL.
#[deprecated(note = "LibsqlConnectionPool is deprecated and will be replaced by a unified connection manager.")]
#[derive(Clone)]
pub struct LibsqlConnectionPool {
    inner: Arc<PoolInner>,
}

struct PoolInner {
    database: Database,
    connections: Mutex<VecDeque<Connection>>,
    semaphore: Arc<Semaphore>,
    config: PoolConfig,
}

/// A smart pointer wrapper that returns the connection to the pool when dropped.
pub struct PooledConnection {
    conn: Option<Connection>,
    pool: LibsqlConnectionPool,
    _permit: OwnedSemaphorePermit,
}

impl LibsqlConnectionPool {
    /// Create a new connection pool with a shared libSQL database instance
    pub fn new(database: Database, config: PoolConfig) -> Self {
        let max_size = config.max_size;
        Self {
            inner: Arc::new(PoolInner {
                database,
                connections: Mutex::new(VecDeque::with_capacity(max_size)),
                semaphore: Arc::new(Semaphore::new(max_size)),
                config,
            }),
        }
    }

    /// Acquire a connection from the pool asynchronously.
    /// If no connections are available and max_size is reached, it will wait.
    pub async fn get(&self) -> Result<PooledConnection> {
        let timeout_dur = self.inner.config.connection_timeout;

        let res = tokio::time::timeout(timeout_dur, async {
            // Acquire permit from the semaphore, which naturally handles limits and queuing.
            let permit = self
                .inner
                .semaphore
                .clone()
                .acquire_owned()
                .await
                .context("failed to acquire connection pool permit")?;

            // Try to pop an idle connection and validate it
            let mut conn = None;
            while let Some(c) = {
                let mut conns = self.inner.connections.lock();
                conns.pop_front()
            } {
                // Liveness probe: run a quick lightweight query to check if connection is active
                if c.query("SELECT 1", ()).await.is_ok() {
                    conn = Some(c);
                    break;
                }
                // If invalid/stale, it gets dropped here and we pop the next one.
            }

            let conn = match conn {
                Some(c) => c,
                None => {
                    // Establish a new connection if none were idle or all were stale
                    let new_conn = self
                        .inner
                        .database
                        .connect()
                        .context("failed to establish a new pool connection")?;

                    // Apply WAL and performance optimizations
                    new_conn
                        .execute_batch(
                            "PRAGMA journal_mode=WAL; \
                             PRAGMA synchronous=NORMAL; \
                             PRAGMA cache_size=-32768; \
                             PRAGMA temp_store=MEMORY; \
                             PRAGMA foreign_keys=ON;",
                        )
                        .await
                        .context("failed to optimize new pooled connection")?;

                    new_conn
                }
            };

            Ok::<(Connection, OwnedSemaphorePermit), anyhow::Error>((conn, permit))
        })
        .await;

        match res {
            Ok(inner_res) => {
                let (conn, permit) = inner_res?;
                Ok(PooledConnection {
                    conn: Some(conn),
                    pool: self.clone(),
                    _permit: permit,
                })
            }
            Err(_) => Err(anyhow!(
                "connection pool acquisition timed out after {:?}",
                timeout_dur
            )),
        }
    }

    /// Return an active connection to the pool's idle queue
    fn return_connection(&self, conn: Connection) {
        let mut conns = self.inner.connections.lock();
        conns.push_back(conn);
    }

    /// Get the number of active (borrowed) connections
    pub fn active_connections(&self) -> usize {
        let max_size = self.inner.config.max_size;
        let available = self.inner.semaphore.available_permits();
        max_size.saturating_sub(available)
    }

    /// Get the number of idle (cached) connections
    pub fn idle_connections(&self) -> usize {
        let conns = self.inner.connections.lock();
        conns.len()
    }
}

impl Deref for PooledConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.conn.as_ref().expect("connection was already dropped")
    }
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.return_connection(conn);
        }
    }
}
