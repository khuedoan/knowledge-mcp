# Knowledge MCP

MCP server to allow agents to learn from and contribute to my personal knowledge
management system (see [./examples](./examples/)).

> **DISCLAIMER:** I basically vibe coded this MCP server. I provided the agent
> direction, architecture choices, libraries to use, feedback when using the
> server, etc., but I didn't look at the actual code.

## Features

- **Vault Indexing** - Parses markdown files with wiki-style `[[links]]`
- **Knowledge Graph** - Builds a graph of note connections (links, backlinks)
- **Content Search** - Regex-based search across all notes
- **Semantic Search** - Natural language search using local embeddings (BGE-small)
- **Knowledge Ingestion** - Create, update, and delete notes with content validation
- **Live Updates** - File system watching for automatic re-indexing
- **Content Caching** - LRU cache with modification time tracking
- **Sensitive Data Filtering** - Configurable keyword-based content filtering

## MCP Tools

### Read Tools

| Tool | Description |
|------|-------------|
| `list_notes` | List all notes with basic info |
| `get_note` | Get note content, links, and metadata |
| `search_notes` | Search notes by text or regex pattern |
| `semantic_search` | Search notes by meaning (natural language) |
| `find_similar_notes` | Find notes similar to a given note |
| `get_backlinks` | Get all notes linking to a specific note |
| `get_links` | Get all outgoing links from a note |
| `get_graph_stats` | Get knowledge graph statistics |

### Write Tools

| Tool | Description |
|------|-------------|
| `get_writing_guidelines` | Get vault's AGENTS.md conventions (must call before writing) |
| `create_note` | Create a new atomic note |
| `update_note` | Update an existing note (replace or append) |
| `delete_note` | Delete a note (requires confirmation) |

## MCP Resources

| Resource URI | Description |
|--------------|-------------|
| `vault://guidelines` | Writing guidelines from AGENTS.md |

## Writing Guidelines (AGENTS.md)

To enable write operations, create an `AGENTS.md` file in your vault root with your writing conventions. The agent must call `get_writing_guidelines` before creating, updating, or deleting notes.

Example `AGENTS.md`:

```markdown
# Writing Guidelines

## Zettelkasten Principles

1. **Atomicity**: One idea per note
2. **Self-contained**: Understandable on its own
3. **Connectivity**: Link to related notes with [[Wiki Links]]

## Note Structure

- Use H1 for the title (matching the note name)
- Use [[wiki links]] for connections to other notes
- Add a ## References section for citing sources

## Reference Format

- [Source Title](URL) - Accessed YYYY-MM-DD
  - *Optional insight about the source*
```

The server validates note content and returns warnings (not errors) if conventions aren't followed, helping guide the agent toward better note structure.

## Configuration

Set via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `KNOWLEDGE_VAULT_PATH` | (required) | Path to your markdown vault |
| `KNOWLEDGE_SENSITIVE_KEYWORDS` | `salary` | Comma-separated keywords to filter |
| `KNOWLEDGE_ENABLE_EMBEDDINGS` | `true` | Enable semantic search |
| `KNOWLEDGE_ENABLE_WATCHER` | `true` | Enable live file watching |
| `KNOWLEDGE_CACHE_SIZE` | `500` | Number of notes to cache in memory |
| `KNOWLEDGE_CACHE_DIR` | System cache dir | Directory for embeddings cache |

## Usage

Example OpenCode config:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "knowledge": {
      "type": "local",
      "command": [
        "cargo",
        "run",
        "--manifest-path",
        "/home/foo/src/knowledge-mcp/Cargo.toml"
      ],
      "environment": {
        "KNOWLEDGE_VAULT_PATH": "/home/foo/notes"
      },
      "enabled": true
    }
  }
}
```

Run manually:

```bash
# Build
cargo build --release

# Run (stdio transport for MCP)
KNOWLEDGE_VAULT_PATH=/path/to/vault ./target/release/knowledge-mcp
```

## Development

```bash
# Run tests
make test

# Format code
make fmt

# Run in dev mode
make dev
```
