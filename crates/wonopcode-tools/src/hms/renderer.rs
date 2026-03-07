//! Template rendering for AGENTS.md generation.
//!
//! Uses Tera templates to render `AGENTS.TEMPLATE.md` files with memory context.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tera::{Context, Tera, Value};
use tokio::fs;
use tracing::debug;

use super::{CachedResolver, HmsError, MemoryValue, ResolvedMemory};

/// Renders AGENTS.md from templates and memory context.
pub struct TemplateRenderer {
    tera: Tera,
}

impl Default for TemplateRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateRenderer {
    /// Create a new template renderer with custom filters.
    pub fn new() -> Self {
        let mut tera = Tera::default();

        // Register custom filters
        tera.register_filter("bullet_list", filter_bullet_list);
        tera.register_filter("numbered_list", filter_numbered_list);
        tera.register_filter("code_block", filter_code_block);

        Self { tera }
    }

    /// Find the nearest AGENTS.TEMPLATE.md walking up the tree.
    pub fn find_template(&self, start_dir: &Path, project_root: &Path) -> Option<PathBuf> {
        let mut current = start_dir.to_path_buf();

        loop {
            // Check .wonopcode-local first (higher priority)
            let local_template = current.join(".wonopcode-local/AGENTS.TEMPLATE.md");
            if local_template.exists() {
                return Some(local_template);
            }

            // Then check .wonopcode
            let template_path = current.join(".wonopcode/AGENTS.TEMPLATE.md");
            if template_path.exists() {
                return Some(template_path);
            }

            // Stop at project root
            if current == project_root {
                break;
            }

            // Move up
            if !current.pop() {
                break;
            }
        }

        None
    }

    /// Render a template with the given context.
    pub fn render(&mut self, template_content: &str, context: &Context) -> Result<String, HmsError> {
        self.tera
            .add_raw_template("agents", template_content)
            .map_err(|e| HmsError::TemplateError(e.to_string()))?;

        self.tera
            .render("agents", context)
            .map_err(|e| HmsError::TemplateError(e.to_string()))
    }

    /// Render from a template file.
    pub async fn render_file(
        &mut self,
        template_path: &Path,
        context: &Context,
    ) -> Result<String, HmsError> {
        let content = fs::read_to_string(template_path).await?;
        self.render(&content, context)
    }
}

/// Build Tera context from resolved memories.
pub fn build_template_context(
    memories: &[ResolvedMemory],
    target_dir: &Path,
    project_root: &Path,
) -> Context {
    let mut context = Context::new();

    // Add individual memory entries as top-level variables
    for memory in memories {
        let value = memory_value_to_tera(&memory.value);
        context.insert(&memory.key, &value);
    }

    // Add structured access to all memories
    let memories_map: HashMap<String, MemoryInfo> = memories
        .iter()
        .map(|m| {
            (
                m.key.clone(),
                MemoryInfo {
                    value: memory_value_to_tera(&m.value),
                    visibility: m.visibility.to_string(),
                    source: m.source_path.display().to_string(),
                    description: m.description.clone(),
                    tags: m.tags.clone(),
                },
            )
        })
        .collect();
    context.insert("memories", &memories_map);

    // Add metadata
    context.insert("__dir__", &target_dir.display().to_string());
    context.insert("__project_root__", &project_root.display().to_string());
    context.insert(
        "__relative_path__",
        &target_dir
            .strip_prefix(project_root)
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    );

    context
}

/// Memory info for template access.
#[derive(Debug, Clone, serde::Serialize)]
struct MemoryInfo {
    value: Value,
    visibility: String,
    source: String,
    description: Option<String>,
    tags: Vec<String>,
}

/// Convert MemoryValue to Tera Value.
fn memory_value_to_tera(value: &MemoryValue) -> Value {
    match value {
        MemoryValue::String(s) => Value::String(s.clone()),
        MemoryValue::List(items) => {
            Value::Array(items.iter().map(|s| Value::String(s.clone())).collect())
        }
        MemoryValue::Map(map) => {
            // Convert serde_yaml::Mapping to tera::Value via serde_json
            serde_json::to_value(map)
                .ok()
                .and_then(|v| tera::to_value(v).ok())
                .unwrap_or(Value::Null)
        }
    }
}

