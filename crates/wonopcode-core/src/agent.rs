//! Agent system for wonopcode.
//!
//! Agents define specialized AI personas with different capabilities:
//! - `build` - Default agent for coding tasks (full access)
//! - `plan` - Planning agent with restricted write permissions
//! - `explore` - Fast exploration agent (read-only)
//! - `general` - General-purpose subagent
//! - `compaction` - Internal agent for session summarization
//! - `title` - Internal agent for generating session titles
//! - `summary` - Internal agent for generating summaries

use crate::config::{
    AgentConfig, AgentMode as ConfigAgentMode, AgentSandboxConfig, Config, Permission,
    PermissionOrMap,
};
use std::collections::HashMap;
use std::path::Path;

/// Agent information.
#[derive(Debug, Clone)]
pub struct Agent {
    /// Agent identifier.
    pub name: String,

    /// Human-readable description.
    pub description: Option<String>,

    /// Agent mode (primary, subagent, or both).
    pub mode: AgentMode,

    /// Whether this is a built-in (native) agent.
    pub native: bool,

    /// Whether the agent is hidden from UI selection.
    pub hidden: bool,

    /// Whether this is the default agent.
    pub is_default: bool,

    /// Temperature for generation.
    pub temperature: Option<f32>,

    /// Top-p sampling.
    pub top_p: Option<f32>,

    /// Display color (hex).
    pub color: Option<String>,

    /// Permission settings.
    pub permission: AgentPermission,

    /// Model override (provider/model format).
    pub model: Option<String>,

    /// Custom system prompt addition.
    pub prompt: Option<String>,

    /// Tool enable/disable map.
    pub tools: HashMap<String, bool>,

    /// Maximum steps per turn.
    pub max_steps: Option<u32>,

    /// Per-agent sandbox configuration overrides.
    pub sandbox: Option<AgentSandboxConfig>,
}

/// Agent operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentMode {
    /// Only available as a subagent (spawned by Task tool).
    Subagent,
    /// Primary agent (user-selectable).
    #[default]
    Primary,
    /// Available as both primary and subagent.
    All,
}

impl AgentMode {
    /// Parse from string.
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "subagent" => AgentMode::Subagent,
            "primary" => AgentMode::Primary,
            "all" => AgentMode::All,
            _ => AgentMode::Primary,
        }
    }

    /// Check if agent can be selected as primary.
    pub fn is_primary(&self) -> bool {
        matches!(self, AgentMode::Primary | AgentMode::All)
    }

    /// Check if agent can be used as subagent.
    pub fn is_subagent(&self) -> bool {
        matches!(self, AgentMode::Subagent | AgentMode::All)
    }
}

/// Agent permission configuration.
#[derive(Debug, Clone)]
pub struct AgentPermission {
    /// Edit file permission.
    pub edit: Permission,

    /// Bash command permissions (pattern -> permission).
    pub bash: HashMap<String, Permission>,

    /// Skill permissions (pattern -> permission).
    pub skill: HashMap<String, Permission>,

    /// Web fetch permission.
    pub webfetch: Permission,

    /// Doom loop prevention permission.
    pub doom_loop: Option<Permission>,

    /// External directory access permission.
    pub external_directory: Option<Permission>,
}

impl Default for AgentPermission {
    fn default() -> Self {
        let mut bash = HashMap::new();
        bash.insert("*".to_string(), Permission::Allow);

        let mut skill = HashMap::new();
        skill.insert("*".to_string(), Permission::Allow);

        Self {
            edit: Permission::Allow,
            bash,
            skill,
            webfetch: Permission::Allow,
            doom_loop: Some(Permission::Ask),
            external_directory: Some(Permission::Ask),
        }
    }
}

/// Agent registry - manages all available agents.
#[derive(Debug, Clone)]
pub struct AgentRegistry {
    agents: HashMap<String, Agent>,
    default_agent: String,
}

