//! Mock implementations for testing.
//!
//! Provides mock implementations and test doubles to enable isolated unit testing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A mock command executor for testing tool execution without actual shell execution.
///
/// Records all commands executed and returns configurable responses.
///
/// # Example
///
/// ```rust
/// use wonopcode_test_utils::mocks::MockCommandExecutor;
///
/// let executor = MockCommandExecutor::new()
///     .with_response("echo hello", Ok("hello\n".to_string()))
///     .with_response("ls", Ok("file1.txt\nfile2.txt\n".to_string()));
///
/// // Use in tests...
/// let result = executor.execute("echo hello");
/// assert_eq!(result.unwrap(), "hello\n");
/// ```
#[derive(Clone)]
pub struct MockCommandExecutor {
    /// Recorded command executions.
    executions: Arc<Mutex<Vec<CommandExecution>>>,
    /// Configured responses (command -> result).
    responses: Arc<Mutex<HashMap<String, Result<String, String>>>>,
    /// Default response when no specific response is configured.
    default_response: Arc<Mutex<Option<Result<String, String>>>>,
    /// Working directory.
    workdir: PathBuf,
}

/// A recorded command execution.
#[derive(Debug, Clone)]
pub struct CommandExecution {
    /// The command that was executed.
    pub command: String,
    /// The working directory at execution time.
    pub workdir: PathBuf,
    /// Environment variables passed.
    pub env: HashMap<String, String>,
    /// Timeout in milliseconds (if specified).
    pub timeout_ms: Option<u64>,
}

impl MockCommandExecutor {
    /// Create a new mock command executor.
    pub fn new() -> Self {
        Self {
            executions: Arc::new(Mutex::new(Vec::new())),
            responses: Arc::new(Mutex::new(HashMap::new())),
            default_response: Arc::new(Mutex::new(None)),
            workdir: PathBuf::from("/mock/workdir"),
        }
    }

    /// Configure a response for a specific command.
    pub fn with_response(self, command: &str, response: Result<String, String>) -> Self {
        self.responses
            .lock()
            .unwrap()
            .insert(command.to_string(), response);
        self
    }

    /// Configure a default response for unmatched commands.
    pub fn with_default_response(self, response: Result<String, String>) -> Self {
        *self.default_response.lock().unwrap() = Some(response);
        self
    }

    /// Set the working directory.
    pub fn with_workdir(mut self, workdir: impl Into<PathBuf>) -> Self {
        self.workdir = workdir.into();
        self
    }

    /// Execute a command and return the configured response.
    pub fn execute(&self, command: &str) -> Result<String, String> {
        self.execute_with_options(command, None, None, None)
    }

    /// Execute a command with options.
    pub fn execute_with_options(
        &self,
        command: &str,
        workdir: Option<&Path>,
        env: Option<&HashMap<String, String>>,
        timeout_ms: Option<u64>,
    ) -> Result<String, String> {
        // Record the execution
        let execution = CommandExecution {
            command: command.to_string(),
            workdir: workdir
                .map(PathBuf::from)
                .unwrap_or_else(|| self.workdir.clone()),
            env: env.cloned().unwrap_or_default(),
            timeout_ms,
        };
        self.executions.lock().unwrap().push(execution);

        // Find a matching response
        let responses = self.responses.lock().unwrap();

        // Try exact match first
        if let Some(response) = responses.get(command) {
            return response.clone();
        }

        // Try prefix match
        for (cmd, response) in responses.iter() {
            if command.starts_with(cmd) {
                return response.clone();
            }
        }

        drop(responses);

        // Use default response
        let default = self.default_response.lock().unwrap();
        match &*default {
            Some(response) => response.clone(),
            None => Ok(String::new()),
        }
    }

    /// Get all recorded command executions.
    pub fn executions(&self) -> Vec<CommandExecution> {
        self.executions.lock().unwrap().clone()
    }

    /// Get the number of commands executed.
    pub fn execution_count(&self) -> usize {
        self.executions.lock().unwrap().len()
    }

    /// Clear recorded executions.
    pub fn clear_executions(&self) {
        self.executions.lock().unwrap().clear();
    }

    /// Check if a specific command was executed.
    pub fn was_executed(&self, command: &str) -> bool {
        self.executions
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.command.contains(command))
    }

    /// Get the last executed command.
    pub fn last_execution(&self) -> Option<CommandExecution> {
        self.executions.lock().unwrap().last().cloned()
    }

    /// Get the working directory.
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }
}

impl Default for MockCommandExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating mock file system state.
#[derive(Default)]
pub struct MockFileSystem {
    files: HashMap<PathBuf, String>,
    directories: Vec<PathBuf>,
}

impl MockFileSystem {
    /// Create a new mock file system.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a file with content.
    pub fn with_file(mut self, path: impl AsRef<Path>, content: impl Into<String>) -> Self {
        self.files
            .insert(path.as_ref().to_path_buf(), content.into());
        self
    }

