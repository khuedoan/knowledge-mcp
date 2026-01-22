//! Vault management for indexing and accessing markdown notes.
//!
//! This module provides the `Vault` struct which indexes all markdown files
//! in a directory, extracting wiki links, headings, and metadata.
//!
//! Features:
//! - Parallel indexing using rayon for fast initial load
//! - Incremental updates for file changes (add/modify/remove)

pub mod note;
pub mod parser;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rayon::prelude::*;
use thiserror::Error;
use walkdir::WalkDir;

pub use note::Note;
pub use parser::parse_markdown;

/// Errors that can occur during vault operations.
#[derive(Debug, Error)]
pub enum VaultError {
    #[error("Vault path does not exist: {0}")]
    PathNotFound(PathBuf),

    #[error("Failed to read file: {path}")]
    ReadError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to write file: {path}")]
    WriteError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to delete file: {path}")]
    DeleteError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Invalid note name: {0}")]
    InvalidNoteName(String),

    #[error("Note already exists: {0}")]
    NoteAlreadyExists(String),

    #[error("Note not found: {0}")]
    NoteNotFound(String),

    #[error("Failed to walk directory: {0}")]
    WalkError(#[from] walkdir::Error),
}

/// A knowledge vault containing markdown notes.
#[derive(Debug)]
pub struct Vault {
    /// Path to the vault root directory.
    path: PathBuf,
    /// Indexed notes by name (lowercase for case-insensitive lookup).
    notes: HashMap<String, Note>,
    /// Whether the vault has been indexed.
    indexed: bool,
}

impl Vault {
    /// Create a new vault at the given path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            notes: HashMap::new(),
            indexed: false,
        }
    }

    /// Get the vault root path.
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Check if the vault has been indexed.
    #[allow(dead_code)]
    pub fn is_indexed(&self) -> bool {
        self.indexed
    }

    /// Ensure the vault is indexed, indexing if necessary.
    pub fn ensure_indexed(&mut self) -> Result<(), VaultError> {
        if !self.indexed {
            self.index()?;
        }
        Ok(())
    }

    /// Index all markdown files in the vault using parallel processing.
    pub fn index(&mut self) -> Result<(), VaultError> {
        if !self.path.exists() {
            return Err(VaultError::PathNotFound(self.path.clone()));
        }

        self.notes.clear();

        // Collect all markdown file paths
        let paths: Vec<PathBuf> = WalkDir::new(&self.path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
            .map(|e| e.path().to_path_buf())
            .collect();

        // Parse notes in parallel using rayon
        let results: Vec<_> = paths
            .par_iter()
            .map(|path| Self::parse_note_static(path))
            .collect();

        // Insert results into HashMap (sequential, but fast)
        for result in results {
            match result {
                Ok(note) => {
                    let key = note.name.to_lowercase();
                    self.notes.insert(key, note);
                }
                Err((path, e)) => {
                    tracing::warn!("Failed to parse note {:?}: {}", path, e);
                }
            }
        }

        self.indexed = true;
        Ok(())
    }

    /// Add or update a single note (for incremental updates).
    ///
    /// This is more efficient than re-indexing the entire vault when
    /// only a single file has changed.
    pub fn upsert_note(&mut self, path: &Path) -> Result<(), VaultError> {
        if !path.exists() {
            return Err(VaultError::PathNotFound(path.to_path_buf()));
        }

        // Only process markdown files
        if !path.extension().is_some_and(|ext| ext == "md") {
            return Ok(());
        }

        match Self::parse_note_static(path) {
            Ok(note) => {
                let key = note.name.to_lowercase();
                self.notes.insert(key, note);
                Ok(())
            }
            Err((_, e)) => Err(e),
        }
    }

    /// Remove a note from the index by name.
    ///
    /// Used for incremental updates when a file is deleted.
    pub fn remove_note(&mut self, name: &str) {
        self.notes.remove(&name.to_lowercase());
    }

    /// Remove a note from the index by path.
    ///
    /// Used for incremental updates when a file is deleted.
    pub fn remove_note_by_path(&mut self, path: &Path) {
        if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
            self.remove_note(name);
        }
    }

    /// Check if a note needs re-indexing based on modification time.
    pub fn note_needs_update(&self, path: &Path) -> bool {
        let name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => return true,
        };

        match self.get_note(name) {
            Some(note) => {
                // Check if file modification time is newer
                match fs::metadata(path).and_then(|m| m.modified()) {
                    Ok(mtime) => mtime > note.modified,
                    Err(_) => true, // Re-index if we can't get metadata
                }
            }
            None => true, // Note doesn't exist, needs indexing
        }
    }

    /// Parse a single note file (static version for parallel processing).
    ///
    /// Returns the parsed note or an error tuple with the path for logging.
    fn parse_note_static(path: &Path) -> Result<Note, (PathBuf, VaultError)> {
        let content = fs::read_to_string(path).map_err(|e| {
            (
                path.to_path_buf(),
                VaultError::ReadError {
                    path: path.to_path_buf(),
                    source: e,
                },
            )
        })?;

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string();

        let modified = fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::now());

        let parse_result = parse_markdown(&content);

        Ok(Note {
            name,
            path: path.to_path_buf(),
            title: parse_result.title,
            links: parse_result.links,
            headings: parse_result.headings,
            modified,
        })
    }

    /// Get a note by name (case-insensitive).
    pub fn get_note(&self, name: &str) -> Option<&Note> {
        self.notes.get(&name.to_lowercase())
    }

    /// Get all notes in the vault.
    pub fn notes(&self) -> impl Iterator<Item = &Note> {
        self.notes.values()
    }

    /// Get the number of notes in the vault.
    #[allow(dead_code)]
    pub fn note_count(&self) -> usize {
        self.notes.len()
    }

    /// Get all note names.
    #[allow(dead_code)]
    pub fn note_names(&self) -> Vec<&str> {
        self.notes.values().map(|n| n.name.as_str()).collect()
    }

    /// Check if a note exists (case-insensitive).
    pub fn note_exists(&self, name: &str) -> bool {
        self.notes.contains_key(&name.to_lowercase())
    }

    /// Read the raw content of a note.
    pub fn read_note_content(&self, name: &str) -> Result<String, VaultError> {
        let note = self
            .get_note(name)
            .ok_or_else(|| VaultError::NoteNotFound(name.to_string()))?;

        fs::read_to_string(&note.path).map_err(|e| VaultError::ReadError {
            path: note.path.clone(),
            source: e,
        })
    }

    // =========================================================================
    // Write Operations
    // =========================================================================

    /// Validate a note name for safety and convention.
    ///
    /// Rules:
    /// - Must not be empty
    /// - Must not contain path separators (/, \)
    /// - Must not contain invalid filename characters (:, *, ?, <, >, |)
    /// - Must not start with a dot (hidden files)
    /// - Must be reasonable length (1-200 chars)
    pub fn validate_note_name(name: &str) -> Result<(), VaultError> {
        let name = name.trim();

        if name.is_empty() {
            return Err(VaultError::InvalidNoteName(
                "Note name cannot be empty".to_string(),
            ));
        }

        if name.len() > 200 {
            return Err(VaultError::InvalidNoteName(
                "Note name must be 200 characters or less".to_string(),
            ));
        }

        if name.starts_with('.') {
            return Err(VaultError::InvalidNoteName(
                "Note name cannot start with a dot".to_string(),
            ));
        }

        // Check for path traversal and invalid characters
        let invalid_chars = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
        for c in invalid_chars {
            if name.contains(c) {
                return Err(VaultError::InvalidNoteName(format!(
                    "Note name cannot contain '{}'",
                    c
                )));
            }
        }

        // Check for path traversal patterns
        if name.contains("..") {
            return Err(VaultError::InvalidNoteName(
                "Note name cannot contain '..'".to_string(),
            ));
        }

        Ok(())
    }

    /// Get the file path for a note (doesn't check if it exists).
    pub fn note_path(&self, name: &str) -> PathBuf {
        self.path.join(format!("{}.md", name))
    }

    /// Write a new note to the vault.
    ///
    /// Returns the path to the created file.
    /// Fails if a note with this name already exists.
    pub fn write_note(&mut self, name: &str, content: &str) -> Result<PathBuf, VaultError> {
        Self::validate_note_name(name)?;

        let path = self.note_path(name);

        if path.exists() {
            return Err(VaultError::NoteAlreadyExists(name.to_string()));
        }

        fs::write(&path, content).map_err(|e| VaultError::WriteError {
            path: path.clone(),
            source: e,
        })?;

        // Update the index
        self.upsert_note(&path)?;

        Ok(path)
    }

    /// Overwrite an existing note with new content.
    ///
    /// Returns the path to the updated file.
    /// Fails if the note doesn't exist.
    pub fn overwrite_note(&mut self, name: &str, content: &str) -> Result<PathBuf, VaultError> {
        Self::validate_note_name(name)?;

        let path = self.note_path(name);

        if !path.exists() {
            return Err(VaultError::NoteNotFound(name.to_string()));
        }

        fs::write(&path, content).map_err(|e| VaultError::WriteError {
            path: path.clone(),
            source: e,
        })?;

        // Update the index
        self.upsert_note(&path)?;

        Ok(path)
    }

    /// Delete a note from the vault.
    ///
    /// Removes both the file and the index entry.
    /// Fails if the note doesn't exist.
    pub fn delete_note_file(&mut self, name: &str) -> Result<(), VaultError> {
        Self::validate_note_name(name)?;

        let path = self.note_path(name);

        if !path.exists() {
            return Err(VaultError::NoteNotFound(name.to_string()));
        }

        fs::remove_file(&path).map_err(|e| VaultError::DeleteError {
            path: path.clone(),
            source: e,
        })?;

        // Remove from index
        self.remove_note(name);

        Ok(())
    }

    /// Read the AGENTS.md guidelines file from the vault root.
    ///
    /// Returns None if the file doesn't exist.
    pub fn read_guidelines(&self) -> Option<String> {
        let path = self.path.join("AGENTS.md");
        fs::read_to_string(&path).ok()
    }

    /// Get backlinks to a note (notes that link to it).
    pub fn backlinks(&self, name: &str) -> Vec<&Note> {
        let target_lower = name.to_lowercase();
        self.notes
            .values()
            .filter(|note| {
                note.links
                    .iter()
                    .any(|link| link.target.to_lowercase() == target_lower)
            })
            .collect()
    }

    /// Find broken links (links to non-existent notes).
    pub fn broken_links(&self) -> Vec<BrokenLink> {
        let mut broken = Vec::new();

        for note in self.notes.values() {
            for link in &note.links {
                // Skip same-file heading links
                if link.target.is_empty() {
                    continue;
                }

                if !self.note_exists(&link.target) {
                    broken.push(BrokenLink {
                        source: note.name.clone(),
                        target: link.target.clone(),
                    });
                }
            }
        }

        broken
    }

    /// Find orphan notes (notes with no incoming or outgoing links).
    #[allow(dead_code)]
    pub fn orphans(&self) -> Vec<&Note> {
        self.notes
            .values()
            .filter(|note| {
                // No outgoing links (excluding same-file links)
                let has_outgoing = note
                    .links
                    .iter()
                    .any(|l| !l.target.is_empty() && self.note_exists(&l.target));

                // No incoming links
                let has_incoming = !self.backlinks(&note.name).is_empty();

                !has_outgoing && !has_incoming
            })
            .collect()
    }
}

