use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, debug};
use notify::{Watcher, RecommendedWatcher, RecursiveMode, Event, Config};
use crate::error::{GraphError, Result};
use crate::indexer::Indexer;

pub struct AutoSyncWatcher {
    indexer: Arc<Indexer>,
    root: PathBuf,
}

impl AutoSyncWatcher {
    pub fn new(indexer: Arc<Indexer>, root: PathBuf) -> Self {
        Self { indexer, root }
    }

    /// Starts watching the file system and incrementally syncs the graph.
    pub async fn watch(&self) -> Result<()> {
        let (tx, mut rx) = mpsc::channel(100);

        let mut watcher = RecommendedWatcher::new(
            move |res: std::result::Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.blocking_send(event);
                }
            },
            Config::default(),
        )
        .map_err(|e| GraphError::Indexer(format!("Failed to init watcher: {}", e)))?;

        watcher
            .watch(&self.root, RecursiveMode::Recursive)
            .map_err(|e| GraphError::Indexer(format!("Failed to watch dir: {}", e)))?;

        info!("Auto-Sync Watcher active on {:?}", self.root);

        // Simple debounce loop
        while let Some(event) = rx.recv().await {
            match event.kind {
                notify::EventKind::Modify(_) | notify::EventKind::Create(_) | notify::EventKind::Remove(_) => {
                    for path in event.paths {
                        // TODO: Implement fine-grained incremental update. 
                        // For now, we log the path to be updated. The actual incremental logic
                        // requires purging the old file's symbols and edges from DB, and re-parsing.
                        debug!("File changed, queuing incremental sync: {:?}", path);
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }
}
