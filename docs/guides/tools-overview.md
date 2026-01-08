# Tools Overview

Wonopcode provides powerful built-in tools that the AI uses to interact with your codebase and environment.

---

## How Tools Work

When you ask the AI to perform a task, it selects and uses appropriate tools:

1. **Selection**: AI determines which tool(s) to use
2. **Parameters**: AI provides required arguments
3. **Permission**: Tool checks permissions (may prompt you)
4. **Execution**: Tool runs (in sandbox if enabled)
5. **Result**: Output is returned to AI for interpretation

---

## File Tools

### Read

**Purpose**: Read file contents with line numbers

```
Read the configuration file at config/settings.json
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `filePath` | Yes | Absolute path to file |
| `offset` | No | Starting line (0-based) |
| `limit` | No | Number of lines (default: 2000) |

**Output**:
```
    1│ {
    2│   "database": {
    3│     "host": "localhost",
    4│     "port": 5432
    5│   }
    6│ }
```

**Notes**:
- Line numbers are 1-based in output
- Large files are automatically truncated
- Binary files are detected and handled

---

### Write

**Purpose**: Create or overwrite a file

```
Create a new file src/utils/helpers.rs with a string formatting function
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `filePath` | Yes | Absolute path to file |
| `content` | Yes | File content |

**Notes**:
- Creates parent directories automatically
- Overwrites existing files (use edit for modifications)
- Snapshot is taken before overwriting

---

### Edit

**Purpose**: Modify existing files with fuzzy matching

```
In src/main.rs, change the port from 8080 to 3000
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `filePath` | Yes | Absolute path to file |
| `oldString` | Yes | Text to find |
| `newString` | Yes | Replacement text |
| `replaceAll` | No | Replace all occurrences |

**Matching Strategies**:

The edit tool uses 9 matching strategies to find the target text:

1. **Exact match** - Literal string match
2. **Whitespace-normalized** - Ignores whitespace differences
3. **Leading whitespace flexible** - Tolerates indentation changes
4. **Trailing whitespace flexible** - Tolerates trailing spaces
5. **Both whitespace flexible** - Combination of 3 and 4
6. **Line-by-line fuzzy** - Matches most lines
7. **Block fuzzy** - Matches surrounding context
8. **Anchor-based** - Uses first/last lines as anchors
9. **Best fuzzy match** - Highest similarity above threshold

**Notes**:
- Fails if `oldString` matches multiple locations (unless `replaceAll`)
- Shows diff in output
- Snapshot is taken before editing

---

### MultiEdit

**Purpose**: Apply multiple edits atomically

```
Rename the function 'processData' to 'handleData' across all files
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `edits` | Yes | Array of edit operations |

Each edit has: `filePath`, `oldString`, `newString`, `replaceAll`

**Notes**:
- All edits validated before any are applied
- Atomic: if one fails, none are applied
- Useful for refactoring across files

---

### Patch

**Purpose**: Apply unified diff patches

```
Apply this patch to fix the security vulnerability
```

**Format**:
```
*** Begin Patch
*** Update File: src/auth.rs
@@ context line
 unchanged line
-removed line
+added line
*** End Patch
```

**Supported Operations**:
- `*** Add File: path` - Create new file
- `*** Delete File: path` - Remove file
- `*** Update File: path` - Modify file
- `*** Move to: new/path` - Rename/move file

---

## Search Tools

### Glob

**Purpose**: Find files by pattern

```
Find all TypeScript files in the src directory
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `pattern` | Yes | Glob pattern (e.g., `**/*.ts`) |
| `path` | No | Directory to search (default: project root) |

**Pattern Examples**:
| Pattern | Matches |
|---------|---------|
| `*.rs` | Rust files in current directory |
| `**/*.rs` | All Rust files recursively |
| `src/**/*.ts` | TypeScript files under src/ |
| `*.{js,ts}` | JS or TS files |
| `test_*.py` | Python test files |

**Output**:
```
src/main.rs
src/lib.rs
src/utils/helpers.rs
```

Files are sorted by modification time (newest first).

---

### Grep

**Purpose**: Search file contents with regex

```
Find all TODO comments in the codebase
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `pattern` | Yes | Regex pattern |
| `path` | No | Directory to search |
| `include` | No | File pattern filter (e.g., `*.rs`) |

