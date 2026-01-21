use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::SystemTime;

/// A wiki-style link extracted from a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiLink {
    /// The target note name (e.g., "Deep Work" from `[[Deep Work]]`).
    pub target: String,
    /// Optional display text (e.g., "alias" from `[[Note|alias]]`).
    pub display: Option<String>,
    /// Optional heading reference (e.g., "section" from `[[Note#section]]`).
    pub heading: Option<String>,
}

impl WikiLink {
    /// Create a new wiki link with just a target.
    #[allow(dead_code)]
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            display: None,
            heading: None,
        }
    }

    /// Create a wiki link with a display alias.
    #[allow(dead_code)]
    pub fn with_display(target: impl Into<String>, display: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            display: Some(display.into()),
            heading: None,
        }
    }

    /// Create a wiki link with a heading reference.
    #[allow(dead_code)]
    pub fn with_heading(target: impl Into<String>, heading: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            display: None,
            heading: Some(heading.into()),
        }
    }
}

/// A heading extracted from a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    /// The heading level (1-6).
    pub level: u8,
    /// The heading text content.
    pub text: String,
}

impl Heading {
    pub fn new(level: u8, text: impl Into<String>) -> Self {
        Self {
            level,
            text: text.into(),
        }
    }
}

/// A note in the knowledge vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    /// The note name (filename without .md extension).
    pub name: String,
    /// Full path to the note file.
    pub path: PathBuf,
    /// The title (first H1 heading if present).
    pub title: Option<String>,
    /// Outgoing wiki links from this note.
    pub links: Vec<WikiLink>,
    /// Headings in the document.
    pub headings: Vec<Heading>,
    /// Last modification time.
    #[serde(with = "system_time_serde")]
    pub modified: SystemTime,
}

impl Note {
    /// Create a new note with basic information.
    #[allow(dead_code)]
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            title: None,
            links: Vec::new(),
            headings: Vec::new(),
            modified: SystemTime::now(),
        }
    }

    /// Get the display name (title if available, otherwise name).
    #[allow(dead_code)]
    pub fn display_name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }

    /// Get the names of all notes this note links to.
    #[allow(dead_code)]
    pub fn link_targets(&self) -> Vec<&str> {
        self.links.iter().map(|l| l.target.as_str()).collect()
    }
}

/// Serde helper for SystemTime serialization.
mod system_time_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = time.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wiki_link_new() {
        let link = WikiLink::new("Deep Work");
        assert_eq!(link.target, "Deep Work");
        assert!(link.display.is_none());
        assert!(link.heading.is_none());
    }

    #[test]
    fn test_wiki_link_with_display() {
        let link = WikiLink::with_display("Deep Work", "Cal Newport's book");
        assert_eq!(link.target, "Deep Work");
        assert_eq!(link.display, Some("Cal Newport's book".to_string()));
    }

    #[test]
    fn test_wiki_link_with_heading() {
        let link = WikiLink::with_heading("Deep Work", "Chapter 1");
        assert_eq!(link.target, "Deep Work");
        assert_eq!(link.heading, Some("Chapter 1".to_string()));
    }

    #[test]
    fn test_heading_new() {
        let heading = Heading::new(2, "Introduction");
        assert_eq!(heading.level, 2);
        assert_eq!(heading.text, "Introduction");
    }

    #[test]
    fn test_note_new() {
        let note = Note::new("Test Note", "/path/to/test.md");
        assert_eq!(note.name, "Test Note");
        assert_eq!(note.path, PathBuf::from("/path/to/test.md"));
        assert!(note.title.is_none());
        assert!(note.links.is_empty());
    }

    #[test]
    fn test_note_display_name() {
        let mut note = Note::new("test-note", "/path/to/test-note.md");
        assert_eq!(note.display_name(), "test-note");

        note.title = Some("My Test Note".to_string());
        assert_eq!(note.display_name(), "My Test Note");
    }

    #[test]
    fn test_note_link_targets() {
        let mut note = Note::new("Test", "/test.md");
        note.links = vec![
            WikiLink::new("Note A"),
            WikiLink::new("Note B"),
            WikiLink::with_display("Note C", "alias"),
        ];

        let targets = note.link_targets();
        assert_eq!(targets, vec!["Note A", "Note B", "Note C"]);
    }
}