/// Filter: Convert list to bullet points.
fn filter_bullet_list(value: &Value, _args: &HashMap<String, Value>) -> tera::Result<Value> {
    match value {
        Value::Array(items) => {
            let bullets: String = items
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| format!("- {}", s))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(Value::String(bullets))
        }
        Value::String(s) => Ok(Value::String(format!("- {}", s))),
        _ => Ok(value.clone()),
    }
}

/// Filter: Convert list to numbered points.
fn filter_numbered_list(value: &Value, _args: &HashMap<String, Value>) -> tera::Result<Value> {
    match value {
        Value::Array(items) => {
            let numbered: String = items
                .iter()
                .enumerate()
                .filter_map(|(i, v)| v.as_str().map(|s| (i, s)))
                .map(|(i, s)| format!("{}. {}", i + 1, s))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(Value::String(numbered))
        }
        _ => Ok(value.clone()),
    }
}

/// Filter: Wrap in code block.
fn filter_code_block(value: &Value, args: &HashMap<String, Value>) -> tera::Result<Value> {
    let lang = args.get("lang").and_then(|v| v.as_str()).unwrap_or("");
    let content = value.as_str().unwrap_or("");
    Ok(Value::String(format!("```{}\n{}\n```", lang, content)))
}

/// Generates AGENTS.md files from templates and memories.
pub struct AgentsGenerator {
    resolver: CachedResolver,
    renderer: TemplateRenderer,
}

