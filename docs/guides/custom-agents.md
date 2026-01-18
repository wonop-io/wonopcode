# Custom Agents

Create specialized AI personalities with custom system prompts, tool access, and permissions.

---

## What Are Agents?

Agents are AI configurations that define:

- **System prompt**: The AI's personality and instructions
- **Model**: Which AI model to use
- **Tools**: Which tools the agent can access
- **Permissions**: What operations are allowed
- **Sandbox settings**: Isolation configuration

---

## Built-in Agents

Wonopcode includes several built-in agents:

### Primary Agents

| Agent | Purpose |
|-------|---------|
| `build` | Default agent for coding tasks with full access |
| `plan` | Planning agent with read-only file access |

### Subagents

These are spawned by the Task tool for focused work:

| Agent | Purpose |
|-------|---------|
| `explore` | Fast agent for codebase exploration (read-only) |
| `general` | General-purpose agent for multi-step tasks |

### Internal Agents

These are used internally and hidden from selection:

| Agent | Purpose |
|-------|---------|
| `compaction` | Summarizes conversations for context management |
| `title` | Generates session titles |
| `summary` | Generates session summaries |

Switch agents with:

```
/agent explore
```

---

## Creating Custom Agents

### Agent File Format

Create agents in `.wonopcode/agent/` as markdown files:

```markdown
<!-- .wonopcode/agent/reviewer.md -->
---
name: reviewer
description: Code review specialist
model: anthropic/claude-sonnet-4-5-20250929
---

You are an expert code reviewer. When reviewing code:

1. Look for bugs and potential issues
2. Check for security vulnerabilities
3. Evaluate code style and readability
4. Suggest performance improvements
5. Verify error handling

Be thorough but constructive. Always explain why something is an issue
and provide specific suggestions for improvement.

Focus on:
- Logic errors
- Edge cases
- Input validation
- Resource management
- Thread safety (if applicable)
```

### Frontmatter Options

```yaml
---
name: agent-name              # Required: unique identifier
description: Short description # Optional: shown in agent list
model: provider/model-name    # Optional: override default model
small_model: provider/model   # Optional: for lightweight tasks
tools:                        # Optional: tool access control
  read: true
  write: false
  bash: false
permission:                   # Optional: permission overrides
  bash:
    "git *": allow
sandbox:                      # Optional: sandbox settings
  enabled: true
  network: limited
---
```

---

## Tool Access Control

### Enable/Disable Tools

```yaml
---
name: readonly
tools:
  "*": false          # Disable all tools by default
  read: true          # Enable specific tools
  glob: true
  grep: true
  list: true
---
```

### Tool Wildcards

```yaml
---
name: full-access
tools:
  "*": true           # Enable all tools
---
```

```yaml
---
name: no-write
tools:
  "*": true
  write: false        # Disable specific tools
  edit: false
  bash: false
---
```

---

## Permission Configuration

### Bash Permissions

Control which shell commands are allowed:

```yaml
---
name: git-only
permission:
  bash:
    "*": deny         # Deny all by default
    "git *": allow    # Allow git commands
    "ls": allow
    "cat": allow
---
```

### Permission Values

| Value | Behavior |
|-------|----------|
| `allow` | Execute without asking |
| `ask` | Prompt user for approval |
| `deny` | Block execution |

### Pattern Matching

| Pattern | Matches |
|---------|---------|
| `command` | Exact command |
| `command *` | Command with any arguments |
| `*pattern*` | Contains pattern |

---

## Sandbox Configuration

### Per-Agent Sandbox

```yaml
---
name: safe-explorer
sandbox:
  enabled: true
  mounts:
    workspace_writable: false   # Read-only access
  network: none                 # No network
---
```

### Full Network for Build Agent

```yaml
---
name: builder
sandbox:
  enabled: true
  network: full                 # Allow package downloads
  resources:
    memory: 4G
    cpus: 4
---
```

### Disable Sandbox for Trusted Agent

```yaml
---
name: trusted
sandbox:
  enabled: false               # Run directly on host
---
```

---

## Example Agents

### Documentation Writer

```markdown
<!-- .wonopcode/agent/docs.md -->
---
name: docs
description: Documentation specialist
model: anthropic/claude-sonnet-4-5-20250929
tools:
  read: true
  write: true
  glob: true
  grep: true
  edit: true
  bash: false
---

You are a technical documentation expert. Your role is to:

1. Write clear, concise documentation
2. Add docstrings and comments to code
3. Create README files and guides
4. Document APIs and interfaces
5. Write examples and tutorials

Guidelines:
- Use simple, direct language
- Include code examples
- Structure content with clear headings
- Explain the "why" not just the "what"
- Keep documentation up-to-date with code

Do NOT execute commands or make code logic changes. Focus only on
documentation improvements.
```

### Security Auditor

