//! Skill tool for loading specialized instructions from skill files.
//!
//! Skills are Markdown files with YAML frontmatter that provide specialized
//! instructions for specific tasks.

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::fs;
use tracing::{debug, warn};

/// A skill definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Skill name (identifier).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Full path to the skill file.
    pub location: PathBuf,
    /// The skill content (markdown body).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Skill frontmatter in YAML format.
#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
}

/// Skill registry for managing discovered skills.
#[derive(Debug, Default)]
pub struct SkillRegistry {
    skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Discover skills from the given directories.
    ///
    /// Looks for SKILL.md files in:
    /// - `{dir}/.wonopcode/skills/**/SKILL.md`
    /// - `{dir}/.claude/skills/**/SKILL.md`
    /// - `{dir}/skill/**/SKILL.md` (config directory)
    pub async fn discover(directories: &[PathBuf]) -> Self {
        let mut registry = Self::new();

        for dir in directories {
            // .wonopcode/skills pattern
            let wonopcode_skills = dir.join(".wonopcode").join("skills");
            if wonopcode_skills.exists() {
                registry.scan_directory(&wonopcode_skills).await;
            }

            // .claude/skills pattern (compatibility)
            let claude_skills = dir.join(".claude").join("skills");
            if claude_skills.exists() {
                registry.scan_directory(&claude_skills).await;
            }

            // skill/ pattern (for config directories)
            let skill_dir = dir.join("skill");
            if skill_dir.exists() {
                registry.scan_directory(&skill_dir).await;
            }
        }

        debug!(count = registry.skills.len(), "Skills discovered");
        registry
    }

    /// Scan a directory for SKILL.md files.
    async fn scan_directory(&mut self, dir: &Path) {
        let walker = walkdir::WalkDir::new(dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok());

        for entry in walker {
            if entry.file_name() == "SKILL.md" {
                match Self::load_skill(entry.path()).await {
                    Ok(skill) => {
                        debug!(name = %skill.name, path = %skill.location.display(), "Loaded skill");
                        self.skills.insert(skill.name.clone(), skill);
                    }
                    Err(e) => {
                        warn!(path = %entry.path().display(), error = %e, "Failed to load skill");
                    }
                }
            }
        }
    }

