# Skills System

Skills are specialized instruction sets that provide the AI with detailed knowledge for specific tasks. They extend wonopcode's capabilities without modifying the core agent behavior.

## Overview

A skill is a Markdown file with YAML frontmatter that contains:
- **Metadata**: Name and description for discovery
- **Instructions**: Detailed guidance for the AI when performing a specific task

When you invoke a skill, its content is loaded into the conversation context, giving the AI specialized knowledge for that task.

## Skill File Format

Skills are defined in `SKILL.md` files with YAML frontmatter:

```markdown
---
name: skill-identifier
description: Brief description of what this skill does
---

# Skill Title

Detailed instructions and guidance for the AI...

## Steps

1. First step...
2. Second step...

## Best Practices

- Tip 1
- Tip 2
```

### Frontmatter Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Unique identifier for the skill (used to invoke it) |
| `description` | Yes | Human-readable description shown when listing skills |

## Skill Discovery

Wonopcode automatically discovers skills from these locations:

1. **Project skills**: `{project}/.wonopcode/skills/**/SKILL.md`
2. **Claude compatibility**: `{project}/.claude/skills/**/SKILL.md`
3. **Global skills**: `~/.config/wonopcode/skill/**/SKILL.md`

Skills can be organized in subdirectories. The discovery process walks the entire tree looking for `SKILL.md` files.

## Using Skills

### Listing Available Skills

The AI can see available skills in its tool parameters. You can also ask:

```
What skills are available?
```

### Invoking a Skill

Skills are invoked through the Skill tool. The AI will automatically use skills when appropriate, or you can explicitly request one:

```
Use the code-review skill to review my changes
```

```
Load the commit skill and help me write a commit message
```

### Skill Permissions

Skills can have permission requirements configured per-agent:

```jsonc
{
  "agent": {
    "build": {
      "permission": {
        "skill": {
          "dangerous-skill": "ask",
          "*": "allow"
        }
      }
    }
  }
}
```

## Creating Skills

### Basic Example

Create a file at `.wonopcode/skills/code-review/SKILL.md`:

```markdown
---
name: code-review
description: Perform thorough code review with best practices
---

# Code Review Skill

When reviewing code, follow this systematic approach:

## Review Checklist

1. **Correctness**: Does the code do what it's supposed to do?
2. **Security**: Are there any security vulnerabilities?
3. **Performance**: Are there obvious performance issues?
4. **Readability**: Is the code easy to understand?
5. **Testing**: Are there adequate tests?

## What to Look For

### Security Issues
- SQL injection
- XSS vulnerabilities
- Hardcoded credentials
- Improper input validation

### Code Quality
- Unused variables or imports
- Duplicated code
- Overly complex functions
- Missing error handling

## Output Format

Provide feedback in this format:

### Summary
Brief overview of the changes.

### Issues Found
- **[Severity]** Description of issue
  - Location: file:line
  - Suggestion: How to fix

### Positive Observations
Note well-written code and good practices.
```

### Advanced Example: Commit Messages

`.wonopcode/skills/commit/SKILL.md`:

```markdown
---
name: commit
description: Write conventional commit messages
---

# Commit Message Skill

Generate commit messages following the Conventional Commits specification.

## Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

## Types

| Type | Description |
|------|-------------|
| feat | New feature |
| fix | Bug fix |
| docs | Documentation |
| style | Formatting |
| refactor | Code restructuring |
| test | Adding tests |
| chore | Maintenance |

## Guidelines

1. **Subject line**: 50 characters max, imperative mood
2. **Body**: Wrap at 72 characters, explain what and why
3. **Footer**: Reference issues, breaking changes

## Process

1. Run `git diff --staged` to see changes
2. Analyze the changes to understand intent
3. Determine the appropriate type and scope
4. Write a clear, concise subject line
5. Add body if changes need explanation
6. Add footer for issue references

## Examples

```
feat(auth): add OAuth2 support for GitHub login

Implement GitHub OAuth2 authentication flow using the oauth2 crate.
Users can now sign in with their GitHub accounts.

Closes #123
```

```
fix(api): handle null response in user endpoint

The user endpoint was crashing when the database returned null
for optional fields. Added proper null handling.

Fixes #456
```
```

### Domain-Specific Skill

`.wonopcode/skills/rust-patterns/SKILL.md`:

```markdown
---
name: rust-patterns
description: Rust idioms and best practices
---

# Rust Patterns Skill

Apply idiomatic Rust patterns when writing or reviewing Rust code.

## Error Handling

### Use thiserror for Libraries
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MyError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}
```

### Use anyhow for Applications
```rust
use anyhow::{Context, Result};

fn main() -> Result<()> {
    let config = load_config()
        .context("Failed to load configuration")?;
    Ok(())
}
```

## Ownership Patterns

### Prefer borrowing over cloning
```rust
// Good
fn process(data: &str) -> Result<()>

// Avoid unless necessary
fn process(data: String) -> Result<()>
```

### Use Cow for flexibility
```rust
use std::borrow::Cow;

fn process(data: Cow<'_, str>) -> String {
    // Only clones if modification needed
    data.into_owned()
}
```

## Async Patterns

### Structured concurrency with tokio
```rust
let (result1, result2) = tokio::join!(
    async_operation1(),
    async_operation2()
);
```

### Graceful shutdown
```rust
tokio::select! {
    _ = server.run() => {},
    _ = shutdown_signal() => {
        server.shutdown().await;
    }
}
```
```

## Skill Organization

### By Task Type
```
.wonopcode/skills/
├── review/
│   └── SKILL.md          # Code review skill
├── commit/
│   └── SKILL.md          # Commit message skill
├── refactor/
│   └── SKILL.md          # Refactoring skill
└── debug/
    └── SKILL.md          # Debugging skill
```

### By Technology
```
.wonopcode/skills/
├── rust/
│   └── SKILL.md          # Rust patterns
├── typescript/
│   └── SKILL.md          # TypeScript patterns
└── docker/
    └── SKILL.md          # Docker best practices
```

### By Project Domain
```
.wonopcode/skills/
├── api-design/
│   └── SKILL.md          # API design guidelines
├── database/
│   └── SKILL.md          # Database schema patterns
└── testing/
    └── SKILL.md          # Testing strategies
```

## Best Practices

1. **Keep skills focused** - Each skill should address one specific task or domain
2. **Be specific** - Include concrete examples and patterns, not just general advice
3. **Use structured formats** - Checklists, tables, and step-by-step instructions work well
4. **Include examples** - Show expected input/output when applicable
5. **Update regularly** - Keep skills current with your project's evolving practices

## Skill Discovery Debugging

If skills aren't being discovered:

1. Check the file is named exactly `SKILL.md` (case-sensitive)
2. Verify the frontmatter is valid YAML
3. Ensure both `name` and `description` are present
4. Check the directory is in a discoverable location
5. Enable debug logging to see discovery process: `log_level: "debug"`

## Integration with Agents

Skills work with all agents but are most useful with:

- **build**: Full access to load and apply skills
- **plan**: Can reference skills for planning approaches
- **explore**: Can describe available skills

Custom agents can configure skill access:

```jsonc
{
  "agent": {
    "restricted-agent": {
      "permission": {
        "skill": "deny"  // No skill access
      }
    },
    "full-agent": {
      "permission": {
        "skill": "allow"  // Full skill access
      }
    }
  }
}
```
