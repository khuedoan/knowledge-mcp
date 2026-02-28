# Knowledge MCP

MCP server to allow agents to learn from my personal knowledge management
system. An example knowledge vault is available at [`./examples`](./examples/).

> **DISCLAIMER:** I basically vibe coded this MCP server. I provided the agent
> direction, architecture choices, libraries to use, feedback when using the
> server, etc., but I rarely look at the actual code.

## Features

- Vault Indexing: parses markdown files with wiki-style `[[links]]`
- Knowledge Graph: builds a graph of note connections (links, backlinks)
- Content Search: regex-based search across all notes
- Semantic Search: chunked local embeddings with link context and graph expansion
- Live Updates: file system watching for automatic re-indexing
- Sensitive Data Filtering: configurable keyword-based content filtering
- Content Caching: LRU cache with modification time tracking

> This MCP server used to have features for digesting source material, but I
become skeptical about that because it doesn't really help me understand the
source material and write notes in my own words (which is an important part of
Zettelkasten). I want the knowledge to be mine before giving agents access to
it, not the other way around, so I eventually removed edit features and turned
the MCP server into read-only.

## Usage

### Installation

> By default it copies the binary to `~/.local/bin`, you need to have that in
> `$PATH` or change the install destination.

```sh
make install
```

### Configuration

Here's an example OpenCode config:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "knowledge": {
      "type": "local",
      "command": [
        "knowledge-mcp"
      ],
      "environment": {
        "KNOWLEDGE_VAULT_PATH": "~/Documents/notes"
      },
      "enabled": true
    }
  }
}
```

Environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `KNOWLEDGE_VAULT_PATH` | (required) | Path to your markdown vault |
| `KNOWLEDGE_SENSITIVE_KEYWORDS` | `salary` | Comma-separated keywords to filter |
| `KNOWLEDGE_ENABLE_WATCHER` | `true` | Enable live file watching |
| `KNOWLEDGE_CACHE_SIZE` | `500` | Number of notes to cache in memory |
| `KNOWLEDGE_CACHE_DIR` | System cache dir | Directory for embeddings cache |
| `KNOWLEDGE_EMBEDDING_MAX_CHARS` | `1800` | Max characters per embedding chunk |
| `KNOWLEDGE_EMBEDDING_CHUNK_OVERLAP_CHARS` | `200` | Overlap between chunks (characters) |
| `KNOWLEDGE_EMBEDDING_LINK_CONTEXT_MAX_CHARS` | `320` | Max characters for link-context embeddings |
| `KNOWLEDGE_EMBEDDING_LINK_CONTEXT_WEIGHT` | `0.7` | Weight applied to link-context similarity |
| `KNOWLEDGE_EMBEDDING_INCLUDE_LINK_CONTEXT` | `true` | Include link-context embeddings |

## MCP Tools

| Tool | Description |
|------|-------------|
| `get_note` | Get note content, outgoing links, and backlinks |
| `search_notes` | Search notes by text or regex pattern |
| `semantic_search` | Search notes by meaning (natural language) |
| `find_similar_notes` | Find notes similar to a given note |
| `get_backlinks` | Get all notes linking to a specific note |
| `get_links` | Get all outgoing links from a note |
| `get_graph_stats` | Get knowledge graph statistics |

### Semantic Search Behavior

- Long notes are chunked with overlap; note scores aggregate across chunks.
- A link-context embedding (outgoing links + backlinks) is added per note.
- Results expand through link neighbors (backlinks + outgoing links).
- Notes under `archive/` and journal-like notes (e.g., `index`, `journal/`, `daily/`)
  are gently downranked.

## Development

```sh
make dev
make test
```
