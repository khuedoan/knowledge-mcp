//! Content validation for notes.
//!
//! This module provides validation utilities to check note content against
//! Zettelkasten conventions and provide helpful warnings (not hard errors).

use rmcp::schemars::{self, JsonSchema};
use serde::Serialize;

/// Result of validating note content.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ValidationResult {
    /// Whether the note passed all validations (always true, since we only warn).
    pub valid: bool,
    /// List of validation warnings.
    pub warnings: Vec<ValidationWarning>,
}

impl ValidationResult {
    /// Create a new validation result with no warnings.
    pub fn ok() -> Self {
        Self {
            valid: true,
            warnings: Vec::new(),
        }
    }

    /// Add a warning to the result.
    pub fn warn(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.warnings.push(ValidationWarning {
            code: code.into(),
            message: message.into(),
            suggestion: None,
        });
    }

    /// Add a warning with a suggestion to the result.
    pub fn warn_with_suggestion(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) {
        self.warnings.push(ValidationWarning {
            code: code.into(),
            message: message.into(),
            suggestion: Some(suggestion.into()),
        });
    }

    /// Check if there are any warnings.
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}

/// A validation warning for note content.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ValidationWarning {
    /// A short code identifying the warning type.
    pub code: String,
    /// Human-readable description of the issue.
    pub message: String,
    /// Optional suggestion for how to fix the issue.
    pub suggestion: Option<String>,
}

/// Validate note content against Zettelkasten conventions.
///
/// This returns warnings (not errors) to help guide the agent toward
/// better note structure without blocking note creation.
pub fn validate_note_content(name: &str, content: &str) -> ValidationResult {
    let mut result = ValidationResult::ok();

    // Check for H1 heading (title)
    let has_h1 = content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("# ") && !trimmed.starts_with("## ")
    });

    if !has_h1 {
        result.warn_with_suggestion(
            "missing_title",
            "Note has no H1 heading",
            format!("Add '# {}' at the beginning of the note", name),
        );
    }

    // Check for wiki-links (connectivity is key in Zettelkasten)
    let has_wiki_links = content.contains("[[");

    if !has_wiki_links {
        result.warn_with_suggestion(
            "no_links",
            "Note has no wiki-links to other notes",
            "Consider linking to related concepts with [[Note Name]] syntax",
        );
    }

    // Check content length
    let char_count = content.chars().count();

    if char_count < 50 {
        result.warn_with_suggestion(
            "too_short",
            format!("Note is very short ({} characters)", char_count),
            "Atomic notes should still be self-contained with enough context to be understood on their own",
        );
    } else if char_count > 5000 {
        result.warn_with_suggestion(
            "too_long",
            format!("Note is quite long ({} characters)", char_count),
            "Consider splitting into multiple atomic notes, each focused on a single concept",
        );
    }

    // Check for empty content
    if content.trim().is_empty() {
        result.warn("empty_content", "Note content is empty");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_good_note() {
        let content = r#"# My Note

This is a well-structured note about [[Some Topic]].

## Details

More information here with a link to [[Another Note]].
"#;

        let result = validate_note_content("My Note", content);
        assert!(result.valid);
        assert!(!result.has_warnings());
    }

    #[test]
    fn test_validate_missing_title() {
        let content = r#"This note has no H1 heading.

It links to [[Other Note]] though.
"#;

        let result = validate_note_content("Test Note", content);
        assert!(result.valid); // Still valid, just has warnings
        assert!(result.has_warnings());
        assert!(result.warnings.iter().any(|w| w.code == "missing_title"));
    }

    #[test]
    fn test_validate_no_links() {
        let content = r#"# Isolated Note

This note doesn't link to anything else.
It's completely standalone.
"#;

        let result = validate_note_content("Isolated Note", content);
        assert!(result.valid);
        assert!(result.has_warnings());
        assert!(result.warnings.iter().any(|w| w.code == "no_links"));
    }

    #[test]
    fn test_validate_too_short() {
        let content = "# Short\n\nToo brief.";

        let result = validate_note_content("Short", content);
        assert!(result.valid);
        assert!(result.has_warnings());
        assert!(result.warnings.iter().any(|w| w.code == "too_short"));
    }

    #[test]
    fn test_validate_too_long() {
        let content = format!("# Long Note\n\n{}\n\n[[Link]]", "x".repeat(6000));

        let result = validate_note_content("Long Note", &content);
        assert!(result.valid);
        assert!(result.has_warnings());
        assert!(result.warnings.iter().any(|w| w.code == "too_long"));
    }

    #[test]
    fn test_validate_empty_content() {
        let result = validate_note_content("Empty", "");
        assert!(result.valid);
        assert!(result.has_warnings());
        assert!(result.warnings.iter().any(|w| w.code == "empty_content"));
    }

    #[test]
    fn test_validate_multiple_warnings() {
        let content = "No heading, no links, too short.";

        let result = validate_note_content("Test", content);
        assert!(result.valid);
        assert!(result.warnings.len() >= 2); // At least missing_title and no_links
    }
}
