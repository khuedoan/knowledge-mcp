//! Embedding generation and semantic search for the knowledge vault.
//!
//! This module provides vector embeddings for notes using the `fastembed` crate,
//! enabling semantic similarity search across the vault.
//!
//! Features:
//! - Local embedding generation (no external API required)
//! - Content hash-based change detection for efficient re-embedding
//! - Chunked embeddings with overlap for long notes
//! - Optional link-context embeddings (outgoing links + backlinks)
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

const EMBEDDING_FORMAT_VERSION: u8 = 3;
const EMBEDDING_KEY_DELIM: char = '|';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmbeddingKind {
    Content,
    LinkContext,
}

impl EmbeddingKind {
    fn code(self) -> &'static str {
        match self {
            EmbeddingKind::Content => "c",
            EmbeddingKind::LinkContext => "l",
        }
    }

    fn from_code(code: &str) -> Self {
        match code {
            "l" => EmbeddingKind::LinkContext,
            _ => EmbeddingKind::Content,
        }
    }
}

#[derive(Debug, Clone)]
struct PreparedChunk {
    id: String,
    text: String,
}

#[derive(Debug, Clone)]
struct PreparedNote {
    note_name: String,
    hash: u64,
    chunks: Vec<PreparedChunk>,
}

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

/// Input for embedding a note with its content and backlinks.
#[derive(Debug, Clone)]
pub struct EmbeddingInput {
    pub note: Note,
    pub content: String,
    pub backlinks: Vec<String>,
}