impl AgentRegistry {
    /// Create a new agent registry from configuration.
    pub fn new(config: &Config) -> Self {
        let mut agents = HashMap::new();
        let default_tools = config.tools.clone().unwrap_or_default();
        let default_permission = Self::build_default_permission(config);

        // Build agent - default coding agent
        agents.insert(
            "build".to_string(),
            Agent {
                name: "build".to_string(),
                description: Some("Default agent for coding tasks with full access".to_string()),
                mode: AgentMode::Primary,
                native: true,
                hidden: false,
                is_default: false,
                temperature: None,
                top_p: None,
                color: None,
                permission: default_permission.clone(),
                model: None,
                prompt: None,
                tools: default_tools.clone(),
                max_steps: None,
                sandbox: None,
            },
        );

        // Plan agent - restricted permissions for planning
        let plan_permission = Self::build_plan_permission(config);
        agents.insert(
            "plan".to_string(),
            Agent {
                name: "plan".to_string(),
                description: Some("Planning agent with read-only file access".to_string()),
                mode: AgentMode::Primary,
                native: true,
                hidden: false,
                is_default: false,
                temperature: None,
                top_p: None,
                color: None,
                permission: plan_permission,
                model: None,
                prompt: None,
                tools: default_tools.clone(),
                max_steps: None,
                sandbox: None,
            },
        );

        // General subagent
        let mut general_tools = default_tools.clone();
        general_tools.insert("todoread".to_string(), false);
        general_tools.insert("todowrite".to_string(), false);
        agents.insert(
            "general".to_string(),
            Agent {
                name: "general".to_string(),
                description: Some(
                    "General-purpose agent for researching complex questions and executing multi-step tasks."
                        .to_string(),
                ),
                mode: AgentMode::Subagent,
                native: true,
                hidden: true,
                is_default: false,
                temperature: None,
                top_p: None,
                color: None,
                permission: default_permission.clone(),
                model: None,
                prompt: None,
                tools: general_tools,
                max_steps: None,
                sandbox: None,
            },
        );

        // Explore subagent - with read-only sandbox
        let mut explore_tools = default_tools.clone();
        explore_tools.insert("todoread".to_string(), false);
        explore_tools.insert("todowrite".to_string(), false);
        explore_tools.insert("edit".to_string(), false);
        explore_tools.insert("write".to_string(), false);
        agents.insert(
            "explore".to_string(),
            Agent {
                name: "explore".to_string(),
                description: Some(
                    "Fast agent specialized for exploring codebases. Use for finding files, searching code, or answering questions about the codebase."
                        .to_string(),
                ),
                mode: AgentMode::Subagent,
                native: true,
                hidden: false,
                is_default: false,
                temperature: None,
                top_p: None,
                color: None,
                permission: default_permission.clone(),
                model: None,
                prompt: Some(EXPLORE_PROMPT.to_string()),
                tools: explore_tools,
                max_steps: None,
                // Explore agent uses read-only sandbox workspace
                sandbox: Some(AgentSandboxConfig {
                    enabled: None,
                    workspace_writable: Some(false),
                    network: None,
                    bypass_tools: None,
                    resources: None,
                }),
            },
        );

        // Compaction agent (internal)
        let mut no_tools = HashMap::new();
        no_tools.insert("*".to_string(), false);
        agents.insert(
            "compaction".to_string(),
            Agent {
                name: "compaction".to_string(),
                description: None,
                mode: AgentMode::Primary,
                native: true,
                hidden: true,
                is_default: false,
                temperature: None,
                top_p: None,
                color: None,
                permission: default_permission.clone(),
                model: None,
                prompt: Some(COMPACTION_PROMPT.to_string()),
                tools: no_tools.clone(),
                max_steps: None,
                sandbox: None,
            },
        );

        // Title agent (internal)
        agents.insert(
            "title".to_string(),
            Agent {
                name: "title".to_string(),
                description: None,
                mode: AgentMode::Primary,
                native: true,
                hidden: true,
                is_default: false,
                temperature: None,
                top_p: None,
                color: None,
                permission: default_permission.clone(),
                model: None,
                prompt: Some(TITLE_PROMPT.to_string()),
                tools: HashMap::new(),
                max_steps: None,
                sandbox: None,
            },
        );

        // Summary agent (internal)
        agents.insert(
            "summary".to_string(),
            Agent {
                name: "summary".to_string(),
                description: None,
                mode: AgentMode::Primary,
                native: true,
                hidden: true,
                is_default: false,
                temperature: None,
                top_p: None,
                color: None,
                permission: default_permission.clone(),
                model: None,
                prompt: Some(SUMMARY_PROMPT.to_string()),
                tools: HashMap::new(),
                max_steps: None,
                sandbox: None,
            },
        );

        // Apply config overrides
        if let Some(agent_configs) = &config.agent {
            for (name, agent_config) in agent_configs {
                if agent_config.disable.unwrap_or(false) {
                    agents.remove(name);
                    continue;
                }

                let agent = agents.entry(name.clone()).or_insert_with(|| Agent {
                    name: name.clone(),
                    description: None,
                    mode: AgentMode::All,
                    native: false,
                    hidden: false,
                    is_default: false,
                    temperature: None,
                    top_p: None,
                    color: None,
                    permission: default_permission.clone(),
                    model: None,
                    prompt: None,
                    tools: default_tools.clone(),
                    max_steps: None,
                    sandbox: None,
                });

                Self::apply_config_to_agent(agent, agent_config, &default_tools);
            }
        }

        // Determine default agent
        let default_name = config
            .default_agent
            .as_deref()
            .unwrap_or("build")
            .to_string();

        // Mark the default agent
        if let Some(agent) = agents.get_mut(&default_name) {
            if agent.mode.is_primary() && !agent.hidden {
                agent.is_default = true;
            }
        }

        // Fallback to build if configured default is invalid
        if !agents.values().any(|a| a.is_default) {
            if let Some(agent) = agents.get_mut("build") {
                agent.is_default = true;
            }
        }

        let default_agent = agents
            .values()
            .find(|a| a.is_default)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "build".to_string());