/// A broken link (link to a non-existent note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenLink {
    /// The note containing the broken link.
    pub source: String,
    /// The non-existent target.
    pub target: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    fn create_test_vault() -> (tempfile::TempDir, Vault) {
        let dir = tempdir().unwrap();

        // Create test notes
        let notes = [
            (
                "Note A.md",
                "# Note A\n\nThis links to [[Note B]] and [[Note C]].",
            ),
            ("Note B.md", "# Note B\n\nThis links back to [[Note A]]."),
            (
                "Note C.md",
                "# Note C\n\nThis has a [[Broken Link]] to nowhere.",
            ),
            ("Orphan.md", "# Orphan Note\n\nThis note has no links."),
        ];

        for (name, content) in notes {
            let path = dir.path().join(name);
            let mut file = File::create(&path).unwrap();
            file.write_all(content.as_bytes()).unwrap();
        }

        let vault = Vault::new(dir.path());
        (dir, vault)
    }

    #[test]
    fn test_vault_index() {
        let (_dir, mut vault) = create_test_vault();

        vault.index().unwrap();

        assert!(vault.is_indexed());
        assert_eq!(vault.note_count(), 4);
    }

    #[test]
    fn test_vault_get_note() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();

        let note = vault.get_note("Note A").unwrap();
        assert_eq!(note.name, "Note A");
        assert_eq!(note.title, Some("Note A".to_string()));

        // Case insensitive
        let note2 = vault.get_note("note a").unwrap();
        assert_eq!(note2.name, "Note A");

        // Non-existent
        assert!(vault.get_note("Does Not Exist").is_none());
    }

    #[test]
    fn test_vault_backlinks() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();

        let backlinks = vault.backlinks("Note B");
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].name, "Note A");

        let backlinks_a = vault.backlinks("Note A");
        assert_eq!(backlinks_a.len(), 1);
        assert_eq!(backlinks_a[0].name, "Note B");
    }

    #[test]
    fn test_vault_broken_links() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();

        let broken = vault.broken_links();
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].source, "Note C");
        assert_eq!(broken[0].target, "Broken Link");
    }

    #[test]
    fn test_vault_orphans() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();

        let orphans = vault.orphans();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].name, "Orphan");
    }

    #[test]
    fn test_vault_read_content() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();

        let content = vault.read_note_content("Note A").unwrap();
        assert!(content.contains("# Note A"));
        assert!(content.contains("[[Note B]]"));
    }

    #[test]
    fn test_vault_nonexistent_path() {
        let mut vault = Vault::new("/nonexistent/path");
        let result = vault.index();
        assert!(matches!(result, Err(VaultError::PathNotFound(_))));
    }

    #[test]
    fn test_vault_note_names() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();

        let names = vault.note_names();
        assert_eq!(names.len(), 4);
        assert!(names.contains(&"Note A"));
        assert!(names.contains(&"Note B"));
        assert!(names.contains(&"Note C"));
        assert!(names.contains(&"Orphan"));
    }

    #[test]
    fn test_vault_upsert_note() {
        let (dir, mut vault) = create_test_vault();
        vault.index().unwrap();
        assert_eq!(vault.note_count(), 4);

        // Add a new note
        let new_path = dir.path().join("New Note.md");
        let mut file = File::create(&new_path).unwrap();
        file.write_all(b"# New Note\n\nThis is a new note.")
            .unwrap();

        vault.upsert_note(&new_path).unwrap();
        assert_eq!(vault.note_count(), 5);
        assert!(vault.get_note("New Note").is_some());
    }

    #[test]
    fn test_vault_remove_note() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();
        assert_eq!(vault.note_count(), 4);

        vault.remove_note("Note A");
        assert_eq!(vault.note_count(), 3);
        assert!(vault.get_note("Note A").is_none());
    }

    #[test]
    fn test_vault_remove_note_by_path() {
        let (dir, mut vault) = create_test_vault();
        vault.index().unwrap();
        assert_eq!(vault.note_count(), 4);

        let path = dir.path().join("Note A.md");
        vault.remove_note_by_path(&path);
        assert_eq!(vault.note_count(), 3);
        assert!(vault.get_note("Note A").is_none());
    }

    #[test]
    fn test_vault_note_needs_update() {
        let (dir, mut vault) = create_test_vault();
        vault.index().unwrap();

        let path = dir.path().join("Note A.md");

        // Note was just indexed, shouldn't need update
        assert!(!vault.note_needs_update(&path));

        // Non-existent note should need update
        let new_path = dir.path().join("New Note.md");
        assert!(vault.note_needs_update(&new_path));
    }

    // =========================================================================
    // Write Operation Tests
    // =========================================================================

    #[test]
    fn test_validate_note_name_valid() {
        assert!(Vault::validate_note_name("Simple Note").is_ok());
        assert!(Vault::validate_note_name("Note-With-Dashes").is_ok());
        assert!(Vault::validate_note_name("Note_With_Underscores").is_ok());
        assert!(Vault::validate_note_name("Note 123").is_ok());
        assert!(Vault::validate_note_name("日本語ノート").is_ok()); // Unicode
    }

    #[test]
    fn test_validate_note_name_invalid() {
        // Empty
        assert!(Vault::validate_note_name("").is_err());
        assert!(Vault::validate_note_name("   ").is_err());

        // Path traversal
        assert!(Vault::validate_note_name("../etc/passwd").is_err());
        assert!(Vault::validate_note_name("foo/bar").is_err());
        assert!(Vault::validate_note_name("foo\\bar").is_err());
        assert!(Vault::validate_note_name("..").is_err());

        // Invalid characters
        assert!(Vault::validate_note_name("Note:Invalid").is_err());
        assert!(Vault::validate_note_name("Note*Invalid").is_err());
        assert!(Vault::validate_note_name("Note?Invalid").is_err());
        assert!(Vault::validate_note_name("Note<Invalid").is_err());
        assert!(Vault::validate_note_name("Note>Invalid").is_err());
        assert!(Vault::validate_note_name("Note|Invalid").is_err());

        // Hidden files
        assert!(Vault::validate_note_name(".hidden").is_err());

        // Too long
        let long_name = "a".repeat(201);
        assert!(Vault::validate_note_name(&long_name).is_err());
    }

    #[test]
    fn test_vault_write_note() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();
        assert_eq!(vault.note_count(), 4);

        let path = vault
            .write_note("New Note", "# New Note\n\nContent here.")
            .unwrap();

        assert!(path.exists());
        assert_eq!(vault.note_count(), 5);
        assert!(vault.get_note("New Note").is_some());

        let content = vault.read_note_content("New Note").unwrap();
        assert!(content.contains("# New Note"));
    }

    #[test]
    fn test_vault_write_note_already_exists() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();

        let result = vault.write_note("Note A", "# Note A\n\nNew content.");
        assert!(matches!(result, Err(VaultError::NoteAlreadyExists(_))));
    }

    #[test]
    fn test_vault_overwrite_note() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();

        let original = vault.read_note_content("Note A").unwrap();
        assert!(original.contains("[[Note B]]"));

        vault
            .overwrite_note("Note A", "# Note A\n\nCompletely new content.")
            .unwrap();

        let updated = vault.read_note_content("Note A").unwrap();
        assert!(updated.contains("Completely new content"));
        assert!(!updated.contains("[[Note B]]"));
    }

    #[test]
    fn test_vault_overwrite_note_not_found() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();

        let result = vault.overwrite_note("Does Not Exist", "# New\n\nContent.");
        assert!(matches!(result, Err(VaultError::NoteNotFound(_))));
    }

    #[test]
    fn test_vault_delete_note_file() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();
        assert_eq!(vault.note_count(), 4);

        let path = vault.note_path("Note A");
        assert!(path.exists());

        vault.delete_note_file("Note A").unwrap();

        assert!(!path.exists());
        assert_eq!(vault.note_count(), 3);
        assert!(vault.get_note("Note A").is_none());
    }

    #[test]
    fn test_vault_delete_note_not_found() {
        let (_dir, mut vault) = create_test_vault();
        vault.index().unwrap();

        let result = vault.delete_note_file("Does Not Exist");
        assert!(matches!(result, Err(VaultError::NoteNotFound(_))));
    }

    #[test]
    fn test_vault_read_guidelines() {
        let (dir, vault) = create_test_vault();

        // No guidelines file initially
        assert!(vault.read_guidelines().is_none());

        // Create AGENTS.md
        let guidelines_path = dir.path().join("AGENTS.md");
        let mut file = File::create(&guidelines_path).unwrap();
        file.write_all(b"# Writing Guidelines\n\nFollow these rules.")
            .unwrap();

        // Now it should be readable
        let guidelines = vault.read_guidelines().unwrap();
        assert!(guidelines.contains("Writing Guidelines"));
    }
}