    /// Add a directory.
    pub fn with_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.directories.push(path.as_ref().to_path_buf());
        self
    }

    /// Check if a file exists.
    pub fn exists(&self, path: impl AsRef<Path>) -> bool {
        self.files.contains_key(path.as_ref())
            || self.directories.contains(&path.as_ref().to_path_buf())
    }

    /// Read file content.
    pub fn read(&self, path: impl AsRef<Path>) -> Option<&str> {
        self.files.get(path.as_ref()).map(|s| s.as_str())
    }

    /// Write file content.
    pub fn write(&mut self, path: impl AsRef<Path>, content: impl Into<String>) {
        self.files
            .insert(path.as_ref().to_path_buf(), content.into());
    }

    /// Delete a file.
    pub fn delete(&mut self, path: impl AsRef<Path>) -> bool {
        self.files.remove(path.as_ref()).is_some()
    }

    /// List files in a directory.
    pub fn list(&self, dir: impl AsRef<Path>) -> Vec<&Path> {
        let dir = dir.as_ref();
        self.files
            .keys()
            .filter(|p| p.parent() == Some(dir))
            .map(|p| p.as_path())
            .collect()
    }

    /// Get all files.
    pub fn all_files(&self) -> Vec<&Path> {
        self.files.keys().map(|p| p.as_path()).collect()
    }

    /// Get all directories.
    pub fn all_directories(&self) -> Vec<&Path> {
        self.directories.iter().map(|p| p.as_path()).collect()
    }
}

/// A simple mock HTTP client for testing.
#[derive(Default)]
pub struct MockHttpClient {
    responses: HashMap<String, MockHttpResponse>,
    requests: Arc<Mutex<Vec<MockHttpRequest>>>,
}

/// A recorded HTTP request.
#[derive(Debug, Clone)]
pub struct MockHttpRequest {
    /// The URL that was requested.
    pub url: String,
    /// The HTTP method.
    pub method: String,
    /// Request headers.
    pub headers: HashMap<String, String>,
    /// Request body (if any).
    pub body: Option<String>,
}

/// A mock HTTP response.
#[derive(Debug, Clone)]
pub struct MockHttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers.
    pub headers: HashMap<String, String>,
    /// Response body.
    pub body: String,
}

impl MockHttpResponse {
    /// Create a successful JSON response.
    pub fn json(body: impl Into<String>) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        Self {
            status: 200,
            headers,
            body: body.into(),
        }
    }

    /// Create a successful text response.
    pub fn text(body: impl Into<String>) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "text/plain".to_string());
        Self {
            status: 200,
            headers,
            body: body.into(),
        }
    }

    /// Create an error response.
    pub fn error(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: message.into(),
        }
    }
}

impl MockHttpClient {
    /// Create a new mock HTTP client.
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure a response for a URL.
    pub fn with_response(mut self, url: &str, response: MockHttpResponse) -> Self {
        self.responses.insert(url.to_string(), response);
        self
    }

    /// Simulate a GET request.
    pub fn get(&self, url: &str) -> Option<MockHttpResponse> {
        self.record_request(url, "GET", None);
        self.responses.get(url).cloned()
    }

    /// Simulate a POST request.
    pub fn post(&self, url: &str, body: &str) -> Option<MockHttpResponse> {
        self.record_request(url, "POST", Some(body));
        self.responses.get(url).cloned()
    }

    fn record_request(&self, url: &str, method: &str, body: Option<&str>) {
        let request = MockHttpRequest {
            url: url.to_string(),
            method: method.to_string(),
            headers: HashMap::new(),
            body: body.map(String::from),
        };
        self.requests.lock().unwrap().push(request);
    }