        Self {
            agents,
            default_agent,
        }
    }

    /// Build default permission from config.
    fn build_default_permission(config: &Config) -> AgentPermission {
        let mut permission = AgentPermission::default();

        if let Some(perm_config) = &config.permission {
            if let Some(edit) = perm_config.edit {
                permission.edit = edit;
            }
            if let Some(bash) = &perm_config.bash {
                permission.bash = Self::permission_or_map_to_hashmap(bash);
            }
            if let Some(webfetch) = perm_config.webfetch {
                permission.webfetch = webfetch;
            }
            if let Some(ext_dir) = perm_config.external_directory {
                permission.external_directory = Some(ext_dir);
            }
        }

        permission
    }

    /// Build plan agent permission (restricted).
    fn build_plan_permission(config: &Config) -> AgentPermission {
        let mut bash = HashMap::new();

        // Read-only commands allowed
        for pattern in &[
            "cut*",
            "diff*",
            "du*",
            "file *",
            "find *",
            "git diff*",
            "git log*",
            "git show*",
            "git status*",
            "git branch",
            "git branch -v",
            "grep*",
            "head*",
            "less*",
            "ls*",
            "more*",
            "pwd*",
            "rg*",
            "sort*",
            "stat*",
            "tail*",
            "tree*",
            "uniq*",
            "wc*",
            "whereis*",
            "which*",
        ] {
            bash.insert((*pattern).to_string(), Permission::Allow);
        }

        // Dangerous commands need confirmation
        for pattern in &[
            "find * -delete*",
            "find * -exec*",
            "find * -fprint*",
            "find * -fls*",
            "find * -fprintf*",
            "find * -ok*",
            "sort --output=*",
            "sort -o *",
            "tree -o *",
        ] {
            bash.insert((*pattern).to_string(), Permission::Ask);
        }

        // Default to ask for unknown commands
        bash.insert("*".to_string(), Permission::Ask);

        let mut permission = AgentPermission {
            edit: Permission::Deny,
            bash,
            skill: {
                let mut s = HashMap::new();
                s.insert("*".to_string(), Permission::Allow);
                s
            },
            webfetch: Permission::Allow,
            doom_loop: Some(Permission::Ask),
            external_directory: Some(Permission::Ask),
        };

        // Apply config overrides
        if let Some(perm_config) = &config.permission {
            if let Some(webfetch) = perm_config.webfetch {
                permission.webfetch = webfetch;
            }
        }

        permission
    }

    /// Convert PermissionOrMap to HashMap.
    fn permission_or_map_to_hashmap(pom: &PermissionOrMap) -> HashMap<String, Permission> {
        match pom {
            PermissionOrMap::Single(p) => {
                let mut map = HashMap::new();
                map.insert("*".to_string(), *p);
                map
            }
            PermissionOrMap::Map(m) => m.clone(),
        }
    }

    /// Apply config overrides to an agent.
    fn apply_config_to_agent(
        agent: &mut Agent,
        config: &AgentConfig,
        default_tools: &HashMap<String, bool>,
    ) {
        if let Some(model) = &config.model {
            agent.model = Some(model.clone());
        }
        if let Some(temp) = config.temperature {
            agent.temperature = Some(temp);
        }
        if let Some(top_p) = config.top_p {
            agent.top_p = Some(top_p);
        }
        if let Some(prompt) = &config.prompt {
            agent.prompt = Some(prompt.clone());
        }
        if let Some(desc) = &config.description {
            agent.description = Some(desc.clone());
        }
        if let Some(mode) = config.mode {
            agent.mode = match mode {
                ConfigAgentMode::Subagent => AgentMode::Subagent,
                ConfigAgentMode::Primary => AgentMode::Primary,
                ConfigAgentMode::All => AgentMode::All,
            };
        }
        if let Some(color) = &config.color {
            agent.color = Some(color.clone());
        }
        if let Some(max_steps) = config.max_steps {
            agent.max_steps = Some(max_steps);
        }

        // Merge tools
        if let Some(tools) = &config.tools {
            for (name, enabled) in tools {
                agent.tools.insert(name.clone(), *enabled);
            }
        }

        // Merge with default tools
        for (name, enabled) in default_tools {
            agent.tools.entry(name.clone()).or_insert(*enabled);
        }

        // Apply permission overrides
        if let Some(perm) = &config.permission {
            if let Some(edit) = perm.edit {
                agent.permission.edit = edit;
            }
            if let Some(bash) = &perm.bash {
                agent.permission.bash = Self::permission_or_map_to_hashmap(bash);
            }
            if let Some(skill) = &perm.skill {
                agent.permission.skill = Self::permission_or_map_to_hashmap(skill);
            }
            if let Some(webfetch) = perm.webfetch {
                agent.permission.webfetch = webfetch;
            }
            if let Some(doom_loop) = perm.doom_loop {
                agent.permission.doom_loop = Some(doom_loop);
            }
            if let Some(ext_dir) = perm.external_directory {
                agent.permission.external_directory = Some(ext_dir);
            }
        }

        // Apply sandbox overrides
        if let Some(sandbox_config) = &config.sandbox {
            // Merge with existing sandbox config or create new
            let existing = agent.sandbox.take().unwrap_or_default();
            agent.sandbox = Some(AgentSandboxConfig {
                enabled: sandbox_config.enabled.or(existing.enabled),
                workspace_writable: sandbox_config
                    .workspace_writable
                    .or(existing.workspace_writable),
                network: sandbox_config.network.clone().or(existing.network),
                bypass_tools: sandbox_config
                    .bypass_tools
                    .clone()
                    .or(existing.bypass_tools),
                resources: sandbox_config.resources.clone().or(existing.resources),
            });
        }
    }

    /// Get an agent by name.
    pub fn get(&self, name: &str) -> Option<&Agent> {
        self.agents.get(name)
    }

    /// Get all agents.
    pub fn all(&self) -> impl Iterator<Item = &Agent> {
        self.agents.values()
    }

    /// Get primary agents (user-selectable, non-hidden).
    pub fn primary_agents(&self) -> Vec<&Agent> {
        self.agents
            .values()
            .filter(|a| a.mode.is_primary() && !a.hidden)
            .collect()
    }

    /// Get subagents.
    pub fn subagents(&self) -> Vec<&Agent> {
        self.agents
            .values()
            .filter(|a| a.mode.is_subagent())
            .collect()
    }

    /// Get the default agent name.
    pub fn default_agent(&self) -> &str {
        &self.default_agent
    }

    /// Get the default agent.
    pub fn get_default(&self) -> Option<&Agent> {
        self.agents.get(&self.default_agent)
    }

    /// Check if a tool is enabled for an agent.
    pub fn is_tool_enabled(&self, agent: &str, tool: &str) -> bool {
        if let Some(agent) = self.agents.get(agent) {
            // Check specific tool setting
            if let Some(&enabled) = agent.tools.get(tool) {
                return enabled;
            }
            // Check wildcard
            if let Some(&enabled) = agent.tools.get("*") {
                return enabled;
            }
            // Default to enabled
            true
        } else {
            true
        }
    }

    /// Load custom agents from a directory.
    ///
    /// Looks for `.md` files in the directory and parses them as agent definitions.
    pub async fn load_custom_agents(&mut self, dir: &Path) -> std::io::Result<()> {
        if !dir.exists() {
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                if let Ok(agent) = Self::parse_agent_file(&path).await {
                    self.agents.insert(agent.name.clone(), agent);
                }
            }
        }

        Ok(())
    }

    /// Parse an agent definition from a markdown file.
    async fn parse_agent_file(path: &Path) -> std::io::Result<Agent> {
        let content = tokio::fs::read_to_string(path).await?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("custom")
            .to_string();

        // Simple parsing: use file content as prompt
        // In a full implementation, we'd parse frontmatter for settings
        Ok(Agent {
            name,
            description: None,
            mode: AgentMode::All,
            native: false,
            hidden: false,
            is_default: false,
            temperature: None,
            top_p: None,
            color: None,
            permission: AgentPermission::default(),
            model: None,
            prompt: Some(content),
            tools: HashMap::new(),
            max_steps: None,
            sandbox: None,
        })
    }
}

