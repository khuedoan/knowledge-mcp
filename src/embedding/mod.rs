//! Embedding generation and semantic search for the knowledge vault.
//!
//! This module provides vector embeddings for notes using the `fastembed` crate,
//! enabling semantic similarity search across the vault.
//!
//! Features:
//! - Local embedding generation (no external API required)
//! - Content hash-based change detection for efficient re-embedding
//! - Persistent storage of embeddings to avoid re-computation
//! - Cosine similarity search

mod storage;

use std::collections::HashMap;
use std::path::Path;

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use storage::{EmbeddingStorage, load_embeddings, save_embeddings};

use crate::vault::Note;

/// Errors that can occur during embedding operations.
#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("Failed to initialize embedding model: {0}")]
    ModelInitError(String),

    #[error("Failed to generate embeddings: {0}")]
    EmbedError(String),

    #[error("Note not found in embedding index: {0}")]
    NoteNotFound(String),

    #[error("Storage error: {0}")]
    StorageError(#[from] storage::StorageError),
}

/// A note with its similarity score from a semantic search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarNote {
    /// The note name.
    pub name: String,
    /// Cosine similarity score (0.0 to 1.0).
    pub similarity: f32,
}

/// Configuration for the embedding index.
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// Maximum characters of content to include in embedding.
    pub max_content_chars: usize,
    /// Whether to include headings in the embedding text.
    pub include_headings: bool,
    /// Cache directory for storing embeddings.
    pub cache_dir: std::path::PathBuf,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            max_content_chars: 500,
            include_headings: true,
            cache_dir: dirs::cache_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("knowledge-mcp"),
        }
    }
}

/// Embedding index for semantic search over notes.
///
/// Stores vector embeddings for each note and provides similarity search.
pub struct EmbeddingIndex {
    /// The embedding model.
    model: TextEmbedding,
    /// Embeddings by note name.
    embeddings: HashMap<String, Vec<f32>>,
    /// Content hashes for change detection.
    content_hashes: HashMap<String, u64>,
    /// Model name for storage compatibility.
    model_name: String,
    /// Embedding dimensions.
    dimensions: usize,
    /// Configuration.
    config: EmbeddingConfig,
}

impl EmbeddingIndex {
    /// Create a new embedding index with the default model (BGE-small-en-v1.5).
    ///
    /// This will download the model on first use (~33MB).
    pub fn new(config: EmbeddingConfig) -> Result<Self, EmbeddingError> {
        Self::with_model(EmbeddingModel::BGESmallENV15, config)
    }

    /// Create a new embedding index with a specific model.
    pub fn with_model(
        model: EmbeddingModel,
        config: EmbeddingConfig,
    ) -> Result<Self, EmbeddingError> {
        let model_name = format!("{:?}", model);
        let dimensions = Self::model_dimensions(&model);

        // Initialize with cache directory for model storage
        let init_options = InitOptions::new(model)
            .with_show_download_progress(true)
            .with_cache_dir(config.cache_dir.join("models"));

        let text_embedding = TextEmbedding::try_new(init_options)
            .map_err(|e| EmbeddingError::ModelInitError(e.to_string()))?;

        Ok(Self {
            model: text_embedding,
            embeddings: HashMap::new(),
            content_hashes: HashMap::new(),
            model_name,
            dimensions,
            config,
        })
    }

