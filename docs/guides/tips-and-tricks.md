# Tips & Tricks

Power user techniques for getting the most out of wonopcode.

---

## Effective Prompting

### Be Specific

Instead of vague requests, provide details:

```
# ✗ Vague
Fix the bug

# ✓ Specific
Fix the null pointer exception in src/parser.rs line 42 
that occurs when the input string is empty
```

### Provide Context

Include relevant information upfront:

```
I'm building a REST API with Axum and SQLx. The database is PostgreSQL.
Add a new endpoint POST /users that creates a user with email validation.
```

### Use Examples

Show what you want:

```
Refactor this function to use the same pattern as `process_order` in src/orders.rs:
- Extract validation into a separate function
- Use the Result type for error handling
- Add logging at each step
```

### Break Down Complex Tasks

For large changes, work incrementally:

```
Step 1: First, let's understand the current authentication flow
Step 2: Now let's plan the changes needed for JWT support
Step 3: Implement the JWT token generation
Step 4: Add the middleware
Step 5: Update the tests
```

---

## Keyboard Mastery

### Essential Shortcuts

| Shortcut | Action |
|----------|--------|
| `Enter` | Send message |
| `Ctrl+C` | Cancel operation |
| `Ctrl+L` | Clear screen |
| `Ctrl+D` | Quit |
| `Esc` | Cancel input |

### Leader Sequences

Press `Ctrl+X` then:

| Key | Action |
|-----|--------|
| `N` | New session |
| `S` | Switch session |
| `M` | Change model |
| `U` | Undo |
| `R` | Redo |
| `C` | Compact history |

### Input Editing

| Shortcut | Action |
|----------|--------|
| `Ctrl+A` | Start of line |
| `Ctrl+E` | End of line |
| `Ctrl+W` | Delete word |
| `Ctrl+U` | Delete to start |
| `Ctrl+K` | Delete to end |
| `↑` / `↓` | History navigation |

---

## Session Management

### Naming Sessions

Create descriptive sessions:

```
/new auth-refactor
/new bug-fix-123
/new feature-dark-mode
```

### Switching Sessions

```
/sessions              # List all
/switch auth-refactor  # Switch to specific
Ctrl+X, S              # Quick switch
```

### Exporting Work

Save sessions for documentation:

```
/export markdown       # Export as markdown
/export json          # Export as JSON
```

### Cleaning Up

```
/delete old-session   # Delete specific
/clear               # Clear current messages
```

---

## Managing Long Conversations

### When to Compact

Compact when:
- Responses slow down
- Token count is high
- Context becomes unfocused

```
/compact
```

### Manual Context Setting

Start fresh but with context:

```
/new
```

Then provide a summary:

```
I've been working on adding JWT auth. So far:
- Added jsonwebtoken dependency
- Created Token struct in src/auth/mod.rs
- Middleware is partially done

Let's continue with the middleware.
```

### Using Task for Focused Work

Spawn subagents for specific tasks:

```
Have an explore agent analyze the database module and summarize the schema
```

The result comes back without cluttering your main conversation.

---

## Working with Code

### Reading Strategically

Don't read entire files. Be specific:

```
Show me the error handling in the `process_payment` function
```

```
Read lines 50-100 of src/api/handlers.rs
```

### Efficient Edits

For multiple similar changes, use patterns:

```
In all files matching src/api/*.rs, add #[tracing::instrument] 
to every pub async fn
```

### Reviewing Before Applying

Always review diffs:

```
Show me what changes you would make before applying them
```

When prompted for permission, press `V` to view diff.

### Using Snapshots

Enable snapshots for easy rollback:

```json
{ "snapshot": true }
```

Then:
```
/undo     # Revert last change
/revert   # Revert to specific point
```

---

## Model Selection

### When to Use Which Model

| Task | Recommended Model |
|------|-------------------|
| Complex reasoning | Claude Opus / GPT-4 |
| General coding | Claude Sonnet / GPT-4o |
| Quick questions | Claude Haiku / GPT-3.5 |
| Code completion | Fast models |

### Dynamic Switching

```
/model claude-haiku    # Quick question
/model claude-sonnet   # Back to main work
```