/// Configuration for the embedding index.
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// Maximum characters per content chunk.
    pub max_content_chars: usize,
    /// Overlap size between content chunks (in characters).
    pub chunk_overlap_chars: usize,
    /// Whether to include headings in the embedding text.
    pub include_headings: bool,
    /// Whether to include a link-context embedding per note.
    pub include_link_context: bool,
    /// Maximum characters for link-context embedding text.
    pub link_context_max_chars: usize,
    /// Weight applied to link-context similarity scores.
    pub link_context_weight: f32,
    /// Cache directory for storing embeddings.
    pub cache_dir: std::path::PathBuf,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            max_content_chars: 1800,
            chunk_overlap_chars: 200,
            include_headings: true,
            include_link_context: true,
            link_context_max_chars: 320,
            link_context_weight: 0.7,
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
    /// Create a new embedding index with the default model (BGE-base-en-v1.5).
    ///
    /// This will download the model on first use (~90MB).
    pub fn new(config: EmbeddingConfig) -> Result<Self, EmbeddingError> {
        Self::with_model(EmbeddingModel::BGEBaseENV15, config)
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

    fn embedding_id(note_name: &str, kind: EmbeddingKind, chunk_index: usize) -> String {
        format!(
            "{}{}{}{}{}",
            note_name,
            EMBEDDING_KEY_DELIM,
            kind.code(),
            EMBEDDING_KEY_DELIM,
            chunk_index
        )
    }

    fn embedding_note_name(key: &str) -> &str {
        key.split(EMBEDDING_KEY_DELIM).next().unwrap_or(key)
    }

    fn embedding_kind(key: &str) -> EmbeddingKind {
        let mut parts = key.split(EMBEDDING_KEY_DELIM);
        let _note = parts.next();
        let kind = parts.next().unwrap_or("");
        EmbeddingKind::from_code(kind)
    }

    fn note_centroids(&self) -> HashMap<String, Vec<f32>> {
        let mut sums: HashMap<String, (Vec<f32>, f32)> = HashMap::new();

        for (key, embedding) in &self.embeddings {
            let note_name = Self::embedding_note_name(key);
            let kind = Self::embedding_kind(key);
            let weight = if kind == EmbeddingKind::LinkContext {
                self.config.link_context_weight
            } else {
                1.0
            };

            let entry = sums
                .entry(note_name.to_string())
                .or_insert_with(|| (vec![0.0; self.dimensions], 0.0));
            for (i, value) in embedding.iter().enumerate() {
                entry.0[i] += value * weight;
            }
            entry.1 += weight;
        }

        let mut centroids = HashMap::new();
        for (name, (mut sum, weight_sum)) in sums {
            if weight_sum > 0.0 {
                for value in &mut sum {
                    *value /= weight_sum;
                }
                centroids.insert(name, sum);
            }
        }

        centroids
    }

    /// Build a link-context string for embeddings.
    fn build_link_context(&self, note: &Note, backlinks: &[String]) -> String {
        if !self.config.include_link_context {
            return String::new();
        }

        let mut outgoing: Vec<String> = note
            .links
            .iter()
            .filter(|l| !l.target.is_empty())
            .map(|l| match &l.display {
                Some(display) => format!("{} ({})", l.target, display),
                None => l.target.clone(),
            })
            .collect();

        outgoing.sort();
        outgoing.dedup();

        let mut backlinks: Vec<String> = backlinks.iter().cloned().collect();
        backlinks.sort();
        backlinks.dedup();

        if outgoing.is_empty() && backlinks.is_empty() {
            return String::new();
        }

        let mut text = String::new();

        if !outgoing.is_empty() {
            text.push_str("Links: ");
            text.push_str(&outgoing.join(", "));
            text.push_str(". ");
        }

        if !backlinks.is_empty() {
            text.push_str("Backlinks: ");
            text.push_str(&backlinks.join(", "));
            text.push_str(". ");
        }

        truncate_to_chars(&text, self.config.link_context_max_chars)
    }

    fn note_hash(content: &str, link_context: &str) -> u64 {
        use std::hash::{Hash, Hasher};

        let mut hasher = seahash::SeaHasher::new();
        EMBEDDING_FORMAT_VERSION.hash(&mut hasher);
        content.hash(&mut hasher);
        link_context.hash(&mut hasher);
        hasher.finish()
    }

    fn prepare_note_with_link_context(
        &self,
        note: &Note,
        content: &str,
        link_context: &str,
        hash: u64,
    ) -> PreparedNote {
        let mut chunks = self.build_content_chunks(note, content);

        if self.config.include_link_context && !link_context.trim().is_empty() {
            let mut text = String::new();
            text.push_str(note.title.as_deref().unwrap_or(&note.name));
            text.push_str(". ");
            text.push_str(link_context.trim());
            let max_chars = self
                .config
                .link_context_max_chars
                .min(self.config.max_content_chars.max(1));
            text = truncate_to_chars(&text, max_chars);

            chunks.push(PreparedChunk {
                id: Self::embedding_id(&note.name, EmbeddingKind::LinkContext, 0),
                text,
            });
        }

        PreparedNote {
            note_name: note.name.clone(),
            hash,
            chunks,
        }
    }

    fn build_content_chunks(&self, note: &Note, content: &str) -> Vec<PreparedChunk> {
        let mut chunks = Vec::new();
        let title = note.title.as_deref().unwrap_or(&note.name);
        let sections = split_markdown_sections(content);
        let mut chunk_index = 0usize;

        for section in sections {
            let prefix =
                build_content_prefix(title, &section.heading_path, self.config.include_headings);
            let prefix_len = char_len(&prefix);
            let max_chars = self.config.max_content_chars.max(1);
            let available = max_chars.saturating_sub(prefix_len).max(1);
            let overlap = self
                .config
                .chunk_overlap_chars
                .min(available.saturating_sub(1));
            let body_chunks = split_text_with_overlap(&section.body, available, overlap);

            for body in body_chunks {
                if body.trim().is_empty() {
                    continue;
                }

                let mut text = String::new();
                text.push_str(&prefix);
                text.push_str(body.trim());
                text = truncate_to_chars(&text, max_chars);

                chunks.push(PreparedChunk {
                    id: Self::embedding_id(&note.name, EmbeddingKind::Content, chunk_index),
                    text,
                });
                chunk_index += 1;
            }
        }

        if chunks.is_empty() {
            let prefix = build_content_prefix(title, &[], self.config.include_headings);
            let text = truncate_to_chars(&prefix, self.config.max_content_chars.max(1));
            chunks.push(PreparedChunk {
                id: Self::embedding_id(&note.name, EmbeddingKind::Content, 0),
                text,
            });
        }

        chunks
    }

    /// Check if a note needs re-embedding based on content hash.
    pub fn needs_update_hash(&self, note_name: &str, hash: u64) -> bool {
        match self.content_hashes.get(note_name) {
            Some(&old_hash) => old_hash != hash,
            None => true,
        }
    }

    /// Check if a note needs re-embedding based on content + link context.
    pub fn needs_update(&self, note_name: &str, content: &str, link_context: &str) -> bool {
        let hash = Self::note_hash(content, link_context);
        self.needs_update_hash(note_name, hash)
    }

    /// Generate and store embedding for a single note.
    pub fn embed_note(
        &mut self,
        note: &Note,
        content: &str,
        backlinks: &[String],
    ) -> Result<(), EmbeddingError> {
        let link_context = self.build_link_context(note, backlinks);
        let hash = Self::note_hash(content, &link_context);

        if !self.needs_update_hash(&note.name, hash) {
            return Ok(());
        }

        let prepared = self.prepare_note_with_link_context(note, content, &link_context, hash);
        self.embed_prepared_notes_batch(vec![prepared])?;
        Ok(())
    }

    /// Generate embeddings for multiple notes in batch.
    ///
    /// This is more efficient than calling `embed_note` repeatedly.
    pub fn embed_notes_batch(
        &mut self,
        notes_with_content: Vec<EmbeddingInput>,
    ) -> Result<usize, EmbeddingError> {
        if notes_with_content.is_empty() {
            return Ok(0);
        }

        let mut prepared_notes = Vec::new();
        for input in notes_with_content {
            let link_context = self.build_link_context(&input.note, &input.backlinks);
            let hash = Self::note_hash(&input.content, &link_context);
            if !self.needs_update_hash(&input.note.name, hash) {
                continue;
            }
            prepared_notes.push(self.prepare_note_with_link_context(
                &input.note,
                &input.content,
                &link_context,
                hash,
            ));
        }

        if prepared_notes.is_empty() {
            return Ok(0);
        }

        self.embed_prepared_notes_batch(prepared_notes)
    }

    fn embed_prepared_notes_batch(
        &mut self,
        prepared_notes: Vec<PreparedNote>,
    ) -> Result<usize, EmbeddingError> {
        let mut texts = Vec::new();
        let mut meta = Vec::new();

        for prepared in prepared_notes {
            self.remove(&prepared.note_name);
            for chunk in prepared.chunks {
                texts.push(chunk.text);
                meta.push((prepared.note_name.clone(), prepared.hash, chunk.id));
            }
        }

        if texts.is_empty() {
            return Ok(0);
        }

        let embeddings = self
            .model
            .embed(texts, None)
            .map_err(|e| EmbeddingError::EmbedError(e.to_string()))?;

        let count = embeddings.len();
        for (embedding, (note_name, hash, id)) in embeddings.into_iter().zip(meta) {
            self.embeddings.insert(id, embedding);
            self.content_hashes.insert(note_name, hash);
        }

        Ok(count)
    }

    /// Remove embedding for a note.
    pub fn remove(&mut self, note_name: &str) {
        let prefix = format!("{}{}", note_name, EMBEDDING_KEY_DELIM);
        self.embeddings
            .retain(|key, _| key != note_name && !key.starts_with(&prefix));
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

        // Compute similarities, aggregated by note (max over chunks)
        let mut note_scores: HashMap<String, f32> = HashMap::new();
        for (key, embedding) in &self.embeddings {
            let mut similarity = cosine_similarity(&query_embedding, embedding);
            let kind = Self::embedding_kind(key);
            if kind == EmbeddingKind::LinkContext {
                similarity *= self.config.link_context_weight;
            }

            let note_name = Self::embedding_note_name(key);
            let entry = note_scores
                .entry(note_name.to_string())
                .or_insert(similarity);
            if similarity > *entry {
                *entry = similarity;
            }
        }

        let mut similarities: Vec<SimilarNote> = note_scores
            .into_iter()
            .map(|(name, similarity)| SimilarNote { name, similarity })
            .collect();

        similarities.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
        similarities.truncate(limit);

        Ok(similarities)
    }

    /// Find notes similar to a given note.
    pub fn find_similar(
        &self,
        note_name: &str,
        limit: usize,
    ) -> Result<Vec<SimilarNote>, EmbeddingError> {
        let centroids = self.note_centroids();
        let note_embedding = centroids
            .get(note_name)
            .ok_or_else(|| EmbeddingError::NoteNotFound(note_name.to_string()))?;

        let mut similarities: Vec<SimilarNote> = centroids
            .iter()
            .filter(|(name, _)| name.as_str() != note_name)
            .map(|(name, embedding)| SimilarNote {
                name: name.clone(),
                similarity: cosine_similarity(note_embedding, embedding),
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
        let note_count = if !self.content_hashes.is_empty() {
            self.content_hashes.len()
        } else {
            self.embeddings
                .keys()
                .map(|k| Self::embedding_note_name(k).to_string())
                .collect::<std::collections::HashSet<_>>()
                .len()
        };

        EmbeddingStats {
            note_count,
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

#[derive(Debug, Clone)]
struct Section {
    heading_path: Vec<String>,
    body: String,
}

fn split_markdown_sections(content: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut current = Section {
        heading_path: Vec::new(),
        body: String::new(),
    };
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut in_code_fence = false;
    let mut fence_marker = String::new();

    for line in content.lines() {
        let trimmed = line.trim_start();

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let marker: String = trimmed.chars().take(3).collect();
            if !in_code_fence {
                in_code_fence = true;
                fence_marker = marker;
            } else if marker == fence_marker {
                in_code_fence = false;
                fence_marker.clear();
            }
            current.body.push_str(line);
            current.body.push('\n');
            continue;
        }

        if in_code_fence {
            current.body.push_str(line);
            current.body.push('\n');
            continue;
        }

        if let Some((level, text)) = parse_heading_line(line) {
            if !current.body.trim().is_empty() || !current.heading_path.is_empty() {
                sections.push(current);
            }

            while let Some((prev_level, _)) = heading_stack.last() {
                if *prev_level >= level {
                    heading_stack.pop();
                } else {
                    break;
                }
            }

            heading_stack.push((level, text));
            current = Section {
                heading_path: heading_stack.iter().map(|(_, t)| t.clone()).collect(),
                body: String::new(),
            };
            continue;
        }

        current.body.push_str(line);
        current.body.push('\n');
    }

    if !current.body.trim().is_empty() || !current.heading_path.is_empty() {
        sections.push(current);
    }

    if sections.is_empty() && !content.trim().is_empty() {
        sections.push(Section {
            heading_path: Vec::new(),
            body: content.to_string(),
        });
    }

    sections
}

fn parse_heading_line(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim_start();
    let hash_count = trimmed.chars().take_while(|c| *c == '#').count();
    if hash_count == 0 || hash_count > 6 {
        return None;
    }
    let remainder = trimmed.chars().skip(hash_count).collect::<String>();
    let remainder = remainder.trim_start();
    if remainder.is_empty() {
        return None;
    }
    Some((hash_count as u8, remainder.trim().to_string()))
}

fn build_content_prefix(title: &str, heading_path: &[String], include_headings: bool) -> String {
    let mut prefix = String::new();
    prefix.push_str(title);
    prefix.push_str(". ");

    if include_headings && !heading_path.is_empty() {
        prefix.push_str("Section: ");
        prefix.push_str(&heading_path.join(" > "));
        prefix.push_str(". ");
    }

    prefix
}

fn split_text_with_overlap(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let chars: Vec<char> = trimmed.chars().collect();
    let len = chars.len();
    let mut chunks = Vec::new();
    let mut start = 0usize;
    let max_chars = max_chars.max(1);
    let overlap = overlap.min(max_chars.saturating_sub(1));

    while start < len {
        let mut end = (start + max_chars).min(len);

        if end < len {
            let mut back = end;
            while back > start && !chars[back - 1].is_whitespace() {
                back -= 1;
            }
            if back > start + max_chars / 2 {
                end = back;
            }
        }

        if end == start {
            end = (start + max_chars).min(len);
        }

        let chunk: String = chars[start..end].iter().collect();
        let chunk = chunk.trim().to_string();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }

        if end == len {
            break;
        }

        let step_back = overlap.min(end.saturating_sub(start));
        start = end.saturating_sub(step_back);
        if start == end {
            start = end.saturating_add(1);
        }
    }

    chunks
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

fn truncate_to_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars.max(1)).collect()
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
    fn test_note_hash() {
        let content1 = "Hello world";
        let content2 = "Hello world";
        let content3 = "Different content";

        assert_eq!(
            EmbeddingIndex::note_hash(content1, ""),
            EmbeddingIndex::note_hash(content2, "")
        );
        assert_ne!(
            EmbeddingIndex::note_hash(content1, ""),
            EmbeddingIndex::note_hash(content3, "")
        );
    }

    #[test]
    fn test_split_markdown_sections_respects_code_fences() {
        let content = r#"# Title

Intro text.

```
# Not a heading
```

## Section One

Body text.
"#;
        let sections = split_markdown_sections(content);
        assert_eq!(sections.len(), 2);
        assert!(sections[0].body.contains("# Not a heading"));
        assert_eq!(sections[1].heading_path, vec!["Title", "Section One"]);
    }

    #[test]
    fn test_split_text_with_overlap() {
        let text = "abcdefghijklmnopqrstuvwxyz";
        let chunks = split_text_with_overlap(text, 10, 3);
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert!(char_len(chunk) <= 10);
        }
    }
}