// Built-in agent prompts

const EXPLORE_PROMPT: &str = r#"You are a file search specialist. You excel at thoroughly navigating and exploring codebases.

Your strengths:
- Rapidly finding files using glob patterns
- Searching code and text with powerful regex patterns
- Reading and analyzing file contents

Guidelines:
- Use Glob for broad file pattern matching
- Use Grep for searching file contents with regex
- Use Read when you know the specific file path you need to read
- Use Bash for file operations like copying, moving, or listing directory contents
- Adapt your search approach based on the thoroughness level specified by the caller
- Return file paths as absolute paths in your final response
- For clear communication, avoid using emojis
- Do not create any files, or run bash commands that modify the user's system state in any way

Complete the user's search request efficiently and report your findings clearly."#;

const COMPACTION_PROMPT: &str = r#"You are a conversation summarizer. Your task is to create a concise summary of the conversation that preserves all important context.

Guidelines:
- Preserve key decisions and their rationale
- Keep track of file changes and their purposes
- Note any unresolved issues or next steps
- Be concise but comprehensive
- Format as clear bullet points

Output only the summary, no preamble."#;

const TITLE_PROMPT: &str = r#"You are a title generator. Generate a short, descriptive title for the conversation.

Guidelines:
- Keep it under 50 characters
- Be specific about the task or topic
- Use action verbs when appropriate
- No quotes or special formatting

