use std::path::PathBuf;

/// Configuration for the knowledge vault MCP server.
#[derive(Debug, Clone)]
pub struct Config {
    /// Path to the vault directory containing markdown notes.
    pub vault_path: PathBuf,
}

impl Config {
    /// Create a new configuration from environment variables.
    ///
    /// Uses `KNOWLEDGE_VAULT_PATH` environment variable if set,
    /// otherwise defaults to the current directory.
    pub fn from_env() -> Self {
        let vault_path = std::env::var("KNOWLEDGE_VAULT_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        Self { vault_path }
    }

    /// Create a new configuration with a specific vault path.
    #[allow(dead_code)]
    pub fn with_path(vault_path: impl Into<PathBuf>) -> Self {
        Self {
            vault_path: vault_path.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_with_path() {
        let config = Config::with_path("/tmp/vault");
        assert_eq!(config.vault_path, PathBuf::from("/tmp/vault"));
    }

    #[test]
    fn test_config_from_env_with_var() {
        std::env::set_var("KNOWLEDGE_VAULT_PATH", "/test/vault");
        let config = Config::from_env();
        assert_eq!(config.vault_path, PathBuf::from("/test/vault"));
        std::env::remove_var("KNOWLEDGE_VAULT_PATH");
    }
}
