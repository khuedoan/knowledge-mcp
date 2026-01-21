//! MCP tool implementations for the knowledge vault server.
//!
//! This module provides the main `KnowledgeServer` handler and all MCP tools
//! for interacting with a knowledge vault of markdown notes.

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

use crate::config::Config;
use crate::filter::SensitiveDataFilter;
use crate::graph::KnowledgeGraph;
use crate::search::{self, SearchOptions};
use crate::vault::{BrokenLink, Vault};

/// The knowledge vault MCP server handler.
#[derive(Clone)]
pub struct KnowledgeServer {
    vault: Arc<RwLock<Vault>>,
    /// Cached knowledge graph, invalidated when vault is re-indexed.
    graph_cache: Arc<RwLock<Option<KnowledgeGraph>>>,
    config: Config,
    filter: SensitiveDataFilter,
    tool_router: ToolRouter<Self>,
}

impl KnowledgeServer {
    /// Create a new knowledge server with the given configuration.
    pub fn new(config: Config) -> Self {
        let vault = Vault::new(&config.vault_path);
        let filter = SensitiveDataFilter::new(config.sensitive_keywords.clone());
        Self {
            vault: Arc::new(RwLock::new(vault)),
            graph_cache: Arc::new(RwLock::new(None)),
            config,
            filter,
            tool_router: Self::tool_router(),
        }
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
