//! File system watching for live vault updates.
//!
//! This module provides file system watching functionality using the `notify` crate.
//! It watches the vault directory for changes and emits events that can be used
//! to trigger incremental index updates.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebouncedEvent, Debouncer, new_debouncer};
use thiserror::Error;
use tokio::sync::broadcast;

/// Errors that can occur during watcher operations.
#[derive(Debug, Error)]
pub enum WatcherError {
    #[error("Failed to create watcher: {0}")]
    CreateError(#[from] notify::Error),

    #[error("Failed to watch path: {path}")]
    WatchPathError {
        path: PathBuf,
        #[source]
        source: notify::Error,
    },

    #[error("Watcher channel closed")]
    ChannelClosed,
}

/// Types of vault changes detected by the file watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultChange {
    /// A new file was created.
    Created(PathBuf),
    /// An existing file was modified.
    Modified(PathBuf),
    /// A file was removed.
    Removed(PathBuf),
}

impl VaultChange {
    /// Get the path associated with this change.
    pub fn path(&self) -> &Path {
        match self {
            VaultChange::Created(p) | VaultChange::Modified(p) | VaultChange::Removed(p) => p,
        }
    }

    /// Check if this change affects a markdown file.
    pub fn is_markdown(&self) -> bool {
        self.path().extension().is_some_and(|ext| ext == "md")
    }
}

/// File system watcher for the knowledge vault.
///
/// Watches a directory for file changes and broadcasts events to subscribers.
/// Uses debouncing to batch rapid changes (e.g., editor save + backup).
pub struct FileWatcher {
    /// The debouncer that wraps the underlying watcher.
    _debouncer: Debouncer<RecommendedWatcher>,
    /// Broadcast sender for vault change events.
    change_tx: broadcast::Sender<VaultChange>,
    /// Handle to the background polling task.
    _poll_handle: std::thread::JoinHandle<()>,
}

impl FileWatcher {
    /// Create a new file watcher for the given vault path.
    ///
    /// The watcher will recursively monitor all files in the directory
    /// and emit `VaultChange` events when markdown files change.
    ///
    /// # Arguments
    /// * `vault_path` - Path to the vault directory to watch
    /// * `debounce_ms` - Debounce duration in milliseconds (default: 500)
    pub fn new(vault_path: &Path, debounce_ms: Option<u64>) -> Result<Self, WatcherError> {
        let debounce_duration = Duration::from_millis(debounce_ms.unwrap_or(500));
        let (change_tx, _) = broadcast::channel(256);
        let change_tx_clone = change_tx.clone();

        // Create a channel for the debouncer
        let (tx, rx) = mpsc::channel();

        // Create debounced watcher
        let mut debouncer = new_debouncer(debounce_duration, tx)?;

        // Start watching the vault path recursively
        debouncer
            .watcher()
            .watch(vault_path, RecursiveMode::Recursive)
            .map_err(|e| WatcherError::WatchPathError {
                path: vault_path.to_path_buf(),
                source: e,
            })?;

        // Spawn a thread to process events and broadcast them
        let poll_handle = std::thread::spawn(move || {
            loop {
                match rx.recv() {
                    Ok(Ok(events)) => {
                        for event in events {
                            if let Some(change) = Self::debounced_event_to_change(event) {
                                // Only broadcast markdown file changes
                                if change.is_markdown() {
                                    // Ignore send errors (no receivers)
                                    let _ = change_tx_clone.send(change);
                                }
                            }
                        }
                    }
                    Ok(Err(error)) => {
                        tracing::warn!("File watcher error: {:?}", error);
                    }
                    Err(_) => {
                        // Channel closed, watcher is being dropped
                        break;
                    }
                }
            }
        });

        Ok(Self {
            _debouncer: debouncer,
            change_tx,
            _poll_handle: poll_handle,
        })
    }

    /// Subscribe to vault change events.
    ///
    /// Returns a receiver that will receive `VaultChange` events
    /// whenever a markdown file in the vault is created, modified, or removed.
    pub fn subscribe(&self) -> broadcast::Receiver<VaultChange> {
        self.change_tx.subscribe()
    }