    /// Get all recorded requests.
    pub fn requests(&self) -> Vec<MockHttpRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// Check if a URL was requested.
    pub fn was_requested(&self, url: &str) -> bool {
        self.requests.lock().unwrap().iter().any(|r| r.url == url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_command_executor() {
        let executor =
            MockCommandExecutor::new().with_response("echo hello", Ok("hello\n".to_string()));

        let result = executor.execute("echo hello");
        assert_eq!(result.unwrap(), "hello\n");
        assert_eq!(executor.execution_count(), 1);
    }

    #[test]
    fn test_mock_command_executor_prefix_match() {
        let executor =
            MockCommandExecutor::new().with_response("git", Ok("git output".to_string()));

        // "git status" should match "git" prefix
        let result = executor.execute("git status");
        assert_eq!(result.unwrap(), "git output");
    }

    #[test]
    fn test_mock_command_executor_with_workdir() {
        let executor = MockCommandExecutor::new().with_workdir("/custom/workdir");
        assert_eq!(executor.workdir(), Path::new("/custom/workdir"));
    }

    #[test]
    fn test_mock_command_executor_default() {
        let executor = MockCommandExecutor::default();
        assert_eq!(executor.execution_count(), 0);
    }

    #[test]
    fn test_mock_command_executor_was_executed() {
        let executor = MockCommandExecutor::new();
        let _ = executor.execute("test command");
        assert!(executor.was_executed("test"));
        assert!(!executor.was_executed("other"));
    }

    #[test]
    fn test_mock_command_executor_last_execution() {
        let executor = MockCommandExecutor::new();
        let _ = executor.execute("first");
        let _ = executor.execute("second");

        let last = executor.last_execution().unwrap();
        assert_eq!(last.command, "second");
    }

    #[test]
    fn test_mock_command_executor_clear_executions() {
        let executor = MockCommandExecutor::new();
        let _ = executor.execute("test");
        assert_eq!(executor.execution_count(), 1);

        executor.clear_executions();
        assert_eq!(executor.execution_count(), 0);
    }

    #[test]
    fn test_mock_command_executor_executions() {
        let executor = MockCommandExecutor::new();
        let _ = executor.execute("cmd1");
        let _ = executor.execute("cmd2");

        let executions = executor.executions();
        assert_eq!(executions.len(), 2);
        assert_eq!(executions[0].command, "cmd1");
        assert_eq!(executions[1].command, "cmd2");
    }

    #[test]
    fn test_mock_command_executor_with_options() {
        let executor = MockCommandExecutor::new().with_response("test", Ok("output".to_string()));

        let mut env = HashMap::new();
        env.insert("KEY".to_string(), "VALUE".to_string());

        let result = executor.execute_with_options(
            "test",
            Some(Path::new("/custom/dir")),
            Some(&env),
            Some(5000),
        );

        assert!(result.is_ok());

        let last = executor.last_execution().unwrap();
        assert_eq!(last.workdir, PathBuf::from("/custom/dir"));
        assert_eq!(last.env.get("KEY"), Some(&"VALUE".to_string()));
        assert_eq!(last.timeout_ms, Some(5000));
    }

    #[test]
    fn test_mock_command_executor_no_default_returns_empty() {
        let executor = MockCommandExecutor::new();
        let result = executor.execute("unknown");
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn test_mock_command_default_response() {
        let executor =
            MockCommandExecutor::new().with_default_response(Ok("default output".to_string()));

        let result = executor.execute("any command");
        assert_eq!(result.unwrap(), "default output");
    }

    #[test]
    fn test_mock_command_error_response() {
        let executor =
            MockCommandExecutor::new().with_response("fail", Err("command failed".to_string()));

        let result = executor.execute("fail");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "command failed");
    }

    #[test]
    fn test_mock_filesystem() {
        let mut fs = MockFileSystem::new()
            .with_file("/test/file.txt", "content")
            .with_dir("/test/subdir");

        assert!(fs.exists("/test/file.txt"));
        assert!(fs.exists("/test/subdir"));
        assert!(!fs.exists("/nonexistent"));

        assert_eq!(fs.read("/test/file.txt"), Some("content"));

        fs.write("/test/new.txt", "new content");
        assert_eq!(fs.read("/test/new.txt"), Some("new content"));

        assert!(fs.delete("/test/new.txt"));
        assert!(!fs.exists("/test/new.txt"));
    }

    #[test]
    fn test_mock_filesystem_list() {
        let fs = MockFileSystem::new()
            .with_file("/dir/file1.txt", "1")
            .with_file("/dir/file2.txt", "2")
            .with_file("/other/file3.txt", "3");

        let files = fs.list("/dir");
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_mock_filesystem_all_files() {
        let fs = MockFileSystem::new()
            .with_file("/a.txt", "a")
            .with_file("/b.txt", "b");

        let all = fs.all_files();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_mock_filesystem_all_directories() {
        let fs = MockFileSystem::new().with_dir("/dir1").with_dir("/dir2");

        let dirs = fs.all_directories();
        assert_eq!(dirs.len(), 2);
    }

    #[test]
    fn test_mock_filesystem_delete_nonexistent() {
        let mut fs = MockFileSystem::new();
        assert!(!fs.delete("/nonexistent"));
    }

    #[test]
    fn test_mock_http_client() {
        let client = MockHttpClient::new()
            .with_response("/api/test", MockHttpResponse::json(r#"{"ok": true}"#));

        let response = client.get("/api/test").unwrap();
        assert_eq!(response.status, 200);
        assert!(response.body.contains("ok"));
        assert!(client.was_requested("/api/test"));
    }

    #[test]
    fn test_mock_http_client_post() {
        let client = MockHttpClient::new().with_response(
            "/api/create",
            MockHttpResponse::json(r#"{"created": true}"#),
        );

        let response = client.post("/api/create", r#"{"name": "test"}"#).unwrap();
        assert_eq!(response.status, 200);

        let requests = client.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].body, Some(r#"{"name": "test"}"#.to_string()));
    }

    #[test]
    fn test_mock_http_response_text() {
        let response = MockHttpResponse::text("plain text");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, "plain text");
        assert_eq!(
            response.headers.get("content-type"),
            Some(&"text/plain".to_string())
        );
    }

    #[test]
    fn test_mock_http_response_error() {
        let response = MockHttpResponse::error(404, "Not found");
        assert_eq!(response.status, 404);
        assert_eq!(response.body, "Not found");
    }

    #[test]
    fn test_mock_http_client_get_nonexistent() {
        let client = MockHttpClient::new();
        let response = client.get("/unknown");
        assert!(response.is_none());
    }

    #[test]
    fn test_mock_http_client_was_requested_false() {
        let client = MockHttpClient::new();
        assert!(!client.was_requested("/anything"));
    }
}
