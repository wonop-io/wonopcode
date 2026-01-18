# Hooks System

Hooks allow you to run custom commands in response to events during a wonopcode session. This enables automation, integration with external tools, and custom workflows.

## Overview

Hooks are shell commands that execute when specific events occur. Each hook receives context about the event through environment variables.

## Hook Events

| Event | Description | When Triggered |
|-------|-------------|----------------|
| `file_edited` | A file was edited | After any file modification via Edit, Write, or Patch tools |
| `session_completed` | Session ended | When a session is closed or completed |
| `message_sent` | Message sent | After a message is sent in the conversation |
| `tool_executed` | Tool executed | After any tool completes execution |

## Hook Configuration

Hooks are defined in your configuration file:

```jsonc
{
  "hooks": {
    "file_edited": {
      "command": ["./scripts/on-file-edit.sh", "$FILE"],
      "environment": {
        "PROJECT_ROOT": "/path/to/project"
      }
    },
    "session_completed": {
      "command": ["./scripts/cleanup.sh"]
    }
  }
}
```

### Hook Properties

| Property | Type | Description |
|----------|------|-------------|
| `command` | `string[]` | Command and arguments to execute |
| `environment` | `object` | Additional environment variables |

## Context Variables

Hooks receive context through environment variables. You can reference these in your command using `$VAR` or `${VAR}` syntax.

### file_edited Context

| Variable | Description |
|----------|-------------|
| `FILE` | Full path to the edited file |
| `EXT` | File extension |

### session_completed Context

| Variable | Description |
|----------|-------------|
| `SESSION_ID` | Unique session identifier |

### tool_executed Context

| Variable | Description |
|----------|-------------|
| `TOOL_NAME` | Name of the tool that was executed |

## File Pattern Hooks

For `file_edited` events, you can register hooks that only trigger for specific file patterns:

```jsonc
{
  "hooks": {
    "file_edited": {
      "*.rs": {
        "command": ["cargo", "fmt", "--", "$FILE"]
      },
      "*.ts": {
        "command": ["prettier", "--write", "$FILE"]
      },
      "package.json": {
        "command": ["npm", "install"]
      }
    }
  }
}
```

### Pattern Matching

- `*.ext` - Match by file extension
- `filename` - Exact filename match
- `path/to/*.ext` - Match with path prefix
- `*` - Match any file

## Examples

### Auto-format on Edit

Run formatters when specific file types are edited:

```jsonc
{
  "hooks": {
    "file_edited": {
      "*.rs": {
        "command": ["cargo", "fmt", "--", "$FILE"]
      },
      "*.py": {
        "command": ["black", "$FILE"]
      },
      "*.go": {
        "command": ["gofmt", "-w", "$FILE"]
      }
    }
  }
}
```

### Run Tests on Change

Trigger test runs when source files change:

```jsonc
{
  "hooks": {
    "file_edited": {
      "src/*.rs": {
        "command": ["cargo", "test", "--lib"]
      },
      "tests/*.rs": {
        "command": ["cargo", "test"]
      }
    }
  }
}
```

### Session Logging

Log session activity for auditing:

```jsonc
{
  "hooks": {
    "session_completed": {
      "command": ["./scripts/log-session.sh"],
      "environment": {
        "LOG_DIR": "/var/log/wonopcode"
      }
    }
  }
}
```

### Notify on Completion

Send notifications when sessions complete:

```jsonc
{
  "hooks": {
    "session_completed": {
      "command": ["notify-send", "Wonopcode", "Session completed"]
    }
  }
}
```

## Best Practices

1. **Keep hooks fast** - Hooks run synchronously and can slow down the experience if they take too long.

2. **Handle failures gracefully** - Hook failures are logged but don't stop the main operation. Design your hooks to fail silently when appropriate.

3. **Use absolute paths** - When referencing scripts, use absolute paths or ensure the working directory is set correctly.

4. **Test hooks independently** - Test your hook commands outside of wonopcode first to ensure they work correctly.

5. **Avoid infinite loops** - Be careful with `file_edited` hooks that modify files, as they could trigger themselves.

## Troubleshooting

### Hook not running

- Check that the event name is spelled correctly
- Verify the command exists and is executable
- Check the logs for hook execution errors (`log_level: "debug"`)

### Hook failing silently

- Run the command manually to see error output
- Add logging to your hook script
- Check that all required environment variables are set

### Pattern not matching

- File patterns are matched against the full file path
- Use `log_level: "debug"` to see which patterns are being checked
- Test patterns with simpler wildcards first

## Limitations

- Hooks run in a subprocess with limited access to wonopcode internals
- Long-running hooks may cause delays in the UI
- Hook output is captured but not displayed in the main interface (check logs)
- Hooks cannot modify the conversation or tool results

## Future Enhancements

The hooks system is designed to be extensible. Planned features include:

- Additional events (pre-edit, model response, error handling)
- Async hook execution for long-running tasks
- Hook result integration (using hook output in the conversation)
- Web hook support for remote integrations
