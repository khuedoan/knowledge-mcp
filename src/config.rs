//! Configuration management for the knowledge vault MCP server.
//!
//! This module provides configuration loading from environment variables
//! with sensible defaults.

use std::path::PathBuf;

/// Default keywords that indicate potentially sensitive content.
const DEFAULT_SENSITIVE_KEYWORDS: &[&str] = &["salary"];

/// Trait for reading environment variables.
///
/// This abstraction allows for dependency injection in tests,
/// avoiding unsafe manipulation of global environment state.
pub trait EnvReader {
    /// Get the value of an environment variable.
    fn get_var(&self, key: &str) -> Option<String>;

    /// Get the current working directory.
    fn current_dir(&self) -> Option<PathBuf>;
}

/// Real environment reader that uses `std::env`.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealEnvReader;

impl EnvReader for RealEnvReader {
    fn get_var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn current_dir(&self) -> Option<PathBuf> {
        std::env::current_dir().ok()
    }
}

/// Configuration for the knowledge vault MCP server.
#[derive(Debug, Clone)]
pub struct Config {
    /// Path to the vault directory containing markdown notes.
    pub vault_path: PathBuf,
    /// Keywords that indicate potentially sensitive content.
    pub sensitive_keywords: Vec<String>,
}

impl Config {
    /// Create a new configuration from environment variables.
    ///
    /// Uses `KNOWLEDGE_VAULT_PATH` environment variable if set,
    /// otherwise defaults to the current directory.
    ///
    /// Uses `KNOWLEDGE_SENSITIVE_KEYWORDS` environment variable for sensitive
    /// keyword detection (comma-separated). Defaults to common sensitive terms.
    pub fn from_env() -> Self {
        Self::from_env_reader(&RealEnvReader)
    }

    /// Create a new configuration from a custom environment reader.
    ///
    /// This is primarily useful for testing without modifying global state.
    pub fn from_env_reader(reader: &impl EnvReader) -> Self {
        let vault_path = reader
            .get_var("KNOWLEDGE_VAULT_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| reader.current_dir().unwrap_or_else(|| PathBuf::from(".")));

        let sensitive_keywords = reader
            .get_var("KNOWLEDGE_SENSITIVE_KEYWORDS")
            .map(|s| {
                s.split(',')
                    .map(|k| k.trim().to_lowercase())
                    .filter(|k| !k.is_empty())
                    .collect()
            })
            .unwrap_or_else(|| {
                DEFAULT_SENSITIVE_KEYWORDS
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            });

        Self {
            vault_path,
            sensitive_keywords,
        }
    }

    /// Create a new configuration with a specific vault path.
    #[allow(dead_code)]
    pub fn with_path(vault_path: impl Into<PathBuf>) -> Self {
        Self {
            vault_path: vault_path.into(),
            sensitive_keywords: DEFAULT_SENSITIVE_KEYWORDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Mock environment reader for testing.
    struct MockEnvReader {
        vars: HashMap<String, String>,
        current_dir: Option<PathBuf>,
    }

    impl MockEnvReader {
        fn new() -> Self {
            Self {
                vars: HashMap::new(),
                current_dir: Some(PathBuf::from("/mock/cwd")),
            }
        }

        fn with_var(mut self, key: &str, value: &str) -> Self {
            self.vars.insert(key.to_string(), value.to_string());
            self
        }
    }

    impl EnvReader for MockEnvReader {
        fn get_var(&self, key: &str) -> Option<String> {
            self.vars.get(key).cloned()
        }

        fn current_dir(&self) -> Option<PathBuf> {
            self.current_dir.clone()
        }
    }

    #[test]
    fn test_config_with_path() {
        let config = Config::with_path("/tmp/vault");
        assert_eq!(config.vault_path, PathBuf::from("/tmp/vault"));
        assert!(!config.sensitive_keywords.is_empty());
    }

    #[test]
    fn test_config_from_env_with_vault_path() {
        let reader = MockEnvReader::new().with_var("KNOWLEDGE_VAULT_PATH", "/test/vault");
        let config = Config::from_env_reader(&reader);
        assert_eq!(config.vault_path, PathBuf::from("/test/vault"));
    }

    #[test]
    fn test_config_from_env_without_vault_path() {
        let reader = MockEnvReader::new();
        let config = Config::from_env_reader(&reader);
        // Should fall back to current_dir
        assert_eq!(config.vault_path, PathBuf::from("/mock/cwd"));
    }

    #[test]
    fn test_config_default_sensitive_keywords() {
        let config = Config::with_path("/tmp/vault");
        assert!(config.sensitive_keywords.contains(&"salary".to_string()));
    }

    #[test]
    fn test_config_custom_sensitive_keywords() {
        let reader = MockEnvReader::new().with_var("KNOWLEDGE_SENSITIVE_KEYWORDS", "foo, bar, baz");
        let config = Config::from_env_reader(&reader);
        assert_eq!(config.sensitive_keywords, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_config_empty_sensitive_keywords_filtered() {
        let reader = MockEnvReader::new().with_var("KNOWLEDGE_SENSITIVE_KEYWORDS", "foo, , bar");
        let config = Config::from_env_reader(&reader);
        assert_eq!(config.sensitive_keywords, vec!["foo", "bar"]);
    }
}
