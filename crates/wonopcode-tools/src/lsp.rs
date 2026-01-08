//! LSP tool - Language Server Protocol operations for code intelligence.
//!
//! Provides AI with access to code intelligence features:
//! - Go to definition
//! - Find references
//! - Document symbols
//! - Hover information

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;
use wonopcode_lsp::LspClient;

/// LSP tool for code intelligence operations.
pub struct LspTool {
    /// LSP client (lazily initialized or shared).
    client: RwLock<Option<Arc<LspClient>>>,
}

impl LspTool {
    /// Create a new LSP tool.
    pub fn new() -> Self {
        Self {
            client: RwLock::new(None),
        }
    }

    /// Create a new LSP tool with a shared client.
    pub fn with_client(client: Arc<LspClient>) -> Self {
        Self {
            client: RwLock::new(Some(client)),
        }
    }

    /// Get or initialize the LSP client.
    async fn get_client(&self) -> Arc<LspClient> {
        let mut guard = self.client.write().await;
        if guard.is_none() {
            let client = LspClient::with_defaults();
            *guard = Some(Arc::new(client));
        }
        // SAFETY: We just ensured guard is Some above
        guard.clone().expect("LSP client was just initialized")
    }

    /// Get the LSP client if initialized (for status polling).
    pub async fn client(&self) -> Option<Arc<LspClient>> {
        self.client.read().await.clone()
    }
}

impl Default for LspTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LspArgs {
    /// Operation to perform.
    operation: String,
    /// File path.
    file: String,
    /// Line number (0-based).
    #[serde(default)]
    line: Option<u32>,
    /// Column number (0-based).
    #[serde(default)]
    column: Option<u32>,
    /// Include declaration in references.
    #[serde(default = "default_true")]
    include_declaration: bool,
}

fn default_true() -> bool {
    true
}

#[async_trait]
impl Tool for LspTool {
    fn id(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        r#"Perform LSP (Language Server Protocol) operations for code intelligence.

Operations:
- "definition": Go to definition of symbol at position
- "references": Find all references to symbol at position  
- "symbols": List all symbols in the file
- "hover": Get hover information (type, docs) at position

For definition/references/hover, provide file, line, and column (0-based).
For symbols, only file is required.

Note: Requires the appropriate language server to be installed:
- Rust: rust-analyzer
- TypeScript/JavaScript: typescript-language-server
- Python: pyright-langserver
- Go: gopls"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["operation", "file"],
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["definition", "references", "symbols", "hover"],
                    "description": "The LSP operation to perform"
                },
                "file": {
                    "type": "string",
                    "description": "Path to the file"
                },
                "line": {
                    "type": "integer",
                    "description": "Line number (0-based, required for definition/references/hover)"
                },
                "column": {
                    "type": "integer",
                    "description": "Column number (0-based, required for definition/references/hover)"
                },
                "includeDeclaration": {
                    "type": "boolean",
                    "description": "Include declaration in references (default: true)"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: LspArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {}", e)))?;

        // Resolve file path
        let file_path = resolve_path(&args.file, &ctx.cwd, &ctx.root_dir)?;

        let client = self.get_client().await;

        match args.operation.as_str() {
            "definition" => {
                let line = args.line.ok_or_else(|| {
                    ToolError::validation("line is required for definition operation")
                })?;
                let column = args.column.ok_or_else(|| {
                    ToolError::validation("column is required for definition operation")
                })?;

                match client.goto_definition(&file_path, line, column).await {
                    Ok(locations) => {
                        if locations.is_empty() {
                            Ok(ToolOutput::new(
                                "No definition found",
                                "No definition found at the specified location.",
                            ))
                        } else {
                            let out = format_locations(&locations);
                            Ok(ToolOutput::new(
                                format!("Found {} definition(s)", locations.len()),
                                out,
                            ).with_metadata(json!({
                                "count": locations.len()
                            })))
                        }
                    }
                    Err(e) => {
                        warn!("LSP definition failed: {}", e);
                        Ok(ToolOutput::new(
                            "Definition lookup failed",
                            format!("Error: {}. Make sure the language server is installed and the file type is supported.", e),
                        ))
                    }
                }
            }

            "references" => {
                let line = args.line.ok_or_else(|| {
                    ToolError::validation("line is required for references operation")
                })?;
                let column = args.column.ok_or_else(|| {
                    ToolError::validation("column is required for references operation")
                })?;

                match client.find_references(&file_path, line, column, args.include_declaration).await {
                    Ok(locations) => {
                        if locations.is_empty() {
                            Ok(ToolOutput::new(
                                "No references found",
                                "No references found at the specified location.",
                            ))
                        } else {
                            let out = format_locations(&locations);
                            Ok(ToolOutput::new(
                                format!("Found {} reference(s)", locations.len()),
                                out,
                            ).with_metadata(json!({
                                "count": locations.len()
                            })))
                        }
                    }
                    Err(e) => {
                        warn!("LSP references failed: {}", e);
                        Ok(ToolOutput::new(
                            "References lookup failed",
                            format!("Error: {}. Make sure the language server is installed.", e),
                        ))
                    }
                }
            }

            "symbols" => {
                match client.document_symbols(&file_path).await {
                    Ok(symbols) => {
                        if symbols.is_empty() {
                            Ok(ToolOutput::new(
                                "No symbols found",
                                "No symbols found in the file.",
                            ))
                        } else {
                            let out = format_symbols(&symbols, 0);
                            Ok(ToolOutput::new(
                                format!("Found {} symbol(s)", count_symbols(&symbols)),
                                out,
                            ).with_metadata(json!({
                                "count": count_symbols(&symbols)
                            })))
                        }
                    }
                    Err(e) => {
                        warn!("LSP symbols failed: {}", e);
                        Ok(ToolOutput::new(
                            "Symbols lookup failed",
                            format!("Error: {}. Make sure the language server is installed.", e),
                        ))
                    }
                }
            }

            "hover" => {
                let line = args.line.ok_or_else(|| {
                    ToolError::validation("line is required for hover operation")
                })?;
                let column = args.column.ok_or_else(|| {
                    ToolError::validation("column is required for hover operation")
                })?;

                match client.hover(&file_path, line, column).await {
                    Ok(Some(info)) => {
                        Ok(ToolOutput::new("Hover information", info))
                    }
                    Ok(None) => {
                        Ok(ToolOutput::new(
                            "No hover information",
                            "No hover information available at the specified location.",
                        ))
                    }
                    Err(e) => {
                        warn!("LSP hover failed: {}", e);
                        Ok(ToolOutput::new(
                            "Hover lookup failed",
                            format!("Error: {}. Make sure the language server is installed.", e),
                        ))
                    }
                }
            }

            _ => Err(ToolError::validation(format!(
                "Unknown operation: {}. Valid operations are: definition, references, symbols, hover",
                args.operation
            ))),
        }
    }
}

