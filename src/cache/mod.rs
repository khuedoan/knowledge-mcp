//! Content caching with modification time tracking.
//!
//! This module provides an LRU cache for note contents that tracks file modification
//! times to avoid re-reading unchanged files from disk.

use std::fs;
use std::num::NonZeroUsize;
use std::path::Path;
use std::time::SystemTime;

use lru::LruCache;
use thiserror::Error;

/// Errors that can occur during cache operations.
#[derive(Debug, Error)]
pub enum CacheError {
    #[error("Failed to read file: {path}")]
    ReadError {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to get file metadata: {path}")]
    MetadataError {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Cached content with its modification time.
#[derive(Debug, Clone)]
struct CachedContent {
    content: String,
    modified: SystemTime,
}

/// LRU cache for note contents with modification time tracking.
#[derive(Debug)]
pub struct ContentCache {
    cache: LruCache<String, CachedContent>,
}

impl ContentCache {
    /// Create a new content cache with the specified capacity.
    ///
    /// # Arguments
    /// * `capacity` - Maximum number of notes to cache
    pub fn new(capacity: usize) -> Self {
        let capacity = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(1).unwrap());
        Self {
            cache: LruCache::new(capacity),
        }
    }

    /// Get content from cache or read from disk.
    ///
    /// Returns cached content if:
    /// 1. The file is in the cache
    /// 2. The file's modification time hasn't changed
    ///
    /// Otherwise, reads from disk and updates the cache.
    pub fn get_or_read(&mut self, note_name: &str, path: &Path) -> Result<String, CacheError> {
        // Get current file modification time
        let current_mtime = fs::metadata(path).and_then(|m| m.modified()).map_err(|e| {
            CacheError::MetadataError {
                path: path.display().to_string(),
                source: e,
            }
        })?;

        // Check if we have a valid cached version
        if let Some(cached) = self.cache.get(note_name) {
            if cached.modified == current_mtime {
                return Ok(cached.content.clone());
            }
        }

        // Read from disk and update cache
        let content = fs::read_to_string(path).map_err(|e| CacheError::ReadError {
            path: path.display().to_string(),
            source: e,
        })?;

        self.cache.put(
            note_name.to_string(),
            CachedContent {
                content: content.clone(),
                modified: current_mtime,
            },
        );

        Ok(content)
    }

    /// Invalidate a specific cache entry.
    pub fn invalidate(&mut self, note_name: &str) {
        self.cache.pop(note_name);
    }

    /// Clear the entire cache.
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Get the number of cached entries.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if the cache is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Get cache statistics for debugging.
    #[allow(dead_code)]
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.cache.len(),
            capacity: self.cache.cap().get(),
        }
    }
}

/// Cache statistics for debugging/monitoring.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entries: usize,
    pub capacity: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::thread::sleep;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn test_cache_new() {
        let cache = ContentCache::new(100);
        assert!(cache.is_empty());
        assert_eq!(cache.stats().capacity, 100);
    }

    #[test]
    fn test_cache_get_or_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        let mut file = File::create(&path).unwrap();
        file.write_all(b"# Test Content").unwrap();

        let mut cache = ContentCache::new(10);

        // First read should hit disk
        let content = cache.get_or_read("test", &path).unwrap();
        assert_eq!(content, "# Test Content");
        assert_eq!(cache.len(), 1);

        // Second read should hit cache
        let content2 = cache.get_or_read("test", &path).unwrap();
        assert_eq!(content2, "# Test Content");
    }

    #[test]
    fn test_cache_invalidation_on_modify() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");

        // Create initial file
        {
            let mut file = File::create(&path).unwrap();
            file.write_all(b"Original content").unwrap();
        }

        let mut cache = ContentCache::new(10);

        // Read initial content
        let content1 = cache.get_or_read("test", &path).unwrap();
        assert_eq!(content1, "Original content");

        // Wait a bit and modify the file
        sleep(Duration::from_millis(100));
        {
            let mut file = File::create(&path).unwrap();
            file.write_all(b"Modified content").unwrap();
        }

        // Should detect modification and re-read
        let content2 = cache.get_or_read("test", &path).unwrap();
        assert_eq!(content2, "Modified content");
    }

    #[test]
    fn test_cache_invalidate() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        let mut file = File::create(&path).unwrap();
        file.write_all(b"Content").unwrap();

        let mut cache = ContentCache::new(10);
        cache.get_or_read("test", &path).unwrap();
        assert_eq!(cache.len(), 1);

        cache.invalidate("test");
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_clear() {
        let dir = tempdir().unwrap();

        let mut cache = ContentCache::new(10);

        for i in 0..5 {
            let path = dir.path().join(format!("test{}.md", i));
            let mut file = File::create(&path).unwrap();
            file.write_all(format!("Content {}", i).as_bytes()).unwrap();
            cache.get_or_read(&format!("test{}", i), &path).unwrap();
        }

        assert_eq!(cache.len(), 5);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_lru_eviction() {
        let dir = tempdir().unwrap();
        let mut cache = ContentCache::new(3); // Small cache

        // Create and read 5 files
        for i in 0..5 {
            let path = dir.path().join(format!("test{}.md", i));
            let mut file = File::create(&path).unwrap();
            file.write_all(format!("Content {}", i).as_bytes()).unwrap();
            cache.get_or_read(&format!("test{}", i), &path).unwrap();
        }

        // Cache should only have 3 entries (LRU evicted oldest)
        assert_eq!(cache.len(), 3);
    }
}
