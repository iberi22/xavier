//! Shared, lazily opened PageIndex instance.

use std::sync::{Arc, Mutex, OnceLock};

use xavier_pageindex::store::SqliteStore;
use xavier_pageindex::{PageIndex, PageIndexConfig, PageIndexError};

use super::settings::PageIndexSettings;

pub type SharedIndex = Arc<PageIndex<SqliteStore>>;

pub struct PageIndexState {
    pub settings: PageIndexSettings,
    index: Mutex<Option<SharedIndex>>,
}

pub(crate) fn config_for(settings: &PageIndexSettings) -> PageIndexConfig {
    PageIndexConfig {
        structure_char_budget: settings.max_response_chars,
        response_char_cap: settings.max_response_chars,
    }
}

impl PageIndexState {
    /// The store is opened on first use, not at construction.
    pub fn new(settings: PageIndexSettings) -> Self {
        Self {
            settings,
            index: Mutex::new(None),
        }
    }

    /// Wrap an already built index (used by tests with in-memory stores).
    pub fn with_store(settings: PageIndexSettings, store: SqliteStore) -> Self {
        let idx = Arc::new(PageIndex::with_config(store, config_for(&settings)));
        Self {
            settings,
            index: Mutex::new(Some(idx)),
        }
    }

    /// Blocking: may open the database. Call from `spawn_blocking`.
    pub fn index(&self) -> Result<SharedIndex, PageIndexError> {
        let mut guard = self
            .index
            .lock()
            .map_err(|_| PageIndexError::Store("pageindex state lock poisoned".into()))?;
        if let Some(idx) = guard.as_ref() {
            return Ok(Arc::clone(idx));
        }
        if let Some(parent) = self
            .settings
            .db_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| PageIndexError::Store(format!("create {}: {e}", parent.display())))?;
        }
        let store = SqliteStore::open(&self.settings.db_path)?;
        let idx = Arc::new(PageIndex::with_config(store, config_for(&self.settings)));
        *guard = Some(Arc::clone(&idx));
        Ok(idx)
    }
}

static SHARED_STATE: OnceLock<Arc<PageIndexState>> = OnceLock::new();

/// Process-wide PageIndex state shared by the HTTP routes and the MCP tools,
/// so both see one store. The database opens lazily on first use.
pub fn shared_state() -> Arc<PageIndexState> {
    SHARED_STATE
        .get_or_init(|| Arc::new(PageIndexState::new(PageIndexSettings::from_env())))
        .clone()
}

/// Install the shared state before first use (tests). Returns false if one
/// was already installed.
pub fn install_shared_state(state: PageIndexState) -> bool {
    SHARED_STATE.set(Arc::new(state)).is_ok()
}
