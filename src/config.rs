//! Configuration management for the knowledge vault MCP server.
//!
//! This module provides configuration loading from environment variables
//! with sensible defaults.

use std::path::PathBuf;

/// Expand tilde (~) in paths to the user's home directory.
fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

/// Default keywords that indicate potentially sensitive content.
const DEFAULT_SENSITIVE_KEYWORDS: &[&str] = &["salary"];

/// Default content cache size (number of notes to cache).
const DEFAULT_CACHE_SIZE: usize = 500;

/// Default debounce duration for file watcher in milliseconds.
const DEFAULT_WATCHER_DEBOUNCE_MS: u64 = 500;

/// Default maximum content characters for embedding.
/// 2000 chars (~400 words) provides good semantic context for embeddings.
const DEFAULT_EMBEDDING_MAX_CHARS: usize = 2000;

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
    /// Content cache size (number of notes to cache in memory).
    pub cache_size: usize,
    /// Whether to enable file system watching for live updates.
    pub enable_watcher: bool,
    /// Debounce duration for file watcher in milliseconds.
    pub watcher_debounce_ms: u64,
    /// Whether to enable semantic search with embeddings.
    pub enable_embeddings: bool,
    /// Maximum content characters to include in embeddings.
    pub embedding_max_chars: usize,
    /// Cache directory for embeddings and model files.
    pub cache_dir: PathBuf,
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
            .map(|s| expand_tilde(&s))
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

        let cache_size = reader
            .get_var("KNOWLEDGE_CACHE_SIZE")
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_CACHE_SIZE);

        let enable_watcher = reader
            .get_var("KNOWLEDGE_ENABLE_WATCHER")
            .map(|s| s.to_lowercase() != "false" && s != "0")
            .unwrap_or(true);

        let watcher_debounce_ms = reader
            .get_var("KNOWLEDGE_WATCHER_DEBOUNCE_MS")
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_WATCHER_DEBOUNCE_MS);

        let enable_embeddings = reader
            .get_var("KNOWLEDGE_ENABLE_EMBEDDINGS")
            .map(|s| s.to_lowercase() != "false" && s != "0")
            .unwrap_or(true);

        let embedding_max_chars = reader
            .get_var("KNOWLEDGE_EMBEDDING_MAX_CHARS")
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_EMBEDDING_MAX_CHARS);

        let cache_dir = reader
            .get_var("KNOWLEDGE_CACHE_DIR")
            .map(|s| expand_tilde(&s))
            .unwrap_or_else(|| {
                dirs::cache_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("knowledge-mcp")
            });

        Self {
            vault_path,
            sensitive_keywords,
            cache_size,
            enable_watcher,
            watcher_debounce_ms,
            enable_embeddings,
            embedding_max_chars,
            cache_dir,
        }
    }

    /// Create a new configuration with a specific vault path.
    #[allow(dead_code)]
    pub fn with_path(vault_path: impl Into<PathBuf>) -> Self {
        let vault_path = vault_path.into();
        Self {
            vault_path,
            sensitive_keywords: DEFAULT_SENSITIVE_KEYWORDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            cache_size: DEFAULT_CACHE_SIZE,
            enable_watcher: true,
            watcher_debounce_ms: DEFAULT_WATCHER_DEBOUNCE_MS,
            enable_embeddings: true,
            embedding_max_chars: DEFAULT_EMBEDDING_MAX_CHARS,
            cache_dir: dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("knowledge-mcp"),
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

    #[test]
    fn test_config_default_cache_settings() {
        let config = Config::with_path("/tmp/vault");
        assert_eq!(config.cache_size, DEFAULT_CACHE_SIZE);
        assert!(config.enable_watcher);
        assert_eq!(config.watcher_debounce_ms, DEFAULT_WATCHER_DEBOUNCE_MS);
        assert!(config.enable_embeddings);
        assert_eq!(config.embedding_max_chars, DEFAULT_EMBEDDING_MAX_CHARS);
    }

    #[test]
    fn test_config_custom_cache_size() {
        let reader = MockEnvReader::new().with_var("KNOWLEDGE_CACHE_SIZE", "1000");
        let config = Config::from_env_reader(&reader);
        assert_eq!(config.cache_size, 1000);
    }

    #[test]
    fn test_config_disable_watcher() {
        let reader = MockEnvReader::new().with_var("KNOWLEDGE_ENABLE_WATCHER", "false");
        let config = Config::from_env_reader(&reader);
        assert!(!config.enable_watcher);

        let reader2 = MockEnvReader::new().with_var("KNOWLEDGE_ENABLE_WATCHER", "0");
        let config2 = Config::from_env_reader(&reader2);
        assert!(!config2.enable_watcher);
    }

    #[test]
    fn test_config_disable_embeddings() {
        let reader = MockEnvReader::new().with_var("KNOWLEDGE_ENABLE_EMBEDDINGS", "false");
        let config = Config::from_env_reader(&reader);
        assert!(!config.enable_embeddings);
    }

    #[test]
    fn test_config_custom_cache_dir() {
        let reader = MockEnvReader::new().with_var("KNOWLEDGE_CACHE_DIR", "/custom/cache");
        let config = Config::from_env_reader(&reader);
        assert_eq!(config.cache_dir, PathBuf::from("/custom/cache"));
    }

    #[test]
    fn test_expand_tilde_with_path() {
        let expanded = expand_tilde("~/Documents/notes");
        let home = dirs::home_dir().unwrap();
        assert_eq!(expanded, home.join("Documents/notes"));
    }

    #[test]
    fn test_expand_tilde_only() {
        let expanded = expand_tilde("~");
        let home = dirs::home_dir().unwrap();
        assert_eq!(expanded, home);
    }

    #[test]
    fn test_expand_tilde_absolute_path() {
        let expanded = expand_tilde("/absolute/path");
        assert_eq!(expanded, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_expand_tilde_relative_path() {
        let expanded = expand_tilde("relative/path");
        assert_eq!(expanded, PathBuf::from("relative/path"));
    }

    #[test]
    fn test_config_vault_path_tilde_expansion() {
        let reader = MockEnvReader::new().with_var("KNOWLEDGE_VAULT_PATH", "~/Documents/notes");
        let config = Config::from_env_reader(&reader);
        let home = dirs::home_dir().unwrap();
        assert_eq!(config.vault_path, home.join("Documents/notes"));
    }

    #[test]
    fn test_config_cache_dir_tilde_expansion() {
        let reader = MockEnvReader::new().with_var("KNOWLEDGE_CACHE_DIR", "~/.cache/knowledge");
        let config = Config::from_env_reader(&reader);
        let home = dirs::home_dir().unwrap();
        assert_eq!(config.cache_dir, home.join(".cache/knowledge"));
    }
}