Output only the title, nothing else."#;

const SUMMARY_PROMPT: &str = r#"You are a session summarizer. Create a brief summary of what was accomplished in this session.

Guidelines:
- List key accomplishments
- Note any remaining tasks
- Be concise (2-3 sentences max)

Output only the summary, no preamble."#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_agents() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        assert!(registry.get("build").is_some());
        assert!(registry.get("plan").is_some());
        assert!(registry.get("explore").is_some());
        assert!(registry.get("general").is_some());
        assert_eq!(registry.default_agent(), "build");
    }

    #[test]
    fn test_primary_agents() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        let primary = registry.primary_agents();
        assert!(primary.iter().any(|a| a.name == "build"));
        assert!(primary.iter().any(|a| a.name == "plan"));
        // explore and general are subagents, but explore is not hidden
        assert!(primary.iter().all(|a| !a.hidden));
    }

    #[test]
    fn test_plan_agent_permissions() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        let plan = registry.get("plan").unwrap();
        assert_eq!(plan.permission.edit, Permission::Deny);
        assert_eq!(plan.permission.bash.get("ls*"), Some(&Permission::Allow));
        assert_eq!(plan.permission.bash.get("*"), Some(&Permission::Ask));
    }

    #[test]
    fn test_tool_enabled() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        // explore agent has edit disabled
        assert!(!registry.is_tool_enabled("explore", "edit"));
        assert!(!registry.is_tool_enabled("explore", "write"));

        // build agent has everything enabled
        assert!(registry.is_tool_enabled("build", "edit"));
        assert!(registry.is_tool_enabled("build", "write"));
    }

    #[test]
    fn test_agent_mode() {
        assert!(AgentMode::Primary.is_primary());
        assert!(!AgentMode::Primary.is_subagent());

        assert!(!AgentMode::Subagent.is_primary());
        assert!(AgentMode::Subagent.is_subagent());

        assert!(AgentMode::All.is_primary());
        assert!(AgentMode::All.is_subagent());
    }

    #[test]
    fn test_agent_mode_parse() {
        assert_eq!(AgentMode::parse("subagent"), AgentMode::Subagent);
        assert_eq!(AgentMode::parse("primary"), AgentMode::Primary);
        assert_eq!(AgentMode::parse("all"), AgentMode::All);
        assert_eq!(AgentMode::parse("SUBAGENT"), AgentMode::Subagent);
        assert_eq!(AgentMode::parse("PRIMARY"), AgentMode::Primary);
        assert_eq!(AgentMode::parse("ALL"), AgentMode::All);
        assert_eq!(AgentMode::parse("unknown"), AgentMode::Primary); // default
    }

    #[test]
    fn test_agent_mode_default() {
        let mode: AgentMode = Default::default();
        assert_eq!(mode, AgentMode::Primary);
    }

    #[test]
    fn test_agent_permission_default() {
        let perm = AgentPermission::default();
        assert_eq!(perm.edit, Permission::Allow);
        assert_eq!(perm.bash.get("*"), Some(&Permission::Allow));
        assert_eq!(perm.skill.get("*"), Some(&Permission::Allow));
        assert_eq!(perm.webfetch, Permission::Allow);
        assert_eq!(perm.doom_loop, Some(Permission::Ask));
        assert_eq!(perm.external_directory, Some(Permission::Ask));
    }

    #[test]
    fn test_agent_registry_all() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        let agents: Vec<_> = registry.all().collect();
        assert!(!agents.is_empty());
        assert!(agents.iter().any(|a| a.name == "build"));
    }

    #[test]
    fn test_agent_registry_get_default() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        let default = registry.get_default();
        assert!(default.is_some());
        assert_eq!(default.unwrap().name, "build");
    }

    #[test]
    fn test_subagents() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        let subagents = registry.subagents();
        assert!(subagents.iter().any(|a| a.name == "explore"));
        assert!(subagents.iter().any(|a| a.name == "general"));
    }

    #[test]
    fn test_is_tool_enabled_unknown_agent() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        // Unknown agent defaults to enabled
        assert!(registry.is_tool_enabled("nonexistent", "bash"));
    }

    #[test]
    fn test_is_tool_enabled_wildcard() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        // compaction has "*" -> false
        assert!(!registry.is_tool_enabled("compaction", "bash"));
        assert!(!registry.is_tool_enabled("compaction", "edit"));
    }

    #[test]
    fn test_hidden_agents() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        let compaction = registry.get("compaction").unwrap();
        assert!(compaction.hidden);
        assert!(compaction.native);

        let title = registry.get("title").unwrap();
        assert!(title.hidden);

        let summary = registry.get("summary").unwrap();
        assert!(summary.hidden);

        let general = registry.get("general").unwrap();
        assert!(general.hidden);
    }

    #[test]
    fn test_explore_agent_readonly() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        let explore = registry.get("explore").unwrap();
        assert!(explore.sandbox.is_some());
        assert_eq!(
            explore.sandbox.as_ref().unwrap().workspace_writable,
            Some(false)
        );
    }

    #[test]
    fn test_agent_clone() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        let build = registry.get("build").unwrap();
        let cloned = build.clone();
        assert_eq!(cloned.name, build.name);
        assert_eq!(cloned.is_default, build.is_default);
    }

    #[test]
    fn test_agent_debug() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);

        let build = registry.get("build").unwrap();
        let debug_str = format!("{:?}", build);
        assert!(debug_str.contains("build"));
    }

    #[test]
    fn test_agent_permission_clone() {
        let perm = AgentPermission::default();
        let cloned = perm.clone();
        assert_eq!(cloned.edit, perm.edit);
        assert_eq!(cloned.webfetch, perm.webfetch);
    }

    #[test]
    fn test_registry_clone() {
        let config = Config::default();
        let registry = AgentRegistry::new(&config);
        let cloned = registry.clone();

        assert_eq!(cloned.default_agent(), registry.default_agent());
        assert!(cloned.get("build").is_some());
    }

    #[test]
    fn test_agent_with_custom_config() {
        use crate::config::{AgentConfig, AgentMode as ConfigAgentMode, AgentPermissionConfig};
        use std::collections::HashMap;

        let mut config = Config::default();
        let mut agents = HashMap::new();

        // Configure a custom agent
        agents.insert(
            "custom".to_string(),
            AgentConfig {
                model: Some("custom-model".to_string()),
                temperature: Some(0.5),
                top_p: Some(0.9),
                prompt: Some("Custom prompt".to_string()),
                description: Some("Custom description".to_string()),
                mode: Some(ConfigAgentMode::All),
                color: Some("#FF0000".to_string()),
                max_steps: Some(10),
                tools: Some({
                    let mut t = HashMap::new();
                    t.insert("bash".to_string(), false);
                    t
                }),
                permission: Some(AgentPermissionConfig {
                    edit: Some(Permission::Deny),
                    bash: None,
                    skill: None,
                    webfetch: Some(Permission::Deny),
                    doom_loop: Some(Permission::Deny),
                    external_directory: Some(Permission::Deny),
                }),
                sandbox: None,
                disable: None,
            },
        );

        config.agent = Some(agents);
        let registry = AgentRegistry::new(&config);

        let custom = registry.get("custom").unwrap();
        assert_eq!(custom.model, Some("custom-model".to_string()));
        assert_eq!(custom.temperature, Some(0.5));
        assert_eq!(custom.top_p, Some(0.9));
        assert_eq!(custom.description, Some("Custom description".to_string()));
        assert_eq!(custom.color, Some("#FF0000".to_string()));
        assert_eq!(custom.max_steps, Some(10));
        assert_eq!(custom.permission.edit, Permission::Deny);
        assert_eq!(custom.permission.webfetch, Permission::Deny);
    }

    #[test]
    fn test_disable_agent() {
        use crate::config::AgentConfig;
        use std::collections::HashMap;

        let mut config = Config::default();
        let mut agents = HashMap::new();

        agents.insert(
            "build".to_string(),
            AgentConfig {
                disable: Some(true),
                ..Default::default()
            },
        );

        config.agent = Some(agents);
        let registry = AgentRegistry::new(&config);

        // build should be removed
        assert!(registry.get("build").is_none());
    }

    #[test]
    fn test_custom_default_agent() {
        let config = Config {
            default_agent: Some("plan".to_string()),
            ..Default::default()
        };

        let registry = AgentRegistry::new(&config);
        assert_eq!(registry.default_agent(), "plan");

        let plan = registry.get("plan").unwrap();
        assert!(plan.is_default);
    }

    #[test]
    fn test_invalid_default_agent_fallback() {
        let config = Config {
            default_agent: Some("nonexistent".to_string()),
            ..Default::default()
        };

        let registry = AgentRegistry::new(&config);
        // Should fallback to build
        assert_eq!(registry.default_agent(), "build");
    }

    #[tokio::test]
    async fn test_load_custom_agents_nonexistent_dir() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let nonexistent = dir.path().join("nonexistent");

        let config = Config::default();
        let mut registry = AgentRegistry::new(&config);

        // Should not fail for nonexistent directory
        let result = registry.load_custom_agents(&nonexistent).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_custom_agents() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();

        // Create a custom agent file
        let agent_file = dir.path().join("my-agent.md");
        std::fs::write(
            &agent_file,
            "# Custom Agent\n\nThis is a custom agent prompt.",
        )
        .unwrap();

        let config = Config::default();
        let mut registry = AgentRegistry::new(&config);
        registry.load_custom_agents(dir.path()).await.unwrap();

        let custom = registry.get("my-agent");
        assert!(custom.is_some());
        let custom = custom.unwrap();
        assert!(!custom.native);
        assert_eq!(custom.mode, AgentMode::All);
        assert!(custom.prompt.as_ref().unwrap().contains("Custom Agent"));
    }

    #[tokio::test]
    async fn test_load_custom_agents_ignores_non_md() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();

        // Create non-md file
        std::fs::write(dir.path().join("not-an-agent.txt"), "ignored").unwrap();

        let config = Config::default();
        let mut registry = AgentRegistry::new(&config);
        let initial_count = registry.all().count();

        registry.load_custom_agents(dir.path()).await.unwrap();

        // Should not add new agent
        assert_eq!(registry.all().count(), initial_count);
    }

    #[test]
    fn test_permission_or_map_to_hashmap_single() {
        let pom = PermissionOrMap::Single(Permission::Deny);
        let map = AgentRegistry::permission_or_map_to_hashmap(&pom);
        assert_eq!(map.get("*"), Some(&Permission::Deny));
    }

    #[test]
    fn test_permission_or_map_to_hashmap_map() {
        let mut m = HashMap::new();
        m.insert("ls*".to_string(), Permission::Allow);
        m.insert("rm*".to_string(), Permission::Deny);
        let pom = PermissionOrMap::Map(m);

        let map = AgentRegistry::permission_or_map_to_hashmap(&pom);
        assert_eq!(map.get("ls*"), Some(&Permission::Allow));
        assert_eq!(map.get("rm*"), Some(&Permission::Deny));
    }

    #[test]
    fn test_build_default_permission_with_config() {
        use crate::config::PermissionConfig;

        let config = Config {
            permission: Some(PermissionConfig {
                edit: Some(Permission::Deny),
                bash: Some(PermissionOrMap::Single(Permission::Ask)),
                webfetch: Some(Permission::Deny),
                external_directory: Some(Permission::Deny),
                allow_all_in_sandbox: None,
            }),
            ..Default::default()
        };

        let perm = AgentRegistry::build_default_permission(&config);
        assert_eq!(perm.edit, Permission::Deny);
        assert_eq!(perm.bash.get("*"), Some(&Permission::Ask));
        assert_eq!(perm.webfetch, Permission::Deny);
        assert_eq!(perm.external_directory, Some(Permission::Deny));
    }

    #[test]
    fn test_sandbox_config_merge() {
        use crate::config::{AgentConfig, AgentSandboxConfig as ConfigSandbox};
        use std::collections::HashMap;

        let mut config = Config::default();
        let mut agents = HashMap::new();

        // Configure explore with sandbox override
        agents.insert(
            "explore".to_string(),
            AgentConfig {
                sandbox: Some(ConfigSandbox {
                    enabled: Some(false),
                    workspace_writable: None, // Keep existing
                    network: Some("none".to_string()),
                    bypass_tools: None,
                    resources: None,
                }),
                ..Default::default()
            },
        );

        config.agent = Some(agents);
        let registry = AgentRegistry::new(&config);

        let explore = registry.get("explore").unwrap();
        assert!(explore.sandbox.is_some());
        let sandbox = explore.sandbox.as_ref().unwrap();
        assert_eq!(sandbox.enabled, Some(false));
        assert_eq!(sandbox.workspace_writable, Some(false)); // preserved from original
        assert_eq!(sandbox.network, Some("none".to_string()));
    }
}