/// Resolve a file path.
fn resolve_path(
    path: &str,
    cwd: &std::path::Path,
    root_dir: &std::path::Path,
) -> ToolResult<PathBuf> {
    let path = PathBuf::from(path);

    if path.is_absolute() {
        if path.exists() {
            return Ok(path);
        }
    } else {
        // Try relative to cwd
        let cwd_path = cwd.join(&path);
        if cwd_path.exists() {
            return Ok(cwd_path);
        }

        // Try relative to root
        let root_path = root_dir.join(&path);
        if root_path.exists() {
            return Ok(root_path);
        }
    }

    // Return as-is for LSP to handle (it may create the file URI anyway)
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(cwd.join(&path))
    }
}

/// Format locations for output.
fn format_locations(locations: &[wonopcode_lsp::Location]) -> String {
    locations
        .iter()
        .map(|loc| {
            let path = loc.uri.path();
            let start = &loc.range.start;
            format!("{}:{}:{}", path, start.line + 1, start.character + 1)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format symbols for output.
fn format_symbols(symbols: &[wonopcode_lsp::client::DocumentSymbolInfo], indent: usize) -> String {
    let mut output = String::new();
    let prefix = "  ".repeat(indent);

    for symbol in symbols {
        let kind = format!("{:?}", symbol.kind);
        let line = symbol.range.start.line + 1;
        output.push_str(&format!(
            "{}{} {} (line {})\n",
            prefix, kind, symbol.name, line
        ));

        if !symbol.children.is_empty() {
            output.push_str(&format_symbols(&symbol.children, indent + 1));
        }
    }

    output
}

/// Count total symbols including children.
fn count_symbols(symbols: &[wonopcode_lsp::client::DocumentSymbolInfo]) -> usize {
    symbols.iter().map(|s| 1 + count_symbols(&s.children)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsp_tool_creation() {
        let tool = LspTool::new();
        assert_eq!(tool.id(), "lsp");
    }

    #[test]
    fn test_parameters_schema() {
        let tool = LspTool::new();
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["operation"].is_object());
        assert!(schema["properties"]["file"].is_object());
    }
}
