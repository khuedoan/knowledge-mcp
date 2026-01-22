//! Integration tests for knowledge-mcp server.
//!
//! These tests use the examples/ directory as a test vault and verify
//! the MCP protocol interactions work correctly.

mod common;

use std::path::PathBuf;

use anyhow::Result;
use common::TestClientHandler;
use knowledge_mcp::{Config, KnowledgeServer};
use rmcp::{ServiceExt, model::ResourceContents, model::*, object};
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

    assert!(tool_names.contains(&"list_notes"));
    assert!(tool_names.contains(&"get_note"));
    assert!(tool_names.contains(&"search_notes"));
    assert!(tool_names.contains(&"get_backlinks"));
    assert!(tool_names.contains(&"get_links"));
    assert!(tool_names.contains(&"get_graph_stats"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_list_notes_returns_all_example_notes() -> Result<()> {
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
            name: "list_notes".into(),
            arguments: None,
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;
    let notes = json.as_array().expect("Expected array of notes");

    // Should have 14 example notes (including AGENTS.md)
    assert_eq!(notes.len(), 14);

    // Check some expected note names
    let note_names: Vec<&str> = notes
        .iter()
        .filter_map(|n| n.get("name").and_then(|v| v.as_str()))
        .collect();

    assert!(note_names.contains(&"Zettelkasten Method"));
    assert!(note_names.contains(&"Deep Work"));
    assert!(note_names.contains(&"Knowledge Graph"));

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

    assert_eq!(json.get("total_notes").and_then(|v| v.as_u64()), Some(14));
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
// WRITE TOOL TESTS
// =============================================================================

/// Create a temporary vault with an AGENTS.md file.
fn create_write_test_vault() -> Result<TempDir> {
    let temp_dir = TempDir::new()?;

    // Create AGENTS.md with writing guidelines
    std::fs::write(
        temp_dir.path().join("AGENTS.md"),
        r#"# Writing Guidelines

## Zettelkasten Principles

1. **Atomicity**: One idea per note
2. **Self-contained**: Understandable on its own
3. **Connectivity**: Link to related notes with [[Wiki Links]]

## Note Structure

- Use H1 for the title
- Use [[wiki links]] for connections
- Add a References section for sources
"#,
    )?;

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
async fn test_create_note_requires_guidelines() -> Result<()> {
    let temp_vault = create_write_test_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Try to create note without calling get_writing_guidelines first
    let result = client
        .call_tool(CallToolRequestParam {
            name: "create_note".into(),
            arguments: Some(object!({
                "name": "New Note",
                "content": "# New Note\n\nSome content."
            })),
            task: None,
        })
        .await;

    // Should fail because guidelines haven't been acknowledged
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("get_writing_guidelines"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_get_writing_guidelines_enables_writes() -> Result<()> {
    let temp_vault = create_write_test_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // First call get_writing_guidelines
    let result = client
        .call_tool(CallToolRequestParam {
            name: "get_writing_guidelines".into(),
            arguments: None,
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;
    assert!(json.get("guidelines").is_some());
    assert!(
        json.get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .contains("acknowledged")
    );

    // Now create_note should work
    let result = client
        .call_tool(CallToolRequestParam {
            name: "create_note".into(),
            arguments: Some(object!({
                "name": "New Note",
                "content": "# New Note\n\nThis is a new note about [[Existing Note]]."
            })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;
    assert_eq!(json.get("success").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(json.get("name").and_then(|v| v.as_str()), Some("New Note"));

    // Verify file was created
    assert!(temp_vault.path().join("New Note.md").exists());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_create_note_validates_content() -> Result<()> {
    let temp_vault = create_write_test_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Get guidelines first
    client
        .call_tool(CallToolRequestParam {
            name: "get_writing_guidelines".into(),
            arguments: None,
            task: None,
        })
        .await?;

    // Create note without H1 heading and without links
    let result = client
        .call_tool(CallToolRequestParam {
            name: "create_note".into(),
            arguments: Some(object!({
                "name": "Bad Note",
                "content": "This note has no heading and no links."
            })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;

    // Note should still be created (warnings, not errors)
    assert_eq!(json.get("success").and_then(|v| v.as_bool()), Some(true));

    // But should have validation warnings
    let validation = json.get("validation").expect("Should have validation");
    let warnings = validation
        .get("warnings")
        .and_then(|v| v.as_array())
        .expect("Should have warnings array");

    assert!(!warnings.is_empty());

    // Check for specific warnings
    let warning_codes: Vec<&str> = warnings
        .iter()
        .filter_map(|w| w.get("code").and_then(|v| v.as_str()))
        .collect();

    assert!(warning_codes.contains(&"missing_title"));
    assert!(warning_codes.contains(&"no_links"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_create_note_already_exists() -> Result<()> {
    let temp_vault = create_write_test_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Get guidelines first
    client
        .call_tool(CallToolRequestParam {
            name: "get_writing_guidelines".into(),
            arguments: None,
            task: None,
        })
        .await?;

    // Try to create a note that already exists
    let result = client
        .call_tool(CallToolRequestParam {
            name: "create_note".into(),
            arguments: Some(object!({
                "name": "Existing Note",
                "content": "# Existing Note\n\nNew content."
            })),
            task: None,
        })
        .await;

    // Should fail because note already exists
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("already exists"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_update_note_replace() -> Result<()> {
    let temp_vault = create_write_test_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Get guidelines first
    client
        .call_tool(CallToolRequestParam {
            name: "get_writing_guidelines".into(),
            arguments: None,
            task: None,
        })
        .await?;

    // Update note with replacement content
    let result = client
        .call_tool(CallToolRequestParam {
            name: "update_note".into(),
            arguments: Some(object!({
                "name": "Existing Note",
                "content": "# Existing Note\n\nCompletely new content with [[Target Note]]."
            })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;
    assert_eq!(json.get("success").and_then(|v| v.as_bool()), Some(true));

    // Verify content was replaced
    let content = std::fs::read_to_string(temp_vault.path().join("Existing Note.md"))?;
    assert!(content.contains("Completely new content"));
    assert!(!content.contains("existing note with a"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_update_note_append() -> Result<()> {
    let temp_vault = create_write_test_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Get guidelines first
    client
        .call_tool(CallToolRequestParam {
            name: "get_writing_guidelines".into(),
            arguments: None,
            task: None,
        })
        .await?;

    // Update note by appending content
    let result = client
        .call_tool(CallToolRequestParam {
            name: "update_note".into(),
            arguments: Some(object!({
                "name": "Existing Note",
                "append": "## References\n\n- [Source](https://example.com)"
            })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;
    assert_eq!(json.get("success").and_then(|v| v.as_bool()), Some(true));

    // Verify content was appended (original content still there)
    let content = std::fs::read_to_string(temp_vault.path().join("Existing Note.md"))?;
    assert!(content.contains("existing note that links to")); // Original
    assert!(content.contains("## References")); // Appended

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_update_note_not_found() -> Result<()> {
    let temp_vault = create_write_test_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Get guidelines first
    client
        .call_tool(CallToolRequestParam {
            name: "get_writing_guidelines".into(),
            arguments: None,
            task: None,
        })
        .await?;

    // Try to update a non-existent note
    let result = client
        .call_tool(CallToolRequestParam {
            name: "update_note".into(),
            arguments: Some(object!({
                "name": "NonExistent Note",
                "content": "# Content"
            })),
            task: None,
        })
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not found"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_delete_note_requires_confirm() -> Result<()> {
    let temp_vault = create_write_test_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Get guidelines first
    client
        .call_tool(CallToolRequestParam {
            name: "get_writing_guidelines".into(),
            arguments: None,
            task: None,
        })
        .await?;

    // Try to delete without confirm: true
    let result = client
        .call_tool(CallToolRequestParam {
            name: "delete_note".into(),
            arguments: Some(object!({
                "name": "Existing Note",
                "confirm": false
            })),
            task: None,
        })
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("confirm"));

    // Verify file still exists
    assert!(temp_vault.path().join("Existing Note.md").exists());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_delete_note_success() -> Result<()> {
    let temp_vault = create_write_test_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Get guidelines first
    client
        .call_tool(CallToolRequestParam {
            name: "get_writing_guidelines".into(),
            arguments: None,
            task: None,
        })
        .await?;

    // Delete with confirm: true
    let result = client
        .call_tool(CallToolRequestParam {
            name: "delete_note".into(),
            arguments: Some(object!({
                "name": "Target Note",
                "confirm": true
            })),
            task: None,
        })
        .await?;

    let json = parse_json_result(&result)?;
    assert_eq!(json.get("success").and_then(|v| v.as_bool()), Some(true));

    // Verify file was deleted
    assert!(!temp_vault.path().join("Target Note.md").exists());

    // Check that backlinks_broken includes "Existing Note"
    let backlinks = json
        .get("backlinks_broken")
        .and_then(|v| v.as_array())
        .expect("Should have backlinks_broken");
    let backlink_names: Vec<&str> = backlinks.iter().filter_map(|v| v.as_str()).collect();
    assert!(backlink_names.contains(&"Existing Note"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_list_tools_includes_write_tools() -> Result<()> {
    let temp_vault = create_write_test_vault()?;
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

    // Should include the new write tools
    assert!(tool_names.contains(&"get_writing_guidelines"));
    assert!(tool_names.contains(&"create_note"));
    assert!(tool_names.contains(&"update_note"));
    assert!(tool_names.contains(&"delete_note"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_list_resources_includes_guidelines() -> Result<()> {
    let temp_vault = create_write_test_vault()?;
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let config = Config::with_path(temp_vault.path().to_path_buf());
    let server = KnowledgeServer::new(config);

    tokio::spawn(async move {
        let server_handle = server.serve(server_transport).await.unwrap();
        server_handle.waiting().await.ok();
    });

    let client = TestClientHandler::new().serve(client_transport).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let resources = client.list_resources(None).await?;

    // Should include the guidelines resource
    let resource_uris: Vec<&str> = resources.resources.iter().map(|r| r.uri.as_str()).collect();
    assert!(resource_uris.contains(&"vault://guidelines"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn test_read_guidelines_resource() -> Result<()> {
    let temp_vault = create_write_test_vault()?;
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
        .read_resource(ReadResourceRequestParam {
            uri: "vault://guidelines".into(),
        })
        .await?;

    // Should have content
    assert!(!result.contents.is_empty());

    // Get text content
    if let ResourceContents::TextResourceContents { text, .. } = &result.contents[0] {
        assert!(text.contains("Zettelkasten"));
        assert!(text.contains("Atomicity"));
    } else {
        panic!("Expected text resource contents");
    }

    client.cancel().await?;
    Ok(())
}
