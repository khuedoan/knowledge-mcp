//! Knowledge MCP Server Library
//!
//! This crate provides an MCP (Model Context Protocol) server for interacting
//! with a knowledge vault of markdown notes with wiki-style links.

pub mod config;
pub mod filter;
pub mod graph;
pub mod search;
pub mod tools;
pub mod vault;

pub use config::Config;
pub use filter::SensitiveDataFilter;
pub use tools::KnowledgeServer;
