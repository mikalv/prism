//! MCP tool registry and definitions

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::backends::Query;
use crate::collection::CollectionManager;
use crate::Result;

/// Context passed to tool calls
pub struct ToolContext {
    pub manager: Arc<CollectionManager>,
}

/// Trait for MCP tools
#[async_trait]
pub trait McpTool: Send + Sync {
    /// Tool name (used in tools/call)
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// JSON Schema for input parameters
    fn input_schema(&self) -> Value;

    /// Execute the tool
    async fn call(&self, params: Value, ctx: &ToolContext) -> Result<Value>;
}

/// Registry of available MCP tools
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn McpTool>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool
    pub fn register(&mut self, tool: Arc<dyn McpTool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// List all tools in MCP format
    pub fn list(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|t| {
                json!({
                    "name": t.name(),
                    "description": t.description(),
                    "inputSchema": t.input_schema()
                })
            })
            .collect()
    }

    /// Call a tool by name
    pub async fn call(&self, name: &str, params: Value, ctx: &ToolContext) -> Result<Value> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| crate::Error::Backend(format!("Tool not found: {}", name)))?;

        tool.call(params, ctx).await
    }

    /// Get tool count
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

// ============================================================================
// Basic Tools (included in engraph-core)
// ============================================================================

/// Search tool - search documents in a collection
pub struct SearchTool;

#[async_trait]
impl McpTool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search documents in a collection"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "collection": {
                    "type": "string",
                    "description": "Collection name"
                },
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "limit": {
                    "type": "integer",
                    "default": 10,
                    "description": "Max results"
                }
            },
            "required": ["collection", "query"]
        })
    }

    async fn call(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let collection = params
            .get("collection")
            .and_then(|v| v.as_str())
            .ok_or_else(|| crate::Error::Backend("Missing collection".to_string()))?;

        let query_string = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| crate::Error::Backend("Missing query".to_string()))?;

        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        let query = Query {
            query_string: query_string.to_string(),
            fields: vec![],
            limit,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight: None,
        };

        let results = ctx.manager.search(collection, query, None).await?;

        Ok(json!({
            "results": results.results,
            "count": results.results.len(),
            "total": results.total,
            "latency_ms": results.latency_ms
        }))
    }
}

/// List collections tool
pub struct ListCollectionsTool;

#[async_trait]
impl McpTool for ListCollectionsTool {
    fn name(&self) -> &str {
        "list_collections"
    }

    fn description(&self) -> &str {
        "List all available collections"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn call(&self, _params: Value, ctx: &ToolContext) -> Result<Value> {
        let collections = ctx.manager.list_collections();
        Ok(json!({
            "collections": collections
        }))
    }
}

/// Register basic engraph-core tools
pub fn register_basic_tools(registry: &mut ToolRegistry) {
    registry.register(Arc::new(SearchTool));
    registry.register(Arc::new(ListCollectionsTool));
}
