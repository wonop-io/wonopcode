//! LSP client implementation.

use crate::config::LspConfig;
use crate::error::{LspError, LspResult};
use crate::transport::{JsonRpcNotification, JsonRpcRequest, LspTransport};
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall,
    CallHierarchyPrepareParams, Diagnostic, DiagnosticSeverity, DidOpenTextDocumentParams,
    DocumentSymbol, DocumentSymbolParams, GotoDefinitionParams, Hover, HoverParams,
    InitializeParams, InitializeResult, InitializedParams, Location, PartialResultParams, Position,
    ReferenceContext, ReferenceParams, ServerCapabilities, SymbolInformation,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Uri,
    WorkDoneProgressParams, WorkspaceFolder, WorkspaceSymbolParams,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex, RwLock};
use tracing::{debug, error, info, warn};

/// A connected language server.
struct ServerConnection {
    config: LspConfig,
    transport: Arc<LspTransport>,
    /// Server capabilities advertised during initialization.
    capabilities: ServerCapabilities,
    root: PathBuf,
    /// Diagnostics per file URI
    diagnostics: RwLock<HashMap<String, Vec<Diagnostic>>>,
    /// Pending diagnostics waiters per file path
    diagnostics_waiters: Mutex<HashMap<String, Vec<oneshot::Sender<()>>>>,
}

impl ServerConnection {
    /// Check if the server supports textDocument/definition.
    fn supports_definition(&self) -> bool {
        self.capabilities.definition_provider.is_some()
    }

    /// Check if the server supports textDocument/references.
    fn supports_references(&self) -> bool {
        self.capabilities.references_provider.is_some()
    }

    /// Check if the server supports textDocument/implementation.
    fn supports_implementation(&self) -> bool {
        self.capabilities.implementation_provider.is_some()
    }

    /// Check if the server supports textDocument/hover.
    fn supports_hover(&self) -> bool {
        self.capabilities.hover_provider.is_some()
    }

    /// Check if the server supports textDocument/documentSymbol.
    fn supports_document_symbol(&self) -> bool {
        self.capabilities.document_symbol_provider.is_some()
    }

    /// Check if the server supports workspace/symbol.
    fn supports_workspace_symbol(&self) -> bool {
        self.capabilities.workspace_symbol_provider.is_some()
    }

    /// Check if the server supports textDocument/prepareCallHierarchy.
    fn supports_call_hierarchy(&self) -> bool {
        self.capabilities.call_hierarchy_provider.is_some()
    }
}