```markdown
<!-- .wonopcode/agent/security.md -->
---
name: security
description: Security vulnerability scanner
tools:
  read: true
  glob: true
  grep: true
  write: false
  edit: false
  bash: false
sandbox:
  enabled: true
  mounts:
    workspace_writable: false
---

You are a security auditor. Analyze code for vulnerabilities:

## Check For
- SQL injection
- XSS (Cross-Site Scripting)
- CSRF (Cross-Site Request Forgery)
- Authentication bypasses
- Authorization flaws
- Sensitive data exposure
- Insecure dependencies
- Hardcoded secrets
- Path traversal
- Command injection

## Output Format
For each issue found:
1. **Severity**: Critical/High/Medium/Low
2. **Location**: File and line number
3. **Description**: What the vulnerability is
4. **Impact**: What could happen if exploited
5. **Remediation**: How to fix it

Never modify files. Report findings only.
```

### Test Writer

```markdown
<!-- .wonopcode/agent/tests.md -->
---
name: tests
description: Test generation specialist
model: anthropic/claude-sonnet-4-5-20250929
tools:
  read: true
  write: true
  edit: true
  glob: true
  grep: true
  bash: true
permission:
  bash:
    "*": deny
    "cargo test *": allow
    "npm test *": allow
    "pytest *": allow
sandbox:
  enabled: true
---

You are a test engineering expert. Your role is to:

1. Write comprehensive unit tests
2. Create integration tests
3. Add edge case coverage
4. Improve test reliability
5. Reduce test flakiness

Guidelines:
- Follow existing test patterns in the codebase
- Use descriptive test names
- Test one thing per test
- Include positive and negative cases
- Mock external dependencies
- Aim for high coverage of critical paths

You can run tests to verify your changes work.
```

### Refactoring Expert

```markdown
<!-- .wonopcode/agent/refactor.md -->
---
name: refactor
description: Code refactoring specialist
tools:
  "*": true
permission:
  bash:
    "*": ask
    "git diff *": allow
    "git status": allow
sandbox:
  enabled: true
---

You are a refactoring expert. Improve code quality by:

1. Extracting functions and methods
2. Reducing complexity
3. Improving naming
4. Removing duplication
5. Applying design patterns
6. Improving error handling

Principles:
- Make small, incremental changes
- Ensure tests pass after each change
- Preserve external behavior
- Document significant changes
- Follow existing code style

Before major refactors:
1. Understand the current code
2. Identify what to improve
3. Plan the changes
4. Make changes incrementally
5. Verify with tests
```

---

## Using Custom Agents

### Switch to Agent

```
/agent reviewer
```

### List Available Agents

```
/agent
```

Shows:
```
Available agents:
  build (default) - Default agent for coding tasks with full access
  plan - Planning agent with read-only file access
* reviewer - Code review specialist
  docs - Documentation specialist
  security - Security vulnerability scanner
```

### Agent in Config

Set default agent in `config.json`:

```json
{
  "default_agent": "build"
}
```

### Agent via CLI

```bash
wonopcode --agent reviewer
```

---

## Agent Inheritance

Agents can inherit from built-in agents:

```yaml
---
name: custom-code
extends: code                  # Inherit from code agent
tools:
  webfetch: false             # Override specific settings
---

Additional instructions for this agent...
```

---

## Per-Project Agents

Agents in `.wonopcode/agent/` are project-specific. For global agents, place them in:

```
~/.config/wonopcode/agent/
```

Project agents override global agents with the same name.

---

## Agent Context

Agents have access to:

| Context | Description |
|---------|-------------|
| `{project}` | Project directory name |
| `{cwd}` | Current working directory |
| `{user}` | Username from config |
| `{date}` | Current date |

Use in system prompts:

```markdown
---
name: project-aware
---

You are working on the {project} project in {cwd}.
Today is {date}.
```

---

## Best Practices

### 1. Single Responsibility

Each agent should have one clear purpose:

```markdown
<!-- ✓ Good: focused purpose -->
---
name: reviewer
description: Code review specialist
---

<!-- ✗ Bad: too broad -->
---
name: everything
description: Does everything
---
```

### 2. Minimal Permissions

Only grant necessary access:

```yaml
# ✓ Good: minimal permissions
tools:
  read: true
  glob: true
  write: false
  bash: false

# ✗ Bad: unnecessary permissions
tools:
  "*": true
```

### 3. Clear Instructions

Be specific in system prompts:

```markdown
<!-- ✓ Good: specific instructions -->
When reviewing code:
1. Check for null pointer exceptions
2. Verify error handling
3. Look for SQL injection

<!-- ✗ Bad: vague instructions -->
Review the code and find problems.
```

### 4. Test Your Agents

Try agents with various prompts to ensure they behave correctly.

---

## Troubleshooting

### Agent Not Found

```
Error: Agent 'myagent' not found
```

**Solutions**:
1. Check file is in `.wonopcode/agent/` or `~/.config/wonopcode/agent/`
2. Check filename matches agent name
3. Check frontmatter syntax

### Tool Not Available

```
Error: Tool 'bash' not enabled for agent 'readonly'
```

**Solutions**:
1. Check agent's `tools` configuration
2. Use an agent with required tools
3. Update agent configuration

### Invalid YAML

```
Error: Failed to parse agent frontmatter
```

**Solutions**:
1. Check YAML syntax in frontmatter
2. Ensure `---` delimiters are present
3. Validate with a YAML linter

---

## Next Steps

- [Tools Overview](./tools-overview.md) - Available tools for agents
- [Configuration](../CONFIGURATION.md) - Global agent settings
- [Security Model](../architecture/security-model.md) - Permission system
