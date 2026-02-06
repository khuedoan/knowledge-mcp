//! File system watching for live vault updates.
//!
//! This module provides file system watching functionality using the `notify` crate.
//! It watches the vault directory for changes and emits events that can be used
//! to trigger incremental index updates.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

#[cfg(test)]
use notify::PollWatcher;
#[cfg(not(test))]
use notify::RecommendedWatcher;
use notify::{Config as NotifyConfig, RecursiveMode};
use notify_debouncer_mini::{
    Config as DebouncerConfig, DebouncedEvent, Debouncer, new_debouncer_opt,
};
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
    _debouncer: Debouncer<WatcherBackend>,
    /// Broadcast sender for vault change events.
    change_tx: broadcast::Sender<VaultChange>,
    /// Handle to the background polling task.
    _poll_handle: std::thread::JoinHandle<()>,
}

#[cfg(test)]
type WatcherBackend = PollWatcher;
#[cfg(not(test))]
type WatcherBackend = RecommendedWatcher;

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

        let notify_config = watcher_notify_config();
        let debouncer_config = DebouncerConfig::default()
            .with_timeout(debounce_duration)
            .with_notify_config(notify_config);

        // Create debounced watcher
        let mut debouncer = new_debouncer_opt::<_, WatcherBackend>(debouncer_config, tx)?;

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
fn watcher_notify_config() -> NotifyConfig {
    NotifyConfig::default()
        .with_poll_interval(Duration::from_millis(100))
        .with_compare_contents(true)
}

#[cfg(not(test))]
fn watcher_notify_config() -> NotifyConfig {
    NotifyConfig::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;

    async fn wait_for_change(
        rx: &mut broadcast::Receiver<VaultChange>,
        timeout: Duration,
    ) -> Option<VaultChange> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return None;
            }
            let remaining = deadline - now;
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(change)) => return Some(change),
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(broadcast::error::RecvError::Closed)) => return None,
                Err(_) => return None,
            }
        }
    }

    #[tokio::test]
    async fn test_watcher_detects_new_file() {
        let dir = tempdir().unwrap();
        let watcher = FileWatcher::new(dir.path(), Some(100)).unwrap();
        let mut rx = watcher.subscribe();
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create a new markdown file
        let path = dir.path().join("test.md");
        let mut file = File::create(&path).unwrap();
        file.write_all(b"# Test").unwrap();
        drop(file);

        // Wait for the event with timeout
        let change = wait_for_change(&mut rx, Duration::from_secs(5)).await;

        assert!(change.is_some(), "Should receive event");
        let change = change.unwrap();
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
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Modify the file
        {
            let mut file = File::create(&path).unwrap();
            file.write_all(b"# Modified").unwrap();
        }

        // Wait for the event
        let change = wait_for_change(&mut rx, Duration::from_secs(5)).await;

        assert!(change.is_some(), "Should receive event");
        let change = change.unwrap();
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
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Remove the file
        fs::remove_file(&path).unwrap();

        // Wait for the event
        let change = wait_for_change(&mut rx, Duration::from_secs(5)).await;

        assert!(change.is_some(), "Should receive event");
        let change = change.unwrap();
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