    /// Load embeddings from storage or create a new index.
    pub fn load_or_create(vault_id: &str, config: EmbeddingConfig) -> Result<Self, EmbeddingError> {
        let storage_path = config.cache_dir.join(format!("{}.embeddings", vault_id));

        // Try to load existing embeddings
        if storage_path.exists() {
            match load_embeddings(&storage_path) {
                Ok(storage) => {
                    tracing::info!("Loaded {} embeddings from cache", storage.embeddings.len());

                    // Create model and restore state
                    let mut index = Self::new(config.clone())?;

                    // Only restore if model matches
                    if storage.model_name == index.model_name {
                        index.embeddings = storage.embeddings;
                        index.content_hashes = storage.content_hashes;
                        return Ok(index);
                    } else {
                        tracing::warn!(
                            "Model mismatch (stored: {}, current: {}), re-embedding required",
                            storage.model_name,
                            index.model_name
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to load embeddings: {}, creating new index", e);
                }
            }
        }

        Self::new(config)
    }

    /// Get the embedding dimensions for a model.
    fn model_dimensions(model: &EmbeddingModel) -> usize {
        match model {
            EmbeddingModel::BGESmallENV15 => 384,
            EmbeddingModel::BGEBaseENV15 => 768,
            EmbeddingModel::BGELargeENV15 => 1024,
            EmbeddingModel::AllMiniLML6V2 => 384,
            EmbeddingModel::AllMiniLML12V2 => 384,
            _ => 384, // Default assumption
        }
    }

    /// Generate embedding text from a note's content.
    ///
    /// The embedding text includes:
    /// 1. Title (most important)
    /// 2. H2 headings (structure/subtopics)
    /// 3. First N characters of content (summary)
    fn prepare_embedding_text(&self, note: &Note, content: &str) -> String {
        let mut text = String::new();

        // Add title (most important for semantic meaning)
        if let Some(title) = &note.title {
            text.push_str(title);
            text.push_str(". ");
        } else {
            // Fall back to note name if no title
            text.push_str(&note.name);
            text.push_str(". ");
        }

        // Add H2 headings (capture document structure)
        if self.config.include_headings {
            for heading in &note.headings {
                if heading.level == 2 {
                    text.push_str(&heading.text);
                    text.push_str(". ");
                }
            }
        }

        // Add first N characters of content
        let content_preview: String = content
            .chars()
            .take(self.config.max_content_chars)
            .collect();
        text.push_str(&content_preview);

        text
    }

    /// Compute a content hash for change detection.
    fn content_hash(content: &str) -> u64 {
        seahash::hash(content.as_bytes())
    }

    /// Check if a note needs re-embedding based on content hash.
    pub fn needs_update(&self, note_name: &str, content: &str) -> bool {
        let new_hash = Self::content_hash(content);
        match self.content_hashes.get(note_name) {
            Some(&old_hash) => old_hash != new_hash,
            None => true,
        }
    }

    /// Generate and store embedding for a single note.
    pub fn embed_note(&mut self, note: &Note, content: &str) -> Result<(), EmbeddingError> {
        let text = self.prepare_embedding_text(note, content);
        let hash = Self::content_hash(content);

        // Generate embedding
        let embeddings = self
            .model
            .embed(vec![text], None)
            .map_err(|e| EmbeddingError::EmbedError(e.to_string()))?;

        if let Some(embedding) = embeddings.into_iter().next() {
            self.embeddings.insert(note.name.clone(), embedding);
            self.content_hashes.insert(note.name.clone(), hash);
        }

        Ok(())
    }

    /// Generate embeddings for multiple notes in batch.
    ///
    /// This is more efficient than calling `embed_note` repeatedly.
    pub fn embed_notes_batch(
        &mut self,
        notes_with_content: Vec<(&Note, &str)>,
    ) -> Result<usize, EmbeddingError> {
        if notes_with_content.is_empty() {
            return Ok(0);
        }

        // Prepare texts and track which notes they correspond to
        let mut texts = Vec::new();
        let mut note_info: Vec<(String, u64)> = Vec::new();

        for (note, content) in &notes_with_content {
            // Skip if content hasn't changed
            if !self.needs_update(&note.name, content) {
                continue;
            }

            let text = self.prepare_embedding_text(note, content);
            let hash = Self::content_hash(content);

            texts.push(text);
            note_info.push((note.name.clone(), hash));
        }

        if texts.is_empty() {
            return Ok(0);
        }

        // Generate embeddings in batch
        let embeddings = self
            .model
            .embed(texts, None)
            .map_err(|e| EmbeddingError::EmbedError(e.to_string()))?;

        // Store results
        let count = embeddings.len();
        for (embedding, (name, hash)) in embeddings.into_iter().zip(note_info) {
            self.embeddings.insert(name.clone(), embedding);
            self.content_hashes.insert(name, hash);
        }

        Ok(count)
    }

    /// Remove embedding for a note.
    pub fn remove(&mut self, note_name: &str) {
        self.embeddings.remove(note_name);
        self.content_hashes.remove(note_name);
    }

    /// Perform semantic search for notes similar to a query.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SimilarNote>, EmbeddingError> {
        if self.embeddings.is_empty() {
            return Ok(Vec::new());
        }

        // Generate query embedding
        let query_embeddings = self
            .model
            .embed(vec![query.to_string()], None)
            .map_err(|e| EmbeddingError::EmbedError(e.to_string()))?;

        let query_embedding = query_embeddings
            .into_iter()
            .next()
            .ok_or_else(|| EmbeddingError::EmbedError("No embedding generated".to_string()))?;

        // Compute similarities
        let mut similarities: Vec<SimilarNote> = self
            .embeddings
            .iter()
            .map(|(name, embedding)| {
                let similarity = cosine_similarity(&query_embedding, embedding);
                SimilarNote {
                    name: name.clone(),
                    similarity,
                }
            })
            .collect();

        // Sort by similarity (descending)
        similarities.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());

        // Take top results
        similarities.truncate(limit);

        Ok(similarities)
    }