/// LSP server status for UI display.
#[derive(Debug, Clone)]
pub struct LspStatus {
    pub id: String,
    pub name: String,
    pub root: String,
    pub status: LspServerStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LspServerStatus {
    Connected,
    Error,
}

/// LSP client for managing multiple language server connections.
///
/// - Tracks broken servers to avoid repeated spawn attempts
/// - Deduplicates concurrent spawn attempts for the same server
/// - Collects diagnostics from all connected servers
/// - Supports all major LSP operations
pub struct LspClient {
    /// Available configurations (not yet connected).
    configs: Vec<LspConfig>,
    /// Connected servers keyed by "root_path:server_id".
    servers: RwLock<HashMap<String, Arc<ServerConnection>>>,
    /// Broken server+root combinations (failed to spawn or initialize).
    broken: RwLock<HashSet<String>>,
    /// In-flight spawn operations to deduplicate concurrent requests.
    spawning: Mutex<HashMap<String, Arc<tokio::sync::Notify>>>,
    /// Request ID counter.
    next_id: AtomicU64,
    /// Project root directory.
    project_root: PathBuf,
}

impl LspClient {
    /// Create a new LSP client.
    pub fn new() -> Self {
        Self {
            configs: Vec::new(),
            servers: RwLock::new(HashMap::new()),
            broken: RwLock::new(HashSet::new()),
            spawning: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            project_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    /// Create a client with default configurations.
    pub fn with_defaults() -> Self {
        let mut client = Self::new();
        client.configs = crate::config::default_configs();
        client
    }

    /// Set the project root directory.
    pub fn set_project_root(&mut self, root: PathBuf) {
        self.project_root = root;
    }

    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Add a server configuration.
    pub fn add_config(&mut self, config: LspConfig) {
        self.configs.push(config);
    }

    /// Get the key for a server+root combination.
    fn server_key(server_id: &str, root: &Path) -> String {
        format!("{}:{}", root.display(), server_id)
    }

    /// Check if we have any LSP clients that could handle this file.
    pub async fn has_clients(&self, file_path: &Path) -> bool {
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        // Check connected servers
        let servers = self.servers.read().await;
        for conn in servers.values() {
            if conn.config.handles_extension(ext) {
                return true;
            }
        }
        drop(servers);

        // Check available configs
        let broken = self.broken.read().await;
        for config in &self.configs {
            if config.handles_extension(ext) && config.enabled {
                let root = self.get_root_for_file(file_path, config).await;
                let key = Self::server_key(&config.language, &root);
                if !broken.contains(&key) {
                    return true;
                }
            }
        }

        false
    }

    /// Get root directory for a file based on server config.
    async fn get_root_for_file(&self, _file_path: &Path, _config: &LspConfig) -> PathBuf {
        // For now, use project root. Could be enhanced to detect workspace root
        // based on markers like Cargo.toml, package.json, etc.
        self.project_root.clone()
    }

    /// Get or spawn server for a file, with deduplication and broken tracking.
    #[allow(clippy::cognitive_complexity)]
    async fn get_servers_for_file(
        &self,
        file_path: &Path,
    ) -> LspResult<Vec<Arc<ServerConnection>>> {
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let mut result = Vec::new();

        // Find all applicable configs
        let applicable_configs: Vec<_> = self
            .configs
            .iter()
            .filter(|c| c.handles_extension(ext) && c.enabled)
            .cloned()
            .collect();

        for config in applicable_configs {
            let root = self.get_root_for_file(file_path, &config).await;
            let key = Self::server_key(&config.language, &root);

            // Check if broken
            {
                let broken = self.broken.read().await;
                if broken.contains(&key) {
                    continue;
                }
            }

            // Check if already connected
            {
                let servers = self.servers.read().await;
                if let Some(conn) = servers.get(&key) {
                    result.push(Arc::clone(conn));
                    continue;
                }
            }

            // Check if spawn is in flight, wait for it
            let notify = {
                let mut spawning = self.spawning.lock().await;
                if let Some(notify) = spawning.get(&key) {
                    Some(Arc::clone(notify))
                } else {
                    // We'll be the one spawning
                    let notify = Arc::new(tokio::sync::Notify::new());
                    spawning.insert(key.clone(), notify);
                    None
                }
            };

            if let Some(notify) = notify {
                // Wait for the other spawn to complete
                notify.notified().await;

                // Check if it succeeded
                let servers = self.servers.read().await;
                if let Some(conn) = servers.get(&key) {
                    result.push(Arc::clone(conn));
                }
                continue;
            }

            // Spawn the server
            match self.spawn_server(config.clone(), root.clone()).await {
                Ok(conn) => {
                    let conn = Arc::new(conn);
                    self.servers
                        .write()
                        .await
                        .insert(key.clone(), Arc::clone(&conn));
                    result.push(conn);
                    info!(server = %config.language, root = %root.display(), "LSP server connected");
                }
                Err(e) => {
                    error!(server = %config.language, error = %e, "Failed to spawn LSP server");
                    self.broken.write().await.insert(key.clone());
                }
            }

            // Notify waiters
            {
                let mut spawning = self.spawning.lock().await;
                if let Some(notify) = spawning.remove(&key) {
                    notify.notify_waiters();
                }
            }
        }

        if result.is_empty() {
            Err(LspError::NoServerForFile(file_path.display().to_string()))
        } else {
            Ok(result)
        }
    }

    /// Spawn and initialize a language server.
    async fn spawn_server(&self, config: LspConfig, root: PathBuf) -> LspResult<ServerConnection> {
        info!(language = %config.language, command = %config.command, "Spawning LSP server");

        // Create transport
        let transport = Arc::new(
            LspTransport::new(&config.command, &config.args, &config.env, Some(&root)).await?,
        );

        // Initialize
        let root_uri: Uri = format!("file://{}", root.display())
            .parse()
            .map_err(|e| LspError::InvalidUri(format!("{e}")))?;

        // Use workspace_folders (modern LSP) instead of deprecated root_uri
        let workspace_folder = WorkspaceFolder {
            uri: root_uri,
            name: root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("workspace")
                .to_string(),
        };

        let init_params = InitializeParams {
            process_id: Some(std::process::id()),
            workspace_folders: Some(vec![workspace_folder]),
            capabilities: lsp_types::ClientCapabilities {
                text_document: Some(lsp_types::TextDocumentClientCapabilities {
                    publish_diagnostics: Some(lsp_types::PublishDiagnosticsClientCapabilities {
                        related_information: Some(true),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let request = JsonRpcRequest::new(
            self.next_request_id(),
            "initialize",
            Some(serde_json::to_value(&init_params)?),
        );

        let response = transport.request(request).await?;

        if let Some(error) = response.error {
            return Err(LspError::InitializationFailed(error.message));
        }

        let init_result: InitializeResult = serde_json::from_value(
            response
                .result
                .ok_or_else(|| LspError::protocol_error("Missing initialize result"))?,
        )
        .map_err(|e| LspError::protocol_error(e.to_string()))?;

        debug!(language = %config.language, "LSP server initialized");

        // Send initialized notification
        let notification = JsonRpcNotification::new(
            "initialized",
            Some(serde_json::to_value(InitializedParams {})?),
        );
        transport.notify(notification).await?;

        Ok(ServerConnection {
            config,
            transport,
            capabilities: init_result.capabilities,
            root,
            diagnostics: RwLock::new(HashMap::new()),
            diagnostics_waiters: Mutex::new(HashMap::new()),
        })
    }

    /// Convert a file path to a URI.
    fn path_to_uri(path: &Path) -> LspResult<Uri> {
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| LspError::InvalidUri(e.to_string()))?
                .join(path)
        };

        let uri_string = format!("file://{}", abs_path.display());
        uri_string
            .parse()
            .map_err(|e| LspError::InvalidUri(format!("{}: {}", abs_path.display(), e)))
    }

    /// Touch a file (open it in LSP servers), optionally waiting for diagnostics.
    pub async fn touch_file(&self, file_path: &Path, wait_for_diagnostics: bool) -> LspResult<()> {
        debug!(file = %file_path.display(), wait = wait_for_diagnostics, "Touching file");

        let servers = match self.get_servers_for_file(file_path).await {
            Ok(s) => s,
            Err(_) => return Ok(()), // No applicable servers
        };

        let uri = Self::path_to_uri(file_path)?;
        let content = tokio::fs::read_to_string(file_path)
            .await
            .unwrap_or_default();

        // Detect language from extension
        let language_id = file_path
            .extension()
            .and_then(|e| e.to_str())
            .map(ext_to_language_id)
            .unwrap_or("plaintext");

        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: language_id.to_string(),
                version: 1,
                text: content,
            },
        };

        let mut waiters = Vec::new();

        for server in &servers {
            // Set up waiter before sending notification
            if wait_for_diagnostics {
                let (tx, rx) = oneshot::channel();
                let file_key = file_path.display().to_string();
                server
                    .diagnostics_waiters
                    .lock()
                    .await
                    .entry(file_key)
                    .or_default()
                    .push(tx);
                waiters.push(rx);
            }

            let notification = JsonRpcNotification::new(
                "textDocument/didOpen",
                Some(serde_json::to_value(&params)?),
            );

            if let Err(e) = server.transport.notify(notification).await {
                warn!(error = %e, "Failed to send didOpen notification");
            }
        }

        // Wait for diagnostics if requested
        if wait_for_diagnostics && !waiters.is_empty() {
            // Wait with timeout
            let timeout = tokio::time::Duration::from_secs(5);
            for waiter in waiters {
                let _ = tokio::time::timeout(timeout, waiter).await;
            }
        }

        Ok(())
    }

    /// Get all diagnostics from all connected servers.
    pub async fn diagnostics(&self) -> HashMap<String, Vec<DiagnosticInfo>> {
        let mut result: HashMap<String, Vec<DiagnosticInfo>> = HashMap::new();

        let servers = self.servers.read().await;
        for conn in servers.values() {
            let diags = conn.diagnostics.read().await;
            for (uri, diagnostics) in diags.iter() {
                let path = uri_to_path(uri);
                let infos: Vec<DiagnosticInfo> = diagnostics
                    .iter()
                    .map(|d| DiagnosticInfo::from_lsp(d, &path))
                    .collect();
                result.entry(path).or_default().extend(infos);
            }
        }

        result
    }

    /// Get LSP status for all servers.
    pub async fn status(&self) -> Vec<LspStatus> {
        let servers = self.servers.read().await;
        servers
            .values()
            .map(|conn| LspStatus {
                id: conn.config.language.clone(),
                name: conn.config.command.clone(),
                root: conn.root.display().to_string(),
                status: LspServerStatus::Connected,
            })
            .collect()
    }

    /// Go to definition.
    pub async fn goto_definition(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> LspResult<Vec<Location>> {
        let servers = self.get_servers_for_file(file_path).await?;
        let uri = Self::path_to_uri(file_path)?;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let mut all_locations = Vec::new();

        for server in servers {
            // Check if server supports definition
            if !server.supports_definition() {
                debug!(
                    server = %server.config.language,
                    "Server does not support textDocument/definition"
                );
                continue;
            }

            let request = JsonRpcRequest::new(
                self.next_request_id(),
                "textDocument/definition",
                Some(serde_json::to_value(&params)?),
            );

            match server.transport.request(request).await {
                Ok(response) => {
                    if response.error.is_none() {
                        if let Some(result) = response.result {
                            if let Ok(locs) = parse_goto_definition_response(result) {
                                all_locations.extend(locs);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Definition request failed");
                }
            }
        }

        Ok(all_locations)
    }

    /// Find references.
    pub async fn find_references(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
        include_declaration: bool,
    ) -> LspResult<Vec<Location>> {
        let servers = self.get_servers_for_file(file_path).await?;
        let uri = Self::path_to_uri(file_path)?;

        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration,
            },
        };

        let mut all_locations = Vec::new();

        for server in servers {
            // Check if server supports references
            if !server.supports_references() {
                debug!(
                    server = %server.config.language,
                    "Server does not support textDocument/references"
                );
                continue;
            }

            let request = JsonRpcRequest::new(
                self.next_request_id(),
                "textDocument/references",
                Some(serde_json::to_value(&params)?),
            );

            match server.transport.request(request).await {
                Ok(response) => {
                    if response.error.is_none() {
                        if let Some(result) = response.result {
                            if !result.is_null() {
                                if let Ok(locs) = serde_json::from_value::<Vec<Location>>(result) {
                                    all_locations.extend(locs);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "References request failed");
                }
            }
        }

        Ok(all_locations)
    }

    /// Go to implementation.
    pub async fn goto_implementation(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> LspResult<Vec<Location>> {
        let servers = self.get_servers_for_file(file_path).await?;
        let uri = Self::path_to_uri(file_path)?;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let mut all_locations = Vec::new();

        for server in servers {
            // Check if server supports implementation
            if !server.supports_implementation() {
                debug!(
                    server = %server.config.language,
                    "Server does not support textDocument/implementation"
                );
                continue;
            }

            let request = JsonRpcRequest::new(
                self.next_request_id(),
                "textDocument/implementation",
                Some(serde_json::to_value(&params)?),
            );

            match server.transport.request(request).await {
                Ok(response) => {
                    if response.error.is_none() {
                        if let Some(result) = response.result {
                            if let Ok(locs) = parse_goto_definition_response(result) {
                                all_locations.extend(locs);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Implementation request failed");
                }
            }
        }

        Ok(all_locations)
    }

    /// Workspace symbol search.
    pub async fn workspace_symbol(&self, query: &str) -> LspResult<Vec<SymbolInformation>> {
        let servers = self.servers.read().await;
        let mut all_symbols = Vec::new();

        let params = WorkspaceSymbolParams {
            query: query.to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        for server in servers.values() {
            // Check if server supports workspace symbol
            if !server.supports_workspace_symbol() {
                debug!(
                    server = %server.config.language,
                    "Server does not support workspace/symbol"
                );
                continue;
            }

            let request = JsonRpcRequest::new(
                self.next_request_id(),
                "workspace/symbol",
                Some(serde_json::to_value(&params)?),
            );

            match server.transport.request(request).await {
                Ok(response) => {
                    if response.error.is_none() {
                        if let Some(result) = response.result {
                            if let Ok(symbols) =
                                serde_json::from_value::<Vec<SymbolInformation>>(result)
                            {
                                // Filter by relevant kinds and limit
                                let filtered: Vec<_> = symbols
                                    .into_iter()
                                    .filter(|s| is_relevant_symbol_kind(s.kind))
                                    .take(10)
                                    .collect();
                                all_symbols.extend(filtered);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Workspace symbol request failed");
                }
            }
        }

        Ok(all_symbols)
    }

    /// Get document symbols.
    pub async fn document_symbols(&self, file_path: &Path) -> LspResult<Vec<DocumentSymbolInfo>> {
        let servers = self.get_servers_for_file(file_path).await?;
        let uri = Self::path_to_uri(file_path)?;

        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        for server in servers {
            // Check if server supports document symbol
            if !server.supports_document_symbol() {
                debug!(
                    server = %server.config.language,
                    "Server does not support textDocument/documentSymbol"
                );
                continue;
            }

            let request = JsonRpcRequest::new(
                self.next_request_id(),
                "textDocument/documentSymbol",
                Some(serde_json::to_value(&params)?),
            );

            match server.transport.request(request).await {
                Ok(response) => {
                    if response.error.is_none() {
                        if let Some(result) = response.result {
                            if !result.is_null() {
                                if let Ok(symbols) = parse_document_symbols(result) {
                                    return Ok(symbols);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Document symbols request failed");
                }
            }
        }

        Ok(Vec::new())
    }

    /// Get hover information.
    pub async fn hover(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> LspResult<Option<String>> {
        let servers = self.get_servers_for_file(file_path).await?;
        let uri = Self::path_to_uri(file_path)?;

        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        for server in servers {
            // Check if server supports hover
            if !server.supports_hover() {
                debug!(
                    server = %server.config.language,
                    "Server does not support textDocument/hover"
                );
                continue;
            }

            let request = JsonRpcRequest::new(
                self.next_request_id(),
                "textDocument/hover",
                Some(serde_json::to_value(&params)?),
            );

            match server.transport.request(request).await {
                Ok(response) => {
                    if response.error.is_none() {
                        if let Some(result) = response.result {
                            if !result.is_null() {
                                if let Ok(hover) = serde_json::from_value::<Hover>(result) {
                                    return Ok(Some(extract_hover_text(&hover)));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Hover request failed");
                }
            }
        }

        Ok(None)
    }

    /// Prepare call hierarchy (needed before incoming/outgoing calls).
    pub async fn prepare_call_hierarchy(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> LspResult<Vec<CallHierarchyItem>> {
        let servers = self.get_servers_for_file(file_path).await?;
        let uri = Self::path_to_uri(file_path)?;

        let params = CallHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: column,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        for server in servers {
            // Check if server supports call hierarchy
            if !server.supports_call_hierarchy() {
                debug!(
                    server = %server.config.language,
                    "Server does not support textDocument/prepareCallHierarchy"
                );
                continue;
            }

            let request = JsonRpcRequest::new(
                self.next_request_id(),
                "textDocument/prepareCallHierarchy",
                Some(serde_json::to_value(&params)?),
            );

            match server.transport.request(request).await {
                Ok(response) => {
                    if response.error.is_none() {
                        if let Some(result) = response.result {
                            if let Ok(items) =
                                serde_json::from_value::<Vec<CallHierarchyItem>>(result)
                            {
                                return Ok(items);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Prepare call hierarchy request failed");
                }
            }
        }

        Ok(Vec::new())
    }

    /// Get incoming calls for a call hierarchy item.
    pub async fn incoming_calls(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> LspResult<Vec<CallHierarchyIncomingCall>> {
        let items = self.prepare_call_hierarchy(file_path, line, column).await?;
        if items.is_empty() {
            return Ok(Vec::new());
        }

        let servers = self.get_servers_for_file(file_path).await?;
        let item = &items[0];

        for server in servers {
            let request = JsonRpcRequest::new(
                self.next_request_id(),
                "callHierarchy/incomingCalls",
                Some(serde_json::json!({ "item": item })),
            );

            match server.transport.request(request).await {
                Ok(response) => {
                    if response.error.is_none() {
                        if let Some(result) = response.result {
                            if let Ok(calls) =
                                serde_json::from_value::<Vec<CallHierarchyIncomingCall>>(result)
                            {
                                return Ok(calls);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Incoming calls request failed");
                }
            }
        }

        Ok(Vec::new())
    }

    /// Get outgoing calls for a call hierarchy item.
    pub async fn outgoing_calls(
        &self,
        file_path: &Path,
        line: u32,
        column: u32,
    ) -> LspResult<Vec<CallHierarchyOutgoingCall>> {
        let items = self.prepare_call_hierarchy(file_path, line, column).await?;
        if items.is_empty() {
            return Ok(Vec::new());
        }

        let servers = self.get_servers_for_file(file_path).await?;
        let item = &items[0];

        for server in servers {
            let request = JsonRpcRequest::new(
                self.next_request_id(),
                "callHierarchy/outgoingCalls",
                Some(serde_json::json!({ "item": item })),
            );

            match server.transport.request(request).await {
                Ok(response) => {
                    if response.error.is_none() {
                        if let Some(result) = response.result {
                            if let Ok(calls) =
                                serde_json::from_value::<Vec<CallHierarchyOutgoingCall>>(result)
                            {
                                return Ok(calls);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Outgoing calls request failed");
                }
            }
        }

        Ok(Vec::new())
    }

    /// Close all server connections.
    pub async fn close_all(&self) -> LspResult<()> {
        let mut servers = self.servers.write().await;
        for (name, conn) in servers.drain() {
            if let Err(e) = conn.transport.close().await {
                warn!(language = %name, error = %e, "Error closing server");
            }
        }
        Ok(())
    }

    /// List all connected servers with their status.
    pub async fn list_servers(&self) -> Vec<(String, String, Option<String>, bool)> {
        let servers = self.servers.read().await;
        servers
            .values()
            .map(|conn| {
                let name = conn.config.command.clone();
                let root = Some(conn.root.display().to_string());
                (conn.config.language.clone(), name, root, true)
            })
            .collect()
    }

    /// List available (configured but not connected) servers.
    pub fn list_available_servers(&self) -> Vec<(String, String, bool)> {
        self.configs
            .iter()
            .map(|c| (c.language.clone(), c.command.clone(), c.enabled))
            .collect()
    }
}

impl Default for LspClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Diagnostic information for display.
#[derive(Debug, Clone)]
pub struct DiagnosticInfo {
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub severity: DiagnosticSeverityLevel,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DiagnosticSeverityLevel {
    Error,
    Warning,
    Info,
    Hint,
}

impl DiagnosticInfo {
    fn from_lsp(diag: &Diagnostic, path: &str) -> Self {
        Self {
            path: path.to_string(),
            line: diag.range.start.line + 1,
            column: diag.range.start.character + 1,
            severity: match diag.severity {
                Some(DiagnosticSeverity::ERROR) => DiagnosticSeverityLevel::Error,
                Some(DiagnosticSeverity::WARNING) => DiagnosticSeverityLevel::Warning,
                Some(DiagnosticSeverity::INFORMATION) => DiagnosticSeverityLevel::Info,
                Some(DiagnosticSeverity::HINT) => DiagnosticSeverityLevel::Hint,
                _ => DiagnosticSeverityLevel::Error,
            },
            message: diag.message.clone(),
            source: diag.source.clone(),
        }
    }

    pub fn pretty(&self) -> String {
        let severity = match self.severity {
            DiagnosticSeverityLevel::Error => "ERROR",
            DiagnosticSeverityLevel::Warning => "WARN",
            DiagnosticSeverityLevel::Info => "INFO",
            DiagnosticSeverityLevel::Hint => "HINT",
        };
        format!(
            "{} [{}:{}] {}",
            severity, self.line, self.column, self.message
        )
    }
}

/// Simplified symbol information.
#[derive(Debug, Clone)]
pub struct DocumentSymbolInfo {
    pub name: String,
    pub kind: lsp_types::SymbolKind,
    pub range: lsp_types::Range,
    pub children: Vec<DocumentSymbolInfo>,
}

// Helper functions

fn ext_to_language_id(ext: &str) -> &'static str {
    match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" | "cxx" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        "lua" => "lua",
        "sh" | "bash" => "shellscript",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "md" => "markdown",
        "html" => "html",
        "css" => "css",
        "scss" => "scss",
        "sql" => "sql",
        _ => "plaintext",
    }
}

fn uri_to_path(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
}

fn is_relevant_symbol_kind(kind: lsp_types::SymbolKind) -> bool {
    matches!(
        kind,
        lsp_types::SymbolKind::CLASS
            | lsp_types::SymbolKind::FUNCTION
            | lsp_types::SymbolKind::METHOD
            | lsp_types::SymbolKind::INTERFACE
            | lsp_types::SymbolKind::VARIABLE
            | lsp_types::SymbolKind::CONSTANT
            | lsp_types::SymbolKind::STRUCT
            | lsp_types::SymbolKind::ENUM
    )
}

/// Parse goto definition response.
fn parse_goto_definition_response(value: Value) -> LspResult<Vec<Location>> {
    if value.is_null() {
        return Ok(Vec::new());
    }

    // Try as single Location
    if let Ok(loc) = serde_json::from_value::<Location>(value.clone()) {
        return Ok(vec![loc]);
    }

    // Try as Location[]
    if let Ok(locs) = serde_json::from_value::<Vec<Location>>(value.clone()) {
        return Ok(locs);
    }

    // Try as LocationLink[] and extract target locations
    if let Ok(links) = serde_json::from_value::<Vec<lsp_types::LocationLink>>(value) {
        let locs = links
            .into_iter()
            .map(|link| Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect();
        return Ok(locs);
    }

    Ok(Vec::new())
}

/// Parse document symbols response.
fn parse_document_symbols(value: Value) -> LspResult<Vec<DocumentSymbolInfo>> {
    // Try as DocumentSymbol[]
    if let Ok(symbols) = serde_json::from_value::<Vec<DocumentSymbol>>(value.clone()) {
        return Ok(symbols.into_iter().map(convert_document_symbol).collect());
    }

    // Try as SymbolInformation[]
    if let Ok(symbols) = serde_json::from_value::<Vec<SymbolInformation>>(value) {
        return Ok(symbols
            .into_iter()
            .map(|s| DocumentSymbolInfo {
                name: s.name,
                kind: s.kind,
                range: s.location.range,
                children: Vec::new(),
            })
            .collect());
    }

    Ok(Vec::new())
}

/// Convert DocumentSymbol to our simplified type.
fn convert_document_symbol(symbol: DocumentSymbol) -> DocumentSymbolInfo {
    DocumentSymbolInfo {
        name: symbol.name,
        kind: symbol.kind,
        range: symbol.range,
        children: symbol
            .children
            .unwrap_or_default()
            .into_iter()
            .map(convert_document_symbol)
            .collect(),
    }
}

/// Extract text from hover response.
fn extract_hover_text(hover: &Hover) -> String {
    match &hover.contents {
        lsp_types::HoverContents::Scalar(marked) => match marked {
            lsp_types::MarkedString::String(s) => s.clone(),
            lsp_types::MarkedString::LanguageString(ls) => ls.value.clone(),
        },
        lsp_types::HoverContents::Array(arr) => arr
            .iter()
            .map(|m| match m {
                lsp_types::MarkedString::String(s) => s.clone(),
                lsp_types::MarkedString::LanguageString(ls) => ls.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        lsp_types::HoverContents::Markup(markup) => markup.value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = LspClient::new();
        assert_eq!(client.next_request_id(), 1);
        assert_eq!(client.next_request_id(), 2);
    }

    #[test]
    fn test_client_with_defaults() {
        let client = LspClient::with_defaults();
        assert!(!client.configs.is_empty());
    }

    #[test]
    fn test_path_to_uri() {
        let uri = LspClient::path_to_uri(Path::new("/tmp/test.rs")).unwrap();
        assert!(uri.as_str().starts_with("file://"));
        assert!(uri.as_str().ends_with("/tmp/test.rs"));
    }

    #[test]
    fn test_server_key() {
        let key = LspClient::server_key("rust", Path::new("/home/user/project"));
        assert_eq!(key, "/home/user/project:rust");
    }

    #[test]
    fn test_diagnostic_pretty() {
        let diag = DiagnosticInfo {
            path: "/tmp/test.rs".to_string(),
            line: 10,
            column: 5,
            severity: DiagnosticSeverityLevel::Error,
            message: "cannot find value".to_string(),
            source: Some("rust-analyzer".to_string()),
        };
        assert_eq!(diag.pretty(), "ERROR [10:5] cannot find value");
    }

    #[test]
    fn test_ext_to_language_id() {
        assert_eq!(ext_to_language_id("rs"), "rust");
        assert_eq!(ext_to_language_id("ts"), "typescript");
        assert_eq!(ext_to_language_id("py"), "python");
        assert_eq!(ext_to_language_id("unknown"), "plaintext");
    }
}
