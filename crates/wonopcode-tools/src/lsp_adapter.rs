//! Adapter to bridge wonopcode-lsp LspClient to codemode LspService trait.

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use wonopcode_codemode::{
    LspService as CodemodeService,
    LspLocation, LspSymbol, LspHover,
    ServiceError, ServiceResult,
};
use wonopcode_lsp::{LspClient, DocumentSymbolInfo};

/// Adapter that bridges LspClient to codemode LspService trait.
pub struct LspServiceAdapter {
    client: Arc<LspClient>,
    project_root: PathBuf,
}

impl LspServiceAdapter {
    /// Create a new adapter with an existing LspClient.
    pub fn new(client: Arc<LspClient>, project_root: PathBuf) -> Self {
        Self { client, project_root }
    }
    
    /// Create a new adapter with default configurations.
    pub fn with_defaults(project_root: PathBuf) -> Self {
        let mut client = LspClient::with_defaults();
        client.set_project_root(project_root.clone());
        Self {
            client: Arc::new(client),
            project_root,
        }
    }
    
    fn resolve_path(&self, file: &str) -> PathBuf {
        let path = PathBuf::from(file);
        if path.is_absolute() {
            path
        } else {
            self.project_root.join(path)
        }
    }
}

#[async_trait]
impl CodemodeService for LspServiceAdapter {
    async fn definition(&self, file: &str, line: usize, column: usize) -> ServiceResult<Vec<LspLocation>> {
        let path = self.resolve_path(file);
        
        match self.client.goto_definition(&path, line as u32, column as u32).await {
            Ok(locations) => {
                Ok(locations.into_iter().map(|loc| LspLocation {
                    file: loc.uri.path().to_string(),
                    line: loc.range.start.line as usize,
                    column: loc.range.start.character as usize,
                    end_line: Some(loc.range.end.line as usize),
                    end_column: Some(loc.range.end.character as usize),
                }).collect())
            }
            Err(e) => Err(ServiceError::new("LSP_ERROR", e.to_string())),
        }
    }

    async fn references(
        &self,
        file: &str,
        line: usize,
        column: usize,
        include_declaration: bool,
    ) -> ServiceResult<Vec<LspLocation>> {
        let path = self.resolve_path(file);
        
        match self.client.find_references(&path, line as u32, column as u32, include_declaration).await {
            Ok(locations) => {
                Ok(locations.into_iter().map(|loc| LspLocation {
                    file: loc.uri.path().to_string(),
                    line: loc.range.start.line as usize,
                    column: loc.range.start.character as usize,
                    end_line: Some(loc.range.end.line as usize),
                    end_column: Some(loc.range.end.character as usize),
                }).collect())
            }
            Err(e) => Err(ServiceError::new("LSP_ERROR", e.to_string())),
        }
    }

    async fn symbols(&self, file: &str) -> ServiceResult<Vec<LspSymbol>> {
        let path = self.resolve_path(file);
        let file_str = file.to_string();
        
        match self.client.document_symbols(&path).await {
            Ok(symbols) => {
                Ok(flatten_symbols(&symbols, &file_str))
            }
            Err(e) => Err(ServiceError::new("LSP_ERROR", e.to_string())),
        }
    }

    async fn hover(&self, file: &str, line: usize, column: usize) -> ServiceResult<Option<LspHover>> {
        let path = self.resolve_path(file);
        
        match self.client.hover(&path, line as u32, column as u32).await {
            Ok(Some(info)) => {
                Ok(Some(LspHover {
                    contents: info,
                    range: None,
                }))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(ServiceError::new("LSP_ERROR", e.to_string())),
        }
    }
}

/// Flatten nested DocumentSymbolInfo into flat LspSymbol list.
fn flatten_symbols(symbols: &[DocumentSymbolInfo], file: &str) -> Vec<LspSymbol> {
    let mut result = Vec::new();
    flatten_symbols_recursive(symbols, None, file, &mut result);
    result
}

fn flatten_symbols_recursive(
    symbols: &[DocumentSymbolInfo],
    parent: Option<&str>,
    file: &str,
    result: &mut Vec<LspSymbol>,
) {
    for sym in symbols {
        result.push(LspSymbol {
            name: sym.name.clone(),
            kind: format!("{:?}", sym.kind),
            location: LspLocation {
                file: file.to_string(),
                line: sym.range.start.line as usize,
                column: sym.range.start.character as usize,
                end_line: Some(sym.range.end.line as usize),
                end_column: Some(sym.range.end.character as usize),
            },
            container_name: parent.map(String::from),
        });
        
        flatten_symbols_recursive(&sym.children, Some(&sym.name), file, result);
    }
}
