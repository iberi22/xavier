use crate::error::GraphError;
use crate::indexer::Indexer;
use notify_debouncer_mini::new_debouncer;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};

pub struct DebouncedWatcher {
    indexer: Arc<Indexer>,
    root: PathBuf,
    debounce_ms: u64,
}

impl DebouncedWatcher {
    pub fn new(indexer: Arc<Indexer>, root: PathBuf) -> Self {
        Self {
            indexer,
            root,
            debounce_ms: 500,
        }
    }

    pub fn with_debounce(mut self, ms: u64) -> Self {
        self.debounce_ms = ms;
        self
    }

    pub async fn watch(&self) -> crate::error::Result<()> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        let tx_clone = tx.clone();

        let mut debouncer = new_debouncer(
            Duration::from_millis(self.debounce_ms),
            move |result: std::result::Result<Vec<notify_debouncer_mini::DebouncedEvent>, _>| {
                if let Ok(events) = result {
                    for event in events {
                        let _ = tx_clone.try_send(event.path);
                    }
                }
            },
        )
        .map_err(|e| GraphError::Indexer(format!("Failed to init debouncer: {}", e)))?;

        debouncer
            .watcher()
            .watch(&self.root, notify_debouncer_mini::notify::RecursiveMode::Recursive)
            .map_err(|e| GraphError::Indexer(format!("Failed to watch dir: {}", e)))?;

        info!(
            "DebouncedWatcher active on {:?} (debounce: {}ms)",
            self.root, self.debounce_ms
        );

        while let Some(path) = rx.recv().await {
            debug!("File changed, re-indexing: {:?}", path);
            if let Err(e) = self.indexer.reindex_file(&self.root, &path).await {
                error!("Failed to re-index {:?}: {}", path, e);
            }
        }
        Ok(())
    }
}
