use knowledge_mcp::{Config, KnowledgeServer};
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

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
    tracing::info!(
        "Features: watcher={}, embeddings=enabled",
        config.enable_watcher
    );

    // Create the server
    let server = KnowledgeServer::new(config.clone());

    // Initialize the server (index vault, setup embeddings)
    tracing::info!("Initializing server...");
    if let Err(e) = server.initialize().await {
        tracing::error!("Failed to initialize server: {}", e);
        // Continue anyway - basic functionality will still work
    }

    // Start file watcher if enabled
    if config.enable_watcher {
        match server.start_watcher() {
            Ok(_) => {
                // The watcher task runs in the background until the process exits.
                // We intentionally don't store the handle - the task will be
                // cancelled automatically when the runtime shuts down.
                tracing::info!("File watcher started");
            }
            Err(e) => {
                tracing::warn!("Failed to start file watcher: {}", e);
            }
        }
    }

    // Run the server over stdio
    let service = server.serve(stdio()).await?;

    // Wait for the service to complete
    service.waiting().await?;

    tracing::info!("Server stopped");
    Ok(())
}