### Configure Defaults

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929",
  "small_model": "anthropic/claude-haiku-3-5-20241022"
}
```

---

## Sandbox Tips

### Check Sandbox Status

```
/sandbox status
```

### Interactive Debugging

```
/sandbox shell
```

Opens a shell in the sandbox for manual exploration.

### Performance Optimization

```json
{
  "sandbox": {
    "keep_alive": true,          // Don't restart container
    "persist_caches": true,      // Keep package caches
    "resources": {
      "memory": "4G",            // More memory for builds
      "cpus": 4
    }
  }
}
```

### When to Disable

For trusted, simple operations:

```
/sandbox stop
```

Or per-session:
```bash
wonopcode --no-sandbox
```

---

## MCP Power Usage

### Multiple Servers

Configure complementary servers:

```json
{
  "mcp": {
    "github": { "command": "..." },
    "postgres": { "command": "..." },
    "memory": { "command": "..." }
  }
}
```

### Cross-Tool Workflows

```
1. Search GitHub for similar implementations
2. Check our database schema
3. Generate the code based on both
```

### Custom Tools for Repetitive Tasks

Create an MCP server for your workflow:

```python
# Deploy script as MCP tool
def deploy_staging():
    # Your deployment logic
    pass
```

---

## Agent Strategies

### Task-Specific Agents

```
/agent security     # For security review
/agent docs         # For documentation
/agent tests        # For test generation
```

### Creating Quick Agents

For one-off needs, describe the role:

```
Act as a database optimization expert. Review my queries 
and suggest index improvements.
```

### Agent Chaining

Use Task tool to chain agents:

```
1. Have the explore agent understand the codebase
2. Have the security agent review for vulnerabilities
3. Have the code agent fix the issues
```

---

## Debugging Assistance

### Error Analysis

Paste full error messages:

```
I'm getting this error:
```
error[E0382]: borrow of moved value: `data`
  --> src/main.rs:10:20
   |
8  |     let data = vec![1, 2, 3];
   |         ---- move occurs because `data` has type `Vec<i32>`
9  |     process(data);
   |             ---- value moved here
10 |     println!("{:?}", data);
   |                      ^^^^ value borrowed here after move
```
Help me understand and fix this.
```

### Log Analysis

```
Here are the relevant logs from the last 5 minutes:
[paste logs]

What's causing the connection timeouts?
```

### Reproduction Steps

```
The bug occurs when:
1. User logs in
2. Navigates to /dashboard
3. Clicks refresh
4. Gets a 500 error

Help me trace through the code to find the issue.
```

---

## Performance Optimization

### Reduce Token Usage

1. **Be concise** in prompts
2. **Compact** regularly
3. **Use specific file reads** instead of globbing everything
4. **Clear** irrelevant history

### Speed Up Responses

1. Use **faster models** for simple tasks
2. **Pre-warm** sandbox with `/sandbox start`
3. **Enable caching** for packages
4. Use **local** instead of remote models when possible

### Batch Operations

Instead of:
```
Add logging to function A
Add logging to function B
Add logging to function C
```

Do:
```
Add logging to functions A, B, and C in src/handlers.rs
```

---

## Common Patterns

### Code Review Workflow

```
1. /agent reviewer
2. Review src/new-feature.rs for bugs, security, and style
3. [Review feedback]
4. /agent code
5. Apply the suggested fixes
```

### Documentation Workflow

```
1. Generate documentation for the public API in src/lib.rs
2. Create a README with usage examples
3. Add inline comments for complex functions
```

### Refactoring Workflow

```
1. First, explain the current architecture of src/database/
2. Now let's plan a refactor to use connection pooling
3. [Review plan]
4. Implement step 1: Add the pool configuration
5. [Continue incrementally]
```

### Test-Driven Development

```
1. Write tests for the UserService.create method
2. [Tests created]
3. Now implement the method to make the tests pass
4. Run the tests to verify
```

---

## Troubleshooting

### Slow Responses

- Check network connection
- Try `/compact` to reduce context
- Switch to faster model
- Check API rate limits

### Incorrect Code Changes

- Use `/undo` to revert
- Be more specific in requests
- Provide examples of desired output
- Review diffs before accepting

### Tool Failures

- Check error messages
- Verify permissions
- Check sandbox status
- Try running command manually

### Lost Context

- Export important sessions
- Use Task tool for isolated exploration
- Start new session with summary

---

## Customization Ideas

### Project-Specific Setup

Create `.wonopcode/config.json`:

```json
{
  "model": "anthropic/claude-sonnet-4-5-20250929",
  "permission": {
    "bash": {
      "make *": "allow",
      "docker-compose *": "allow"
    }
  }
}
```

### Team Standards

Share agent definitions:

```
.wonopcode/
├── config.json         # Project config
└── agent/
    ├── pr-review.md    # Team review standards
    ├── security.md     # Security checklist
    └── style.md        # Style enforcement
```

### Workflow Automation

Combine with shell scripts:

```bash
#!/bin/bash
# quick-review.sh
wonopcode -p "Review the changes in $(git diff --name-only HEAD~1)"
```

---

## Next Steps

- [Configuration](../CONFIGURATION.md) - Customize everything
- [Custom Agents](./custom-agents.md) - Create specialized agents
- [Security Model](../architecture/security-model.md) - Understand permissions