impl AgentsGenerator {
    /// Create a new generator.
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            resolver: CachedResolver::new(project_root),
            renderer: TemplateRenderer::new(),
        }
    }

    /// Get the resolver for direct access.
    pub fn resolver(&self) -> &CachedResolver {
        &self.resolver
    }

    /// Generate AGENTS.md content for a directory.
    pub async fn generate(
        &mut self,
        target_dir: &Path,
        project_root: &Path,
    ) -> Result<String, HmsError> {
        // 1. Resolve all memories visible at target
        let memories = self.resolver.resolve(target_dir).await?;
        debug!(
            count = memories.len(),
            target = ?target_dir,
            "Resolved memories for rendering"
        );

        // 2. Find template
        let template_path = self
            .renderer
            .find_template(target_dir, project_root)
            .ok_or(HmsError::NoTemplateFound)?;
        debug!(?template_path, "Found template");

        // 3. Build context
        let context = build_template_context(&memories, target_dir, project_root);

        // 4. Render
        self.renderer.render_file(&template_path, &context).await
    }

    /// Write AGENTS.md to disk.
    ///
    /// This writes to three locations for compatibility:
    /// 1. `.wonopcode/AGENTS.md` - Our standard location
    /// 2. `CLAUDE.md` at target directory root - For Claude Code CLI compatibility
    /// 3. `.wonopcode/CLAUDE.md` - Alternative location some tools check
    pub async fn write_agents_md(
        &mut self,
        target_dir: &Path,
        project_root: &Path,
    ) -> Result<PathBuf, HmsError> {
        let content = self.generate(target_dir, project_root).await?;
        
        // Primary location: .wonopcode/AGENTS.md
        let agents_path = target_dir.join(".wonopcode/AGENTS.md");
        if let Some(parent) = agents_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&agents_path, &content).await?;
        debug!(?agents_path, "Wrote AGENTS.md");

        // Claude Code compatibility: CLAUDE.md at root
        // Claude Code (CLI) looks for CLAUDE.md in the working directory root
        let claude_root_path = target_dir.join("CLAUDE.md");
        fs::write(&claude_root_path, &content).await?;
        debug!(?claude_root_path, "Wrote CLAUDE.md (root)");
        
        // Also write to .wonopcode/CLAUDE.md for consistency
        let claude_wonopcode_path = target_dir.join(".wonopcode/CLAUDE.md");
        fs::write(&claude_wonopcode_path, &content).await?;
        debug!(?claude_wonopcode_path, "Wrote CLAUDE.md (.wonopcode)");

        Ok(agents_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hms::MemoryEntryData;
    use tempfile::TempDir;

    fn create_test_memory(key: &str, value: &str) -> ResolvedMemory {
        ResolvedMemory::from_entry(
            key.to_string(),
            &MemoryEntryData::new(value),
            PathBuf::from("/test/memory.yaml"),
            super::super::StorageClass::Tracked,
        )
    }

    #[test]
    fn test_build_context_basic() {
        let memories = vec![create_test_memory("project_name", "TestProject")];
        let context = build_template_context(
            &memories,
            Path::new("/project/src"),
            Path::new("/project"),
        );

        assert!(context.get("project_name").is_some());
        assert!(context.get("memories").is_some());
        assert!(context.get("__dir__").is_some());
    }

    #[test]
    fn test_filter_bullet_list() {
        let value = Value::Array(vec![
            Value::String("item1".to_string()),
            Value::String("item2".to_string()),
        ]);
        let result = filter_bullet_list(&value, &HashMap::new()).unwrap();
        assert_eq!(result.as_str().unwrap(), "- item1\n- item2");
    }

    #[test]
    fn test_filter_numbered_list() {
        let value = Value::Array(vec![
            Value::String("first".to_string()),
            Value::String("second".to_string()),
        ]);
        let result = filter_numbered_list(&value, &HashMap::new()).unwrap();
        assert_eq!(result.as_str().unwrap(), "1. first\n2. second");
    }

    #[test]
    fn test_filter_code_block() {
        let value = Value::String("fn main() {}".to_string());
        let mut args = HashMap::new();
        args.insert("lang".to_string(), Value::String("rust".to_string()));
        let result = filter_code_block(&value, &args).unwrap();
        assert_eq!(
            result.as_str().unwrap(),
            "```rust\nfn main() {}\n```"
        );
    }

    #[test]
    fn test_render_basic() {
        let mut renderer = TemplateRenderer::new();
        let mut context = Context::new();
        context.insert("name", "World");

        let result = renderer.render("Hello, {{ name }}!", &context).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_render_with_default() {
        let mut renderer = TemplateRenderer::new();
        let context = Context::new();

        let result = renderer
            .render("{{ missing | default(value='Default') }}", &context)
            .unwrap();
        assert_eq!(result, "Default");
    }

    #[test]
    fn test_render_with_bullet_filter() {
        let mut renderer = TemplateRenderer::new();
        let mut context = Context::new();
        context.insert("items", &vec!["a", "b", "c"]);

        let result = renderer
            .render("{{ items | bullet_list }}", &context)
            .unwrap();
        assert_eq!(result, "- a\n- b\n- c");
    }

    #[tokio::test]
    async fn test_find_template() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let subdir = root.join("src/api");

        // Create directories
        fs::create_dir_all(&subdir).await.unwrap();
        fs::create_dir_all(root.join(".wonopcode")).await.unwrap();

        // Create template at root
        fs::write(
            root.join(".wonopcode/AGENTS.TEMPLATE.md"),
            "# Template",
        )
        .await
        .unwrap();

        let renderer = TemplateRenderer::new();
        let found = renderer.find_template(&subdir, root);

        assert!(found.is_some());
        assert!(found.unwrap().ends_with("AGENTS.TEMPLATE.md"));
    }

    #[tokio::test]
    async fn test_find_template_local_override() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join(".wonopcode")).await.unwrap();
        fs::create_dir_all(root.join(".wonopcode-local"))
            .await
            .unwrap();

        // Create both templates
        fs::write(root.join(".wonopcode/AGENTS.TEMPLATE.md"), "# Tracked")
            .await
            .unwrap();
        fs::write(
            root.join(".wonopcode-local/AGENTS.TEMPLATE.md"),
            "# Local",
        )
        .await
        .unwrap();

        let renderer = TemplateRenderer::new();
        let found = renderer.find_template(root, root).unwrap();

        // Should prefer local
        assert!(found.to_string_lossy().contains("wonopcode-local"));
    }
}
