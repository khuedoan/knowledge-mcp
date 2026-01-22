//! MCP tool implementations for the knowledge vault server.
//!
//! This module provides the main `KnowledgeServer` handler and all MCP tools
//! for interacting with a knowledge vault of markdown notes.
//!
//! Features:
//! - Content caching with modification time tracking
//! - File system watching for live vault updates
//! - Semantic search using local embeddings

use std::sync::Arc;

use rmcp::{
    ServerHandler,
    handler::server::{tool::ToolCallContext, tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, // Import the crate so derive macro can find it
    service::{Peer, RequestContext, RoleServer},
    tool,
    tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::cache::ContentCache;
use crate::config::Config;
use crate::embedding::{EmbeddingConfig, EmbeddingIndex, vault_id_from_path};
use crate::filter::SensitiveDataFilter;
use crate::graph::KnowledgeGraph;
use crate::search::{self, SearchOptions};
use crate::vault::{BrokenLink, Vault};
use crate::watcher::{FileWatcher, VaultChange};

/// The knowledge vault MCP server handler.
#[derive(Clone)]
pub struct KnowledgeServer {
    vault: Arc<RwLock<Vault>>,
    /// Cached knowledge graph, invalidated when vault is re-indexed.
    graph_cache: Arc<RwLock<Option<KnowledgeGraph>>>,
    /// Content cache for note contents.
    content_cache: Arc<RwLock<ContentCache>>,
    /// Embedding index for semantic search.
    embedding_index: Arc<RwLock<Option<EmbeddingIndex>>>,
    /// File watcher for live updates (if enabled).
    #[allow(dead_code)]
    watcher: Option<Arc<FileWatcher>>,
    config: Config,
    filter: SensitiveDataFilter,
    tool_router: ToolRouter<Self>,
}

impl KnowledgeServer {
    /// Create a new knowledge server with the given configuration.
    pub fn new(config: Config) -> Self {
        let vault = Vault::new(&config.vault_path);
        let filter = SensitiveDataFilter::new(config.sensitive_keywords.clone());
        let content_cache = ContentCache::new(config.cache_size);

        Self {
            vault: Arc::new(RwLock::new(vault)),
            graph_cache: Arc::new(RwLock::new(None)),
            content_cache: Arc::new(RwLock::new(content_cache)),
            embedding_index: Arc::new(RwLock::new(None)),
            watcher: None,
            config,
            filter,
            tool_router: Self::tool_router(),
        }
    }

    /// Initialize the server: index vault, setup embeddings, start watcher.
    ///
    /// This should be called after creating the server but before serving requests.
    pub async fn initialize(&self) -> Result<(), String> {
        // Index the vault
        self.ensure_indexed()
            .await
            .map_err(|e| format!("Failed to index vault: {}", e.message))?;

        // Initialize embeddings if enabled
        if self.config.enable_embeddings {
            self.initialize_embeddings()
                .await
                .map_err(|e| format!("Failed to initialize embeddings: {}", e))?;
        }

        Ok(())
    }

    /// Initialize the embedding index and embed all notes.
    async fn initialize_embeddings(&self) -> Result<(), String> {
        let vault_id = vault_id_from_path(&self.config.vault_path);

        let embedding_config = EmbeddingConfig {
            max_content_chars: self.config.embedding_max_chars,
            include_headings: true,
            cache_dir: self.config.cache_dir.clone(),
        };

        // Load or create embedding index
        let mut index = EmbeddingIndex::load_or_create(&vault_id, embedding_config)
            .map_err(|e| e.to_string())?;

        // Embed all notes
        let vault = self.vault.read().await;
        let mut notes_to_embed = Vec::new();

        for note in vault.notes() {
            // Read content using cache
            let content = {
                let mut cache = self.content_cache.write().await;
                cache
                    .get_or_read(&note.name, &note.path)
                    .map_err(|e| e.to_string())?
            };

            if index.needs_update(&note.name, &content) {
                notes_to_embed.push((note, content));
            }
        }

        if !notes_to_embed.is_empty() {
            tracing::info!("Embedding {} notes...", notes_to_embed.len());

            // Convert to references for batch embedding
            let refs: Vec<_> = notes_to_embed
                .iter()
                .map(|(note, content)| (*note, content.as_str()))
                .collect();

            let count = index.embed_notes_batch(refs).map_err(|e| e.to_string())?;
            tracing::info!("Embedded {} notes", count);

            // Save embeddings to cache
            index.save(&vault_id).map_err(|e| e.to_string())?;
        }

        // Store the index
        let mut embedding_index = self.embedding_index.write().await;
        *embedding_index = Some(index);

        Ok(())
    }

    /// Start the file watcher for live updates.
    ///
    /// Returns a handle that can be used to stop the watcher.
    pub fn start_watcher(&self) -> Result<tokio::task::JoinHandle<()>, String> {
        if !self.config.enable_watcher {
            return Err("File watcher is disabled in configuration".to_string());
        }

        let watcher = FileWatcher::new(
            &self.config.vault_path,
            Some(self.config.watcher_debounce_ms),
        )
        .map_err(|e| e.to_string())?;

        let mut rx = watcher.subscribe();
        let vault = Arc::clone(&self.vault);
        let graph_cache = Arc::clone(&self.graph_cache);
        let content_cache = Arc::clone(&self.content_cache);
        let embedding_index = Arc::clone(&self.embedding_index);
        let config = self.config.clone();

        let handle = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(change) => {
                        tracing::debug!("Vault change detected: {:?}", change);

                        match &change {
                            VaultChange::Created(path) | VaultChange::Modified(path) => {
                                // Update vault index
                                let note_clone = {
                                    let mut vault = vault.write().await;
                                    if let Err(e) = vault.upsert_note(path) {
                                        tracing::warn!("Failed to update note: {}", e);
                                        continue;
                                    }

                                    // Get note name and clone note data if needed for embedding
                                    path.file_stem()
                                        .and_then(|s| s.to_str())
                                        .and_then(|name| vault.get_note(name).cloned())
                                };

                                // Invalidate content cache
                                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                                    let mut cache = content_cache.write().await;
                                    cache.invalidate(name);
                                }

                                // Re-embed if embeddings are enabled
                                if config.enable_embeddings {
                                    if let Some(note) = note_clone {
                                        let content = {
                                            let mut cache = content_cache.write().await;
                                            cache.get_or_read(&note.name, path).ok()
                                        };

                                        if let Some(content) = content {
                                            let mut index = embedding_index.write().await;
                                            if let Some(ref mut idx) = *index {
                                                // Use block_in_place to avoid blocking the async
                                                // runtime during ML inference
                                                let result = tokio::task::block_in_place(|| {
                                                    idx.embed_note(&note, &content)
                                                });
                                                if let Err(e) = result {
                                                    tracing::warn!("Failed to embed note: {}", e);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            VaultChange::Removed(path) => {
                                // Remove from vault index
                                let mut vault = vault.write().await;
                                vault.remove_note_by_path(path);

                                // Invalidate content cache
                                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                                    let mut cache = content_cache.write().await;
                                    cache.invalidate(name);

                                    // Remove embedding
                                    if config.enable_embeddings {
                                        let mut index = embedding_index.write().await;
                                        if let Some(ref mut idx) = *index {
                                            idx.remove(name);
                                        }
                                    }
                                }
                            }
                        }

                        // Invalidate graph cache on any change
                        let mut graph = graph_cache.write().await;
                        *graph = None;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Watcher lagged, missed {} events", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::info!("File watcher channel closed");
                        break;
                    }
                }
            }
        });

        Ok(handle)
    }

    /// Ensure the vault is indexed before operations.
    /// Invalidates the graph cache if re-indexing occurs.
    async fn ensure_indexed(&self) -> Result<(), ErrorData> {
        let mut vault = self.vault.write().await;
        let was_indexed = vault.is_indexed();
        vault
            .ensure_indexed()
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Invalidate graph cache if we just indexed
        if !was_indexed {
            let mut graph_cache = self.graph_cache.write().await;
            *graph_cache = None;
        }
        Ok(())
    }

    /// Get or build the knowledge graph from cache.
    async fn get_graph(&self) -> Result<KnowledgeGraph, ErrorData> {
        // Check if we have a cached graph
        {
            let cache = self.graph_cache.read().await;
            if let Some(ref graph) = *cache {
                return Ok(graph.clone());
            }
        }

        // Build and cache the graph
        let vault = self.vault.read().await;
        let graph = KnowledgeGraph::from_vault(&vault);

        let mut cache = self.graph_cache.write().await;
        *cache = Some(graph.clone());

        Ok(graph)
    }

    /// Check content for sensitive data and request user confirmation if needed.
    /// Returns Ok(Ok(())) if content should be returned, Ok(Err(message)) if blocked.
    async fn check_sensitive_content(
        &self,
        content: &str,
        peer: &Peer<RoleServer>,
    ) -> Result<Result<(), String>, ErrorData> {
        let check = self.filter.check(content);
        if !check.is_sensitive {
            return Ok(Ok(()));
        }

        let keywords = check.matched_keywords.join(", ");

        // Check if client supports elicitation
        if peer.supports_elicitation() {
            let message = format!(
                "This content may contain sensitive data ({}).\nDo you want to proceed?",
                keywords
            );

            // Build elicitation schema for a simple boolean confirmation
            let schema = ElicitationSchema::builder()
                .required_bool("confirm")
                .build()
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

            let params = CreateElicitationRequestParam {
                message,
                requested_schema: schema,
            };

            match peer.create_elicitation(params).await {
                Ok(result) => match result.action {
                    ElicitationAction::Accept => {
                        // Check if user confirmed
                        if let Some(content) = result.content
                            && let Some(confirmed) =
                                content.get("confirm").and_then(|v| v.as_bool())
                            && confirmed
                        {
                            return Ok(Ok(()));
                        }
                        Ok(Err(
                            "Access to sensitive content was not confirmed.".to_string()
                        ))
                    }
                    ElicitationAction::Decline => {
                        Ok(Err("User declined to view sensitive content.".to_string()))
                    }
                    ElicitationAction::Cancel => Ok(Err("Request cancelled by user.".to_string())),
                },
                Err(e) => {
                    tracing::warn!("Elicitation failed: {}", e);
                    Ok(Err(format!(
                        "Could not confirm access to sensitive content: {}",
                        e
                    )))
                }
            }
        } else {
            // Client doesn't support elicitation, block by default
            Ok(Err(format!(
                "This content may contain sensitive data ({}). Your client doesn't support confirmation dialogs. Access blocked.",
                keywords
            )))
        }
    }
}

// ============================================================================
// Tool Input/Output Types
// ============================================================================

/// Parameters for the search_notes tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchNotesParams {
    /// The search query (text or regex pattern).
    pub query: String,
    /// Treat query as a regex pattern (default: false).
    #[serde(default)]
    pub regex: bool,
    /// Case-sensitive search (default: false).
    #[serde(default)]
    pub case_sensitive: bool,
    /// Maximum number of results to return.
    pub limit: Option<usize>,
}

/// Parameters for the get_note tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetNoteParams {
    /// The note name (without .md extension).
    pub name: String,
    /// Include the full content in the response (default: true).
    #[serde(default = "default_true")]
    pub include_content: bool,
}