    /// Convert a debounced event to a VaultChange.
    fn debounced_event_to_change(event: DebouncedEvent) -> Option<VaultChange> {
        let path = event.path;

        // Check if the path exists to determine create vs remove
        if path.exists() {
            // File exists - could be create or modify
            // We treat both as Modified since we can't easily distinguish
            // and the vault will re-parse either way
            Some(VaultChange::Modified(path))
        } else {
            // File doesn't exist - it was removed
            Some(VaultChange::Removed(path))
        }
    }

    /// Get the number of active subscribers.
    #[allow(dead_code)]
    pub fn subscriber_count(&self) -> usize {
        self.change_tx.receiver_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_watcher_detects_new_file() {
        let dir = tempdir().unwrap();
        let watcher = FileWatcher::new(dir.path(), Some(100)).unwrap();
        let mut rx = watcher.subscribe();

        // Create a new markdown file
        let path = dir.path().join("test.md");
        let mut file = File::create(&path).unwrap();
        file.write_all(b"# Test").unwrap();
        drop(file);

        // Wait for the event with timeout
        let result = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;

        assert!(result.is_ok(), "Should receive event");
        let change = result.unwrap().unwrap();
        assert!(matches!(change, VaultChange::Modified(_)));
        // Compare file names only to avoid macOS /var vs /private/var differences
        assert_eq!(change.path().file_name(), path.file_name());
    }

    #[tokio::test]
    async fn test_watcher_detects_modification() {
        let dir = tempdir().unwrap();

        // Create initial file
        let path = dir.path().join("test.md");
        {
            let mut file = File::create(&path).unwrap();
            file.write_all(b"# Original").unwrap();
        }

        // Wait a bit before starting watcher
        tokio::time::sleep(Duration::from_millis(100)).await;

        let watcher = FileWatcher::new(dir.path(), Some(100)).unwrap();
        let mut rx = watcher.subscribe();

        // Modify the file
        {
            let mut file = File::create(&path).unwrap();
            file.write_all(b"# Modified").unwrap();
        }

        // Wait for the event
        let result = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;

        assert!(result.is_ok(), "Should receive event");
        let change = result.unwrap().unwrap();
        assert!(matches!(change, VaultChange::Modified(_)));
    }

    #[tokio::test]
    async fn test_watcher_detects_removal() {
        let dir = tempdir().unwrap();

        // Create initial file
        let path = dir.path().join("test.md");
        {
            let mut file = File::create(&path).unwrap();
            file.write_all(b"# Test").unwrap();
        }

        // Wait a bit before starting watcher
        tokio::time::sleep(Duration::from_millis(100)).await;

        let watcher = FileWatcher::new(dir.path(), Some(100)).unwrap();
        let mut rx = watcher.subscribe();

        // Remove the file
        fs::remove_file(&path).unwrap();

        // Wait for the event
        let result = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;

        assert!(result.is_ok(), "Should receive event");
        let change = result.unwrap().unwrap();
        assert!(matches!(change, VaultChange::Removed(_)));
    }

    #[tokio::test]
    async fn test_watcher_ignores_non_markdown() {
        let dir = tempdir().unwrap();
        let watcher = FileWatcher::new(dir.path(), Some(100)).unwrap();
        let mut rx = watcher.subscribe();

        // Create a non-markdown file
        let path = dir.path().join("test.txt");
        let mut file = File::create(&path).unwrap();
        file.write_all(b"Not markdown").unwrap();
        drop(file);

        // Wait briefly - should NOT receive event
        let result = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;

        // Should timeout because non-markdown files are filtered
        assert!(result.is_err(), "Should not receive event for non-markdown");
    }

    #[test]
    fn test_vault_change_is_markdown() {
        let md_change = VaultChange::Created(PathBuf::from("test.md"));
        let txt_change = VaultChange::Created(PathBuf::from("test.txt"));
        let no_ext_change = VaultChange::Created(PathBuf::from("test"));

        assert!(md_change.is_markdown());
        assert!(!txt_change.is_markdown());
        assert!(!no_ext_change.is_markdown());
    }
}
