//! Integration tests for knowledge-mcp server.
//!
//! These tests use the examples/ directory as a test vault and verify
//! the MCP protocol interactions work correctly.

mod common;

use std::path::PathBuf;

use anyhow::Result;
use common::TestClientHandler;
use knowledge_mcp::{Config, KnowledgeServer};
use rmcp::{ServiceExt, model::*, object};
use serde_json::Value;
use tempfile::TempDir;

/// Get the path to the examples directory.
fn examples_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples")
}

/// Extract text content from a tool result.
fn get_text_content(result: &CallToolResult) -> Option<&str> {
    result
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.as_str())
}

/// Parse JSON from tool result text content.
fn parse_json_result(result: &CallToolResult) -> Result<Value> {
    let text = get_text_content(result).ok_or_else(|| anyhow::anyhow!("No text content"))?;
    Ok(serde_json::from_str(text)?)
}

// =============================================================================
// BASIC TOOL TESTS (using examples/ directory)
// =============================================================================

#[tokio::test]
async fn test_list_tools_returns_expected_tools() -> Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(examples_path());
    let server = KnowledgeServer::new(config);

    // Spawn server in background
    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    // Connect client
    let client = TestClientHandler::new().serve(client_transport).await?;

    // Wait for initialization
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let tools = client.list_tools(None).await?;

    let tool_names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();

    assert!(tool_names.contains(&"get_note"));
    assert!(tool_names.contains(&"search_notes"));
    assert!(tool_names.contains(&"get_backlinks"));
    assert!(tool_names.contains(&"get_links"));
    assert!(tool_names.contains(&"get_graph_stats"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_get_note_returns_content() -> Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(examples_path());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "get_note".into(),
            arguments: Some(object!({ "name": "Zettelkasten Method" })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;

    assert_eq!(
        json.get("name").and_then(|v| v.as_str()),
        Some("Zettelkasten Method")
    );
    assert!(json.get("content").is_some());

    let content = json.get("content").and_then(|v| v.as_str()).unwrap();
    assert!(content.contains("Zettelkasten"));
    assert!(content.contains("[[Deep Work]]")); // Should contain wiki-style links

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_get_note_without_content() -> Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(examples_path());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "get_note".into(),
            arguments: Some(object!({ "name": "Deep Work", "include_content": false })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;

    assert_eq!(json.get("name").and_then(|v| v.as_str()), Some("Deep Work"));
    assert!(json.get("content").and_then(|v| v.as_str()).is_none());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_get_note_not_found() -> Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(examples_path());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "get_note".into(),
            arguments: Some(object!({ "name": "NonExistentNote" })),
            task: None,
        })
        .await;

    // Should return an error
    assert!(result.is_err());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_search_notes_finds_results() -> Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(examples_path());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "search_notes".into(),
            arguments: Some(object!({ "query": "wiki link" })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;
    let results = json.as_array().expect("Expected array of results");

    // Should find at least one result (Zettelkasten Method mentions wiki links)
    assert!(!results.is_empty());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_search_notes_no_results() -> Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(examples_path());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "search_notes".into(),
            arguments: Some(object!({ "query": "xyznonexistentterm123" })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;
    let results = json.as_array().expect("Expected array of results");

    assert!(results.is_empty());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_get_backlinks() -> Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(examples_path());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "get_backlinks".into(),
            arguments: Some(object!({ "name": "Deep Work" })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;

    assert_eq!(json.get("note").and_then(|v| v.as_str()), Some("Deep Work"));

    let backlinks = json.get("backlinks").and_then(|v| v.as_array());
    assert!(backlinks.is_some());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_get_links() -> Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(examples_path());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "get_links".into(),
            arguments: Some(object!({ "name": "Knowledge Graph" })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;

    assert_eq!(
        json.get("note").and_then(|v| v.as_str()),
        Some("Knowledge Graph")
    );

    let links = json.get("links").and_then(|v| v.as_array());
    assert!(links.is_some());
    assert!(!links.unwrap().is_empty());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_graph_stats() -> Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(examples_path());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "get_graph_stats".into(),
            arguments: None,
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;

    assert_eq!(json.get("total_notes").and_then(|v| v.as_u64()), Some(13));
    assert!(
        json.get("total_links")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            > 0
    );

    client.cancel().await?;
    Ok(())
}

// =============================================================================
// SENSITIVE DATA FILTER TESTS
// =============================================================================

/// Create a temporary vault with a sensitive note.
fn create_sensitive_vault() -> Result<TempDir> {
    let temp_dir = TempDir::new()?;

    // Create a normal note
    std::fs::write(
        temp_dir.path().join("Normal Note.md"),
        "# Normal Note\n\nThis is a normal note with no sensitive content.",
    )?;

    // Create a note with sensitive content
    std::fs::write(
        temp_dir.path().join("Sensitive Note.md"),
        "# Sensitive Note\n\nMy salary is $150,000 per year.\nMy SSN is 123-45-6789.",
    )?;

    Ok(temp_dir)
}

#[tokio::test]
async fn test_normal_content_passes_filter() -> Result<()> {
    let temp_vault = create_sensitive_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "get_note".into(),
            arguments: Some(object!({ "name": "Normal Note" })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;

    // Normal content should pass through
    assert_eq!(
        json.get("name").and_then(|v| v.as_str()),
        Some("Normal Note")
    );
    let content = json.get("content").and_then(|v| v.as_str()).unwrap();
    assert!(content.contains("normal note"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_sensitive_content_with_elicitation_accept() -> Result<()> {
    let temp_vault = create_sensitive_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    // Client accepts elicitation
    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "get_note".into(),
            arguments: Some(object!({ "name": "Sensitive Note" })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;

    // With accepted elicitation, content should be returned
    assert_eq!(
        json.get("name").and_then(|v| v.as_str()),
        Some("Sensitive Note")
    );
    let content = json.get("content").and_then(|v| v.as_str()).unwrap();
    assert!(content.contains("salary"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_sensitive_content_with_elicitation_decline() -> Result<()> {
    let temp_vault = create_sensitive_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    // Client declines elicitation
    let client = TestClientHandler::declining()
        .serve(client_transport)
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "get_note".into(),
            arguments: Some(object!({ "name": "Sensitive Note" })),
            task: None,
        })
        .await?;

    let text = get_text_content(&result).expect("Should have text content");

    // With declined elicitation, content should be blocked
    assert!(text.contains("declined") || text.contains("not confirmed"));
    assert!(!text.contains("$150,000")); // Actual salary should not appear

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_sensitive_content_without_elicitation_support() -> Result<()> {
    let temp_vault = create_sensitive_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    // Client without elicitation capability
    let client = TestClientHandler::without_elicitation()
        .serve(client_transport)
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "get_note".into(),
            arguments: Some(object!({ "name": "Sensitive Note" })),
            task: None,
        })
        .await?;

    let text = get_text_content(&result).expect("Should have text content");

    // Without elicitation support, content should be blocked
    assert!(text.contains("blocked") || text.contains("doesn't support"));
    assert!(!text.contains("$150,000")); // Actual salary should not appear

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_sensitive_search_with_elicitation_decline() -> Result<()> {
    let temp_vault = create_sensitive_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    // Client declines elicitation
    let client = TestClientHandler::declining()
        .serve(client_transport)
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let result = client
        .call_tool(CallToolRequestParam {
            name: "search_notes".into(),
            arguments: Some(object!({ "query": "salary" })),
            task: None,
        })
        .await?;

    let text = get_text_content(&result).expect("Should have text content");

    // Search results containing sensitive data should be blocked
    assert!(
        text.contains("declined") || text.contains("not confirmed") || text.contains("cancelled")
    );

    client.cancel().await?;
    Ok(())
}

// =============================================================================
// BASIC VAULT TESTS
// =============================================================================

/// Create a temporary vault with a few notes.
fn create_test_vault() -> Result<TempDir> {
    let temp_dir = TempDir::new()?;

    // Create an existing note that links TO Target Note
    std::fs::write(
        temp_dir.path().join("Existing Note.md"),
        "# Existing Note\n\nThis is an existing note that links to [[Target Note]].",
    )?;

    // Create a note that will have backlinks (linked TO by Existing Note)
    std::fs::write(
        temp_dir.path().join("Target Note.md"),
        "# Target Note\n\nThis note is linked to by other notes.",
    )?;

    Ok(temp_dir)
}

#[tokio::test]
async fn test_list_tools_excludes_write_tools() -> Result<()> {
    let temp_vault = create_test_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let tools = client.list_tools(None).await?;
    let tool_names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();

    assert!(!tool_names.contains(&"get_writing_guidelines"));
    assert!(!tool_names.contains(&"create_note"));
    assert!(!tool_names.contains(&"update_note"));
    assert!(!tool_names.contains(&"delete_note"));

    client.cancel().await?;
    Ok(())
}