fn default_true() -> bool {
    true
}

/// Parameters for the list_notes tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListNotesParams {
    /// Sort order: "name", "modified", or "links" (default: "name").
    #[serde(default = "default_sort")]
    pub sort_by: String,
    /// Maximum number of notes to return.
    pub limit: Option<usize>,
}

fn default_sort() -> String {
    "name".to_string()
}

/// Parameters for the get_backlinks tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetBacklinksParams {
    /// The note name to find backlinks for.
    pub name: String,
}

/// Parameters for the get_links tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetLinksParams {
    /// The note name to get outgoing links from.
    pub name: String,
}

/// Parameters for the semantic_search tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SemanticSearchParams {
    /// The search query (natural language).
    pub query: String,
    /// Maximum number of results to return (default: 10).
    #[serde(default = "default_semantic_limit")]
    pub limit: usize,
    /// Minimum similarity threshold 0.0-1.0 (default: 0.3).
    #[serde(default = "default_threshold")]
    pub threshold: f32,
}

fn default_semantic_limit() -> usize {
    10
}

fn default_threshold() -> f32 {
    0.3
}

/// Parameters for the find_similar_notes tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindSimilarParams {
    /// The note name to find similar notes for.
    pub name: String,
    /// Maximum number of results to return (default: 10).
    #[serde(default = "default_semantic_limit")]
    pub limit: usize,
}