    /// Load a skill from a SKILL.md file.
    async fn load_skill(path: &Path) -> Result<Skill, String> {
        let content = fs::read_to_string(path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?;

        let (frontmatter, body) = parse_frontmatter(&content)?;

        Ok(Skill {
            name: frontmatter.name,
            description: frontmatter.description,
            location: path.to_path_buf(),
            content: Some(body),
        })
    }

    /// Get a skill by name.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// Get a skill with full content loaded.
    pub async fn get_with_content(&self, name: &str) -> Option<Skill> {
        let skill = self.skills.get(name)?;

        // If content is already loaded, return it
        if skill.content.is_some() {
            return Some(skill.clone());
        }

        // Otherwise, load the content
        match Self::load_skill(&skill.location).await {
            Ok(loaded) => Some(loaded),
            Err(e) => {
                warn!(name = %name, error = %e, "Failed to load skill content");
                Some(skill.clone())
            }
        }
    }

    /// List all available skills.
    pub fn list(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// Get skill names.
    pub fn names(&self) -> Vec<&str> {
        self.skills.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a skill exists.
    pub fn contains(&self, name: &str) -> bool {
        self.skills.contains_key(name)
    }

    /// Get the number of skills.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Format available skills as XML for the AI.
    pub fn format_available_skills(&self) -> String {
        if self.skills.is_empty() {
            return "No skills are currently available.".to_string();
        }

        let mut output = String::from("<available_skills>\n");
        for skill in self.skills.values() {
            output.push_str(&format!(
                "  <skill>\n    <name>{}</name>\n    <description>{}</description>\n  </skill>\n",
                skill.name, skill.description
            ));
        }
        output.push_str("</available_skills>");
        output
    }
}

/// Parse YAML frontmatter from markdown content.
fn parse_frontmatter(content: &str) -> Result<(SkillFrontmatter, String), String> {
    let content = content.trim();

    // Check for frontmatter delimiter
    if !content.starts_with("---") {
        return Err("Missing frontmatter delimiter (---)".to_string());
    }

    // Find the end of frontmatter
    let rest = &content[3..];
    let end_idx = rest
        .find("\n---")
        .ok_or("Missing closing frontmatter delimiter")?;

    let frontmatter_str = &rest[..end_idx].trim();
    let body = rest[end_idx + 4..].trim();

    // Parse YAML frontmatter
    let frontmatter: SkillFrontmatter = serde_yaml::from_str(frontmatter_str)
        .map_err(|e| format!("Invalid frontmatter YAML: {}", e))?;

    Ok((frontmatter, body.to_string()))
}

/// Skill tool for loading skill definitions.
pub struct SkillTool {
    registry: Arc<RwLock<SkillRegistry>>,
}

impl SkillTool {
    /// Create a new skill tool with a pre-loaded registry.
    pub fn new(registry: Arc<RwLock<SkillRegistry>>) -> Self {
        Self { registry }
    }

    /// Create a skill tool that will discover skills from the given directories.
    pub async fn discover(directories: &[PathBuf]) -> Self {
        let registry = SkillRegistry::discover(directories).await;
        Self {
            registry: Arc::new(RwLock::new(registry)),
        }
    }

    /// Get a reference to the skill registry.
    pub fn registry(&self) -> Arc<RwLock<SkillRegistry>> {
        self.registry.clone()
    }
}

#[derive(Debug, Deserialize)]
struct SkillArgs {
    /// The skill identifier to load.
    name: String,
}

#[async_trait]
impl Tool for SkillTool {
    fn id(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        r#"Load a skill to get detailed instructions for a specific task.

Skills provide specialized knowledge and step-by-step guidance for complex tasks.
Use this when a task matches an available skill's description.

Available skills will be listed in the tool parameters."#
    }

    fn parameters_schema(&self) -> Value {
        // Get available skill names for the enum
        // If lock is poisoned, return schema with no enum (still allows arbitrary input)
        let skill_names: Vec<String> = self
            .registry
            .read()
            .ok()
            .map(|r| r.names().into_iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();

        if skill_names.is_empty() {
            json!({
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The skill identifier. No skills are currently available."
                    }
                }
            })
        } else {
            json!({
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The skill identifier from available_skills",
                        "enum": skill_names
                    }
                }
            })
        }
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: SkillArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {}", e)))?;

        // First, get the skill info from the registry (sync operation)
        let skill_info = {
            let registry = self.registry.read().map_err(|e| {
                ToolError::execution_failed(format!("Failed to access skill registry: {}", e))
            })?;
            registry.get(&args.name).cloned()
        };

        let skill = match skill_info {
            Some(mut skill) => {
                // If content not loaded yet, load it now (async)
                if skill.content.is_none() {
                    if let Ok(loaded) = SkillRegistry::load_skill(&skill.location).await {
                        skill = loaded;
                    }
                }
                skill
            }
            None => {
                let available = {
                    let registry = self.registry.read().map_err(|e| {
                        ToolError::execution_failed(format!(
                            "Failed to access skill registry: {}",
                            e
                        ))
                    })?;
                    registry.names().join(", ")
                };
                return Err(ToolError::validation(format!(
                    "Skill '{}' not found. Available skills: {}",
                    args.name,
                    if available.is_empty() {
                        "none"
                    } else {
                        &available
                    }
                )));
            }
        };

        let content = skill
            .content
            .unwrap_or_else(|| "Skill content could not be loaded.".to_string());

        // Get the base directory of the skill (parent of SKILL.md)
        let base_dir = skill
            .location
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        let output = format!(
            "# Skill: {}\n\n**Description:** {}\n\n**Base Directory:** {}\n\n---\n\n{}",
            skill.name, skill.description, base_dir, content
        );

        Ok(
            ToolOutput::new(format!("Skill: {}", skill.name), output).with_metadata(json!({
                "name": skill.name,
                "description": skill.description,
                "location": skill.location.display().to_string(),
                "base_dir": base_dir
            })),
        )
    }
}

/// Create a dynamic description including available skills.
pub fn skill_description_with_available(registry: &SkillRegistry) -> String {
    let available = registry.format_available_skills();

    format!(
        r#"Load a skill to get detailed instructions for a specific task.

Skills provide specialized knowledge and step-by-step guidance for complex tasks.
Use this when a task matches an available skill's description.

{}
"#,
        available
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter() {
        let content = r#"---
name: code-review
description: Detailed code review with best practices
---

# Code Review Skill

This skill provides step-by-step instructions...
"#;

        let (fm, body) = parse_frontmatter(content).unwrap();
        assert_eq!(fm.name, "code-review");
        assert_eq!(fm.description, "Detailed code review with best practices");
        assert!(body.contains("# Code Review Skill"));
    }

    #[test]
    fn test_parse_frontmatter_missing_delimiter() {
        let content = "# No frontmatter here";
        let result = parse_frontmatter(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_format_available_skills() {
        let mut registry = SkillRegistry::new();
        registry.skills.insert(
            "test".to_string(),
            Skill {
                name: "test".to_string(),
                description: "A test skill".to_string(),
                location: PathBuf::from("/test/SKILL.md"),
                content: None,
            },
        );

        let output = registry.format_available_skills();
        assert!(output.contains("<available_skills>"));
        assert!(output.contains("<name>test</name>"));
        assert!(output.contains("<description>A test skill</description>"));
    }

    #[test]
    fn test_skill_description_with_available() {
        let registry = SkillRegistry::new();
        let desc = skill_description_with_available(&registry);
        assert!(desc.contains("No skills are currently available"));
    }
}
