//! MCP (Model Context Protocol) support for engraph-core
//!
//! This module provides HTTP SSE-based MCP transport with:
//! - SessionManager: Multi-client SSE connection management
//! - McpHandler: JSON-RPC 2.0 request handling
//! - ToolRegistry: Extensible tool registration

pub mod handler;
pub mod session;
pub mod tools;

pub use handler::McpHandler;
pub use session::SessionManager;
pub use tools::ToolRegistry;