/// Response for semantic search results.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SemanticSearchResponse {
    pub query: String,
    pub results: Vec<SemanticSearchResult>,
    pub count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SemanticSearchResult {
    pub name: String,
    pub title: Option<String>,
    pub similarity: f32,
}

/// Response for note information.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoteInfo {
    pub name: String,
    pub title: Option<String>,
    pub path: String,
    pub link_count: usize,
    pub backlink_count: usize,
    pub content: Option<String>,
}

/// Response for graph statistics.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GraphStatsResponse {
    pub total_notes: usize,
    pub total_links: usize,
    pub orphan_count: usize,
    pub orphan_notes: Vec<String>,
    pub hub_notes: Vec<HubNote>,
    pub broken_links: Vec<BrokenLinkInfo>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HubNote {
    pub name: String,
    pub connection_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BrokenLinkInfo {
    pub source: String,
    pub target: String,
}

impl From<BrokenLink> for BrokenLinkInfo {
    fn from(bl: BrokenLink) -> Self {
        Self {
            source: bl.source,
            target: bl.target,
        }
    }
}

// ============================================================================
// Tool Implementations
// ============================================================================

#[tool_router]
impl KnowledgeServer {
    /// Search for notes containing the given query.
    #[tool(description = "Search for notes containing the given text or regex pattern")]
    async fn search_notes(
        &self,
        Parameters(params): Parameters<SearchNotesParams>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        // Validate query is not empty
        if params.query.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "Search query cannot be empty".to_string(),
                None,
            ));
        }

        let options = SearchOptions {
            regex: params.regex,
            case_sensitive: params.case_sensitive,
            limit: params.limit,
            context_lines: 0,
        };

        let results = search::search(&self.config.vault_path, &params.query, &options)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let json = serde_json::to_string_pretty(&results)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Check for sensitive content in search results
        if let Err(message) = self.check_sensitive_content(&json, &peer).await? {
            return Ok(CallToolResult::success(vec![Content::text(message)]));
        }

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Get detailed information about a specific note.
    #[tool(
        description = "Get detailed information about a specific note, including its content, links, and metadata"
    )]
    async fn get_note(
        &self,
        Parameters(params): Parameters<GetNoteParams>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ensure_indexed().await?;

        let vault = self.vault.read().await;

        let note = vault.get_note(&params.name).ok_or_else(|| {
            ErrorData::invalid_params(format!("Note not found: {}", params.name), None)
        })?;

        let backlink_count = vault.backlinks(&params.name).len();

        let content = if params.include_content {
            Some(
                vault
                    .read_note_content(&params.name)
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
            )
        } else {
            None
        };

        let info = NoteInfo {
            name: note.name.clone(),
            title: note.title.clone(),
            path: note.path.to_string_lossy().to_string(),
            link_count: note.links.len(),
            backlink_count,
            content,
        };

        let json = serde_json::to_string_pretty(&info)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Check for sensitive content if content was included
        if params.include_content
            && let Err(message) = self.check_sensitive_content(&json, &peer).await?
        {
            return Ok(CallToolResult::success(vec![Content::text(message)]));
        }

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// List all notes in the vault.
    #[tool(description = "List all notes in the vault with basic information")]
    async fn list_notes(
        &self,
        Parameters(params): Parameters<ListNotesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Validate sort_by parameter
        let valid_sort_options = ["name", "modified", "links"];
        if !valid_sort_options.contains(&params.sort_by.as_str()) {
            return Err(ErrorData::invalid_params(
                format!(
                    "Invalid sort_by value '{}'. Must be one of: {}",
                    params.sort_by,
                    valid_sort_options.join(", ")
                ),
                None,
            ));
        }

        self.ensure_indexed().await?;

        let vault = self.vault.read().await;

        let mut notes: Vec<_> = vault
            .notes()
            .map(|n| {
                let backlinks = vault.backlinks(&n.name).len();
                (
                    n.name.clone(),
                    n.title.clone(),
                    n.links.len(),
                    backlinks,
                    n.modified,
                )
            })
            .collect();

        // Sort based on sort_by parameter
        match params.sort_by.as_str() {
            "modified" => notes.sort_by(|a, b| b.4.cmp(&a.4)),
            "links" => notes.sort_by(|a, b| (b.2 + b.3).cmp(&(a.2 + a.3))),
            _ => notes.sort_by(|a, b| a.0.cmp(&b.0)), // "name" or default
        }

        // Apply limit
        if let Some(limit) = params.limit {
            notes.truncate(limit);
        }

        #[derive(Serialize)]
        struct NoteListItem {
            name: String,
            title: Option<String>,
            outgoing_links: usize,
            backlinks: usize,
        }

        let items: Vec<_> = notes
            .into_iter()
            .map(|(name, title, links, backlinks, _)| NoteListItem {
                name,
                title,
                outgoing_links: links,
                backlinks,
            })
            .collect();

        let json = serde_json::to_string_pretty(&items)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Get notes that link to the specified note.
    #[tool(description = "Get all notes that link to the specified note (backlinks)")]
    async fn get_backlinks(
        &self,
        Parameters(params): Parameters<GetBacklinksParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ensure_indexed().await?;

        let graph = self.get_graph().await?;
        let backlinks = graph.backlinks(&params.name);

        #[derive(Serialize)]
        struct BacklinkResult {
            note: String,
            backlinks: Vec<String>,
            count: usize,
        }

        let result = BacklinkResult {
            note: params.name,
            count: backlinks.len(),
            backlinks,
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Get outgoing links from a note.
    #[tool(description = "Get all outgoing links from the specified note")]
    async fn get_links(
        &self,
        Parameters(params): Parameters<GetLinksParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ensure_indexed().await?;

        let vault = self.vault.read().await;

        let note = vault.get_note(&params.name).ok_or_else(|| {
            ErrorData::invalid_params(format!("Note not found: {}", params.name), None)
        })?;

        #[derive(Serialize)]
        struct LinksResult {
            note: String,
            links: Vec<LinkInfo>,
            count: usize,
        }

        #[derive(Serialize)]
        struct LinkInfo {
            target: String,
            display: Option<String>,
            exists: bool,
        }

        let links: Vec<_> = note
            .links
            .iter()
            .filter(|l| !l.target.is_empty()) // Skip same-file links
            .map(|l| LinkInfo {
                target: l.target.clone(),
                display: l.display.clone(),
                exists: vault.note_exists(&l.target),
            })
            .collect();

        let result = LinksResult {
            note: params.name,
            count: links.len(),
            links,
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Get statistics about the knowledge graph.
    #[tool(
        description = "Get statistics about the knowledge graph including orphan notes, hub notes, and broken links"
    )]
    async fn get_graph_stats(&self) -> Result<CallToolResult, ErrorData> {
        self.ensure_indexed().await?;

        let vault = self.vault.read().await;
        let graph = self.get_graph().await?;
        let stats = graph.stats();

        let broken_links: Vec<BrokenLinkInfo> =
            vault.broken_links().into_iter().map(Into::into).collect();

        let response = GraphStatsResponse {
            total_notes: stats.total_notes,
            total_links: stats.total_links,
            orphan_count: stats.orphan_count,
            orphan_notes: stats.orphan_notes,
            hub_notes: stats
                .hub_notes
                .into_iter()
                .map(|(name, count)| HubNote {
                    name,
                    connection_count: count,
                })
                .collect(),
            broken_links,
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Semantic search for notes by meaning.
    #[tool(
        description = "Search notes by meaning using natural language (semantic search). Requires embeddings to be enabled."
    )]
    async fn semantic_search(
        &self,
        Parameters(params): Parameters<SemanticSearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Check if embeddings are enabled
        if !self.config.enable_embeddings {
            return Err(ErrorData::invalid_params(
                "Semantic search is disabled. Set KNOWLEDGE_ENABLE_EMBEDDINGS=true to enable."
                    .to_string(),
                None,
            ));
        }

        // Validate query is not empty
        if params.query.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "Search query cannot be empty".to_string(),
                None,
            ));
        }

        let embedding_index = self.embedding_index.read().await;
        let index = embedding_index.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Embedding index not initialized. Please wait for initialization to complete."
                    .to_string(),
                None,
            )
        })?;

        // Perform semantic search
        let results = index
            .search(&params.query, params.limit)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Filter by threshold and get note titles
        let vault = self.vault.read().await;
        let results: Vec<SemanticSearchResult> = results
            .into_iter()
            .filter(|r| r.similarity >= params.threshold)
            .map(|r| {
                let title = vault.get_note(&r.name).and_then(|n| n.title.clone());
                SemanticSearchResult {
                    name: r.name,
                    title,
                    similarity: r.similarity,
                }
            })
            .collect();

        let response = SemanticSearchResponse {
            query: params.query,
            count: results.len(),
            results,
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Find notes similar to a given note.
    #[tool(description = "Find notes that are semantically similar to a given note")]
    async fn find_similar_notes(
        &self,
        Parameters(params): Parameters<FindSimilarParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Check if embeddings are enabled
        if !self.config.enable_embeddings {
            return Err(ErrorData::invalid_params(
                "Semantic search is disabled. Set KNOWLEDGE_ENABLE_EMBEDDINGS=true to enable."
                    .to_string(),
                None,
            ));
        }

        self.ensure_indexed().await?;

        let embedding_index = self.embedding_index.read().await;
        let index = embedding_index.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Embedding index not initialized. Please wait for initialization to complete."
                    .to_string(),
                None,
            )
        })?;

        // Find similar notes
        let results = index
            .find_similar(&params.name, params.limit)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Get note titles
        let vault = self.vault.read().await;
        let results: Vec<SemanticSearchResult> = results
            .into_iter()
            .map(|r| {
                let title = vault.get_note(&r.name).and_then(|n| n.title.clone());
                SemanticSearchResult {
                    name: r.name,
                    title,
                    similarity: r.similarity,
                }
            })
            .collect();

        #[derive(Serialize)]
        struct FindSimilarResponse {
            note: String,
            similar_notes: Vec<SemanticSearchResult>,
            count: usize,
        }

        let response = FindSimilarResponse {
            note: params.name,
            count: results.len(),
            similar_notes: results,
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

// ============================================================================
// ServerHandler Implementation
// ============================================================================

impl ServerHandler for KnowledgeServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(false),
                }),
                ..Default::default()
            },
            server_info: Implementation {
                name: "knowledge-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                title: None,
                icons: None,
                website_url: None,
            },
            ..Default::default()
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let ctx = ToolCallContext::new(self, request, context);
        self.tool_router.call(ctx).await
    }
}
