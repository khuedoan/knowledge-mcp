mod config;
mod graph;
mod search;
mod tools;
mod vault;

use rmcp::transport::stdio;
use rmcp::ServiceExt;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::config::Config;
use crate::tools::KnowledgeServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing with env filter
    // Set RUST_LOG=debug for verbose output
    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::from_default_env())
        .init();

    tracing::info!("Starting knowledge-mcp server");

    // Load configuration from environment
    let config = Config::from_env();
    tracing::info!("Vault path: {:?}", config.vault_path);

    // Create the server
    let server = KnowledgeServer::new(config);

    // Run the server over stdio
    let service = server.serve(stdio()).await?;

    // Wait for the service to complete
    service.waiting().await?;

    tracing::info!("Server stopped");
    Ok(())
}
