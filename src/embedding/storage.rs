//! Persistent storage for embedding index.
//!
//! This module handles saving and loading embeddings to/from disk
//! using bincode for efficient binary serialization.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during storage operations.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Failed to create cache directory: {0}")]
    CreateDirError(#[source] std::io::Error),

    #[error("Failed to open file for reading: {0}")]
    OpenReadError(#[source] std::io::Error),

    #[error("Failed to open file for writing: {0}")]
    OpenWriteError(#[source] std::io::Error),

    #[error("Failed to deserialize embeddings: {0}")]
    DeserializeError(#[source] bincode::Error),

    #[error("Failed to serialize embeddings: {0}")]
    SerializeError(#[source] bincode::Error),
}

/// Serializable storage format for embeddings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingStorage {
    /// Model name for compatibility checking.
    pub model_name: String,
    /// Embedding dimensions.
    pub dimensions: usize,
    /// Note name to embedding vector mapping.
    pub embeddings: HashMap<String, Vec<f32>>,
    /// Note name to content hash mapping for change detection.
    pub content_hashes: HashMap<String, u64>,
}

impl EmbeddingStorage {
    /// Create a new empty storage.
    #[allow(dead_code)]
    pub fn new(model_name: String, dimensions: usize) -> Self {
        Self {
            model_name,
            dimensions,
            embeddings: HashMap::new(),
            content_hashes: HashMap::new(),
        }
    }
}

/// Load embeddings from a file.
pub fn load_embeddings(path: &Path) -> Result<EmbeddingStorage, StorageError> {
    let file = File::open(path).map_err(StorageError::OpenReadError)?;
    let reader = BufReader::new(file);

    bincode::deserialize_from(reader).map_err(StorageError::DeserializeError)
}

/// Save embeddings to a file.
pub fn save_embeddings(storage: &EmbeddingStorage, path: &Path) -> Result<(), StorageError> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(StorageError::CreateDirError)?;
    }

    let file = File::create(path).map_err(StorageError::OpenWriteError)?;
    let writer = BufWriter::new(file);

    bincode::serialize_into(writer, storage).map_err(StorageError::SerializeError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_save_and_load_embeddings() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.embeddings");

        // Create test storage
        let mut storage = EmbeddingStorage::new("test-model".to_string(), 384);
        storage
            .embeddings
            .insert("note1".to_string(), vec![0.1, 0.2, 0.3]);
        storage
            .embeddings
            .insert("note2".to_string(), vec![0.4, 0.5, 0.6]);
        storage.content_hashes.insert("note1".to_string(), 12345);
        storage.content_hashes.insert("note2".to_string(), 67890);

        // Save
        save_embeddings(&storage, &path).unwrap();

        // Load
        let loaded = load_embeddings(&path).unwrap();

        assert_eq!(loaded.model_name, "test-model");
        assert_eq!(loaded.dimensions, 384);
        assert_eq!(loaded.embeddings.len(), 2);
        assert_eq!(loaded.content_hashes.len(), 2);
        assert_eq!(
            loaded.embeddings.get("note1").unwrap(),
            &vec![0.1, 0.2, 0.3]
        );
    }

    #[test]
    fn test_load_nonexistent_file() {
        let result = load_embeddings(Path::new("/nonexistent/path.embeddings"));
        assert!(result.is_err());
    }

    #[test]
    fn test_save_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir
            .path()
            .join("nested")
            .join("dirs")
            .join("test.embeddings");

        let storage = EmbeddingStorage::new("test".to_string(), 384);
        save_embeddings(&storage, &path).unwrap();

        assert!(path.exists());
    }
}