**Pattern Examples**:
| Pattern | Matches |
|---------|---------|
| `TODO` | Literal "TODO" |
| `TODO\|FIXME` | TODO or FIXME |
| `fn \w+\(` | Function definitions |
| `import.*from` | ES6 imports |

**Output**:
```
src/main.rs:42: // TODO: Add error handling
src/lib.rs:15: // TODO: Optimize this loop
tests/test_api.rs:8: // FIXME: Flaky test
```

**Notes**:
- Uses ripgrep for fast searching
- Respects `.gitignore`
- Limited to 100 matches by default

---

### List

**Purpose**: List directory contents as tree

```
Show me the structure of the src directory
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `path` | No | Directory path (default: project root) |
| `ignore` | No | Patterns to ignore |

**Output**:
```
src/
├── main.rs
├── lib.rs
├── api/
│   ├── mod.rs
│   ├── handlers.rs
│   └── routes.rs
└── utils/
    └── helpers.rs
```

---

## Execution Tools

### Bash

**Purpose**: Execute shell commands

```
Run the test suite with verbose output
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `command` | Yes | Shell command to execute |
| `description` | Yes | Human-readable description |
| `workdir` | No | Working directory |
| `timeout` | No | Timeout in ms (default: 120000) |

**Output**:
```
┌─ bash ────────────────────────────────────────────────────────┐
│ Command: cargo test -- --nocapture                            │
│ Exit: 0                                                       │
├───────────────────────────────────────────────────────────────┤
│ running 5 tests                                               │
│ test tests::test_parse ... ok                                 │
│ test tests::test_format ... ok                                │
│ ...                                                           │
└───────────────────────────────────────────────────────────────┘
```

**Notes**:
- Subject to permission checks
- Runs in sandbox if enabled
- Timeout prevents runaway commands

**Permission Patterns**:

Configure in `config.json`:
```json
{
  "permission": {
    "bash": {
      "ls": "allow",
      "cat": "allow",
      "git *": "allow",
      "rm *": "deny",
      "sudo *": "deny"
    }
  }
}
```

---

## Web Tools

### WebFetch

**Purpose**: Fetch web content and convert to markdown

```
Fetch the documentation at https://docs.rs/tokio/latest/tokio/
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `url` | Yes | URL to fetch |
| `format` | No | Output format: `markdown`, `text`, `html` |
| `timeout` | No | Timeout in seconds (default: 30) |

**Notes**:
- Automatically converts HTML to markdown
- Respects robots.txt
- Useful for fetching documentation

---

### WebSearch

**Purpose**: Search the web using Exa AI

```
Search for "rust async error handling best practices"
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `query` | Yes | Search query |
| `num_results` | No | Number of results (default: 8) |

**Notes**:
- Optimized for technical queries
- Returns summaries and URLs
- Requires network access

---

### CodeSearch

**Purpose**: Search for code examples and API documentation

```
Find examples of using tokio::spawn with error handling
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `query` | Yes | Code/API search query |
| `tokens_num` | No | Response tokens (default: 5000) |

**Notes**:
- Optimized for code-related queries
- Returns programming context
- Great for API usage examples

---

## Meta Tools

### Task

**Purpose**: Spawn subagent for focused tasks

```
Have an agent explore the authentication module and summarize how it works
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `prompt` | Yes | Task for the subagent |
| `description` | Yes | Human-readable description |
| `subagent_type` | Yes | Agent type: `code`, `explore`, etc. |