    /// Find notes similar to a given note.
    pub fn find_similar(
        &self,
        note_name: &str,
        limit: usize,
    ) -> Result<Vec<SimilarNote>, EmbeddingError> {
        let note_embedding = self
            .embeddings
            .get(note_name)
            .ok_or_else(|| EmbeddingError::NoteNotFound(note_name.to_string()))?;

        // Compute similarities to all other notes
        let mut similarities: Vec<SimilarNote> = self
            .embeddings
            .iter()
            .filter(|(name, _)| *name != note_name) // Exclude the query note
            .map(|(name, embedding)| {
                let similarity = cosine_similarity(note_embedding, embedding);
                SimilarNote {
                    name: name.clone(),
                    similarity,
                }
            })
            .collect();

        // Sort by similarity (descending)
        similarities.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());

        // Take top results
        similarities.truncate(limit);

        Ok(similarities)
    }

    /// Save embeddings to storage.
    pub fn save(&self, vault_id: &str) -> Result<(), EmbeddingError> {
        let storage_path = self
            .config
            .cache_dir
            .join(format!("{}.embeddings", vault_id));

        let storage = EmbeddingStorage {
            model_name: self.model_name.clone(),
            dimensions: self.dimensions,
            embeddings: self.embeddings.clone(),
            content_hashes: self.content_hashes.clone(),
        };

        save_embeddings(&storage, &storage_path)?;
        Ok(())
    }

    /// Get the number of embedded notes.
    pub fn len(&self) -> usize {
        self.embeddings.len()
    }

    /// Check if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.embeddings.is_empty()
    }

    /// Get embedding statistics.
    #[allow(dead_code)]
    pub fn stats(&self) -> EmbeddingStats {
        EmbeddingStats {
            note_count: self.embeddings.len(),
            dimensions: self.dimensions,
            model_name: self.model_name.clone(),
        }
    }
}

/// Embedding index statistics.
#[derive(Debug, Clone)]
pub struct EmbeddingStats {
    pub note_count: usize,
    pub dimensions: usize,
    pub model_name: String,
}

/// Compute cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if magnitude_a == 0.0 || magnitude_b == 0.0 {
        return 0.0;
    }

    dot_product / (magnitude_a * magnitude_b)
}

/// Generate a vault ID from the vault path for cache storage.
pub fn vault_id_from_path(path: &Path) -> String {
    let hash = seahash::hash(path.to_string_lossy().as_bytes());
    format!("{:016x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &c) - 0.0).abs() < 0.001);

        let d = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &d) - (-1.0)).abs() < 0.001);
    }

    #[test]
    fn test_vault_id_from_path() {
        let path1 = std::path::Path::new("/home/user/vault1");
        let path2 = std::path::Path::new("/home/user/vault2");

        let id1 = vault_id_from_path(path1);
        let id2 = vault_id_from_path(path2);

        assert_ne!(id1, id2);
        assert_eq!(id1.len(), 16);
    }

    #[test]
    fn test_content_hash() {
        let content1 = "Hello world";
        let content2 = "Hello world";
        let content3 = "Different content";

        assert_eq!(
            EmbeddingIndex::content_hash(content1),
            EmbeddingIndex::content_hash(content2)
        );
        assert_ne!(
            EmbeddingIndex::content_hash(content1),
            EmbeddingIndex::content_hash(content3)
        );
    }
}
