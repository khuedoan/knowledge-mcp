//! Knowledge MCP Server Library
//!
//! This crate provides an MCP (Model Context Protocol) server for interacting
//! with a knowledge vault of markdown notes with wiki-style links.
//!
//! ## Features
//!
//! - **Content Caching**: LRU cache with modification time tracking
//! - **File Watching**: Live reload via file system watching
//! - **Semantic Search**: Local embeddings using fastembed
//! - **Parallel Indexing**: Fast initial vault indexing using rayon

pub mod cache;
pub mod config;
pub mod embedding;
pub mod filter;
pub mod graph;
pub mod search;
pub mod tools;
pub mod validation;
pub mod vault;
pub mod watcher;

pub use config::Config;
pub use filter::SensitiveDataFilter;
pub use tools::KnowledgeServer;