**Subagent Types**:
| Type | Purpose |
|------|---------|
| `code` | Code modifications |
| `explore` | Codebase exploration (read-only) |
| `build` | Build and test operations |

**Notes**:
- Creates isolated child session
- Subagent has its own tool permissions
- Results are returned to parent session

---

### TodoRead

**Purpose**: Read current task list

```
Show me my current todo list
```

**Output**:
```
## In Progress
[>] Implement user authentication

## Pending
[ ] Add unit tests
[ ] Update documentation

## Completed
[x] Set up project structure
```

---

### TodoWrite

**Purpose**: Update task list

```
Add "Review pull request #42" to my todo list
```

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `todos` | Yes | Array of todo items |

**Todo Item Structure**:
```json
{
  "id": "unique-id",
  "content": "Task description",
  "status": "pending",
  "priority": "high"
}
```

**Status Values**: `pending`, `in_progress`, `completed`, `cancelled`
**Priority Values**: `high`, `medium`, `low`

---

## LSP Tools

### LSP Operations

**Purpose**: Code intelligence via Language Server Protocol

```
Go to the definition of the 'UserService' class
```

**Operations**:
| Operation | Description |
|-----------|-------------|
| `definition` | Go to symbol definition |
| `references` | Find all references |
| `hover` | Get type info and docs |
| `symbols` | List symbols in file |

**Parameters**:
| Parameter | Required | Description |
|-----------|----------|-------------|
| `operation` | Yes | LSP operation |
| `file` | Yes | File path |
| `line` | Yes* | Line number (0-based) |
| `column` | Yes* | Column number (0-based) |

*Required for definition/references/hover

**Notes**:
- Requires language server for the file type
- Supported: Rust, TypeScript/JavaScript, Python, Go
- Provides accurate code navigation

---

## Tool Behavior with Sandbox

When sandbox is enabled, tools behave differently:

| Tool | Direct Mode | Sandboxed Mode |
|------|-------------|----------------|
| `read` | Reads from host | Reads from container |
| `write` | Writes to host | Writes via container |
| `edit` | Edits on host | Edits via container |
| `bash` | Runs on host | Runs in container |
| `glob` | Uses host filesystem | Uses container filesystem |
| `grep` | Uses host ripgrep | Uses container ripgrep |

**Bypass Tools**: Some tools always run on host:
- `todoread` / `todowrite`
- `webfetch` / `websearch` / `codesearch`
- `skill`

Configure bypass in `config.json`:
```json
{
  "sandbox": {
    "bypass_tools": ["todoread", "todowrite", "webfetch"]
  }
}
```

---

## Tool Permissions

### Auto-Approve

Safe tools that execute without prompting:

```json
{
  "permission": {
    "auto_approve": ["read", "glob", "grep", "list", "todoread"]
  }
}
```

### Auto-Deny

Tools that are always blocked:

```json
{
  "permission": {
    "auto_deny": ["bash"]
  }
}
```

### Per-Tool Settings

In agent configuration:

```json
{
  "agent": {
    "readonly": {
      "tools": {
        "*": false,
        "read": true,
        "glob": true,
        "grep": true
      }
    }
  }
}
```

---

## Error Handling

Tools can fail for various reasons:

| Error | Cause | Solution |
|-------|-------|----------|
| `Validation error` | Invalid parameters | AI will retry with corrections |
| `Permission denied` | Not allowed | Approve or configure permissions |
| `Execution failed` | Runtime error | Check error message |
| `Timeout` | Took too long | Increase timeout or simplify |
| `Cancelled` | User cancelled | Re-run if needed |

The AI typically handles errors gracefully and retries or asks for guidance.

---

## Next Steps

- [MCP Servers](./mcp-servers.md) - Add external tools
- [Custom Agents](./custom-agents.md) - Configure tool access per agent
- [Configuration](../CONFIGURATION.md) - Full permission settings
