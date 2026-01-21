use std::path::Path;

use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::Searcher;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use walkdir::WalkDir;

/// Errors that can occur during search operations.
#[derive(Debug, Error)]
pub enum SearchError {
    #[error("Invalid search pattern: {0}")]
    InvalidPattern(#[from] grep_regex::Error),

    #[error("Search failed: {0}")]
    SearchFailed(#[from] std::io::Error),

    #[error("Directory walk error: {0}")]
    WalkError(#[from] walkdir::Error),
}

/// A single match within a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Match {
    /// Line number (1-based).
    pub line_number: u64,
    /// The matching line content (trimmed).
    pub line_content: String,
}

/// Search result for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The note name (filename without .md).
    pub note_name: String,
    /// Relative path to the file.
    pub path: String,
    /// Matches found in the file.
    pub matches: Vec<Match>,
}

/// Search options.
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// Treat the query as a regex pattern.
    pub regex: bool,
    /// Case-sensitive search.
    pub case_sensitive: bool,
    /// Maximum number of results to return.
    pub limit: Option<usize>,
    /// Number of context lines before/after match (reserved for future use).
    #[allow(dead_code)]
    pub context_lines: usize,
}

/// Search for a pattern across all markdown files in the vault.
pub fn search(
    vault_path: &Path,
    query: &str,
    options: &SearchOptions,
) -> Result<Vec<SearchResult>, SearchError> {
    // Build the regex pattern
    let pattern = if options.regex {
        query.to_string()
    } else {
        // Escape special regex characters for literal search
        regex::escape(query)
    };

    // Add case insensitivity if needed
    let pattern = if options.case_sensitive {
        pattern
    } else {
        format!("(?i){}", pattern)
    };

    let matcher = RegexMatcher::new(&pattern)?;
    let mut results = Vec::new();
    let limit = options.limit.unwrap_or(usize::MAX);

    for entry in WalkDir::new(vault_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if results.len() >= limit {
            break;
        }

        let path = entry.path();

        // Only search markdown files
        if !path.extension().is_some_and(|ext| ext == "md") {
            continue;
        }

        let mut file_matches = Vec::new();

        Searcher::new().search_path(
            &matcher,
            path,
            UTF8(|line_num, line| {
                file_matches.push(Match {
                    line_number: line_num,
                    line_content: line.trim().to_string(),
                });
                Ok(true)
            }),
        )?;

        if !file_matches.is_empty() {
            let note_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            let relative_path = path
                .strip_prefix(vault_path)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            results.push(SearchResult {
                note_name,
                path: relative_path,
                matches: file_matches,
            });

            if results.len() >= limit {
                break;
            }
        }
    }

    Ok(results)
}

/// Search for notes that link to a specific target.
#[allow(dead_code)]
pub fn search_backlinks(vault_path: &Path, target: &str) -> Result<Vec<SearchResult>, SearchError> {
    // Search for wiki link patterns pointing to the target
    // Matches [[Target]], [[Target|alias]], [[Target#heading]]
    let escaped_target = regex::escape(target);
    let pattern = format!(r"\[\[{}\s*(?:[|#][^\]]+)?\]\]", escaped_target);

    search(
        vault_path,
        &pattern,
        &SearchOptions {
            regex: true,
            case_sensitive: false,
            limit: None,
            context_lines: 0,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    fn create_test_vault() -> tempfile::TempDir {
        let dir = tempdir().unwrap();

        let notes = [
            (
                "Deep Work.md",
                r#"# Deep Work

Deep work is focused concentration.

## Benefits

It helps with productivity and learning.
"#,
            ),
            (
                "Flow State.md",
                r#"# Flow State

Flow is related to [[Deep Work]].

Deep concentration leads to flow state.
"#,
            ),
            (
                "Productivity.md",
                r#"# Productivity

Productivity requires [[Deep Work]] and [[Flow State]].

Being productive means getting things done.
"#,
            ),
        ];

        for (name, content) in notes {
            let path = dir.path().join(name);
            let mut file = File::create(&path).unwrap();
            file.write_all(content.as_bytes()).unwrap();
        }

        dir
    }

    #[test]
    fn test_search_literal() {
        let dir = create_test_vault();

        let results = search(dir.path(), "concentration", &SearchOptions::default()).unwrap();

        assert_eq!(results.len(), 2);

        let names: Vec<&str> = results.iter().map(|r| r.note_name.as_str()).collect();
        assert!(names.contains(&"Deep Work"));
        assert!(names.contains(&"Flow State"));
    }

    #[test]
    fn test_search_case_insensitive() {
        let dir = create_test_vault();

        let results = search(
            dir.path(),
            "DEEP WORK",
            &SearchOptions {
                case_sensitive: false,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(!results.is_empty());
    }

    #[test]
    fn test_search_case_sensitive() {
        let dir = create_test_vault();

        let results = search(
            dir.path(),
            "DEEP WORK",
            &SearchOptions {
                case_sensitive: true,
                ..Default::default()
            },
        )
        .unwrap();

        // No matches for uppercase when content is mixed case
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_regex() {
        let dir = create_test_vault();

        let results = search(
            dir.path(),
            r"product\w+",
            &SearchOptions {
                regex: true,
                case_sensitive: false,
                ..Default::default()
            },
        )
        .unwrap();

        // Should match "Productivity" in Productivity.md and "productivity" in Deep Work.md
        assert_eq!(results.len(), 2);
        let note_names: Vec<&str> = results.iter().map(|r| r.note_name.as_str()).collect();
        assert!(note_names.contains(&"Productivity"));
        assert!(note_names.contains(&"Deep Work"));
    }

    #[test]
    fn test_search_with_limit() {
        let dir = create_test_vault();

        let results = search(
            dir.path(),
            "the",
            &SearchOptions {
                limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();

        assert!(results.len() <= 1);
    }

    #[test]
    fn test_search_backlinks() {
        let dir = create_test_vault();

        let results = search_backlinks(dir.path(), "Deep Work").unwrap();

        assert_eq!(results.len(), 2);

        let names: Vec<&str> = results.iter().map(|r| r.note_name.as_str()).collect();
        assert!(names.contains(&"Flow State"));
        assert!(names.contains(&"Productivity"));
    }

    #[test]
    fn test_search_no_results() {
        let dir = create_test_vault();

        let results = search(dir.path(), "xyznonexistent", &SearchOptions::default()).unwrap();

        assert!(results.is_empty());
    }

    #[test]
    fn test_search_special_characters() {
        let dir = create_test_vault();

        // Search for literal [[Deep Work]] - special chars should be escaped
        let results = search(
            dir.path(),
            "[[Deep Work]]",
            &SearchOptions {
                regex: false,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(results.len(), 2);
    }
}
