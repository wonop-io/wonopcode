# System Prompt Architecture

## Overview

Wonop Code uses a **Tera template-based system prompt** that is re-rendered before every LLM API invocation. This ensures that dynamic values like date, time, git branch, and AGENTS.md content are always up-to-date.

## How It Works

1. The system prompt is defined as a Tera template in `wonopcode-core/src/system_prompt.rs` (constant `DEFAULT_SYSTEM_PROMPT_TEMPLATE`).
2. Before **every** `generate()` call in the agent loop, the template is rendered with fresh values.
3. If an HMS (Hierarchical Memory System) service is available, AGENTS.md is re-rendered from `memory.yaml` + `AGENTS.TEMPLATE.md` files. Otherwise, raw instruction files are read from disk.
4. This is a **unified approach** across all API-based providers (Anthropic, OpenAI, OpenRouter, etc.). The Claude CLI provider is excluded as it manages its own system prompt.

## Template Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `{{ agent_md }}` | Rendered AGENTS.md / custom instructions | Project-specific rules |
| `{{ date }}` | Current date | "Wed Mar 11 2026" |
| `{{ time }}` | Current time | "14:30:05" |
| `{{ branch }}` | Current git branch name | "main" |
| `{{ working_dir }}` | Absolute working directory path | "/home/user/project" |
| `{{ model_name }}` | LLM model identifier | "claude-sonnet-4-20250514" |
| `{{ provider }}` | Provider identifier | "anthropic" |
| `{{ platform }}` | OS platform | "macos" |
| `{{ is_git_repo }}` | Whether cwd is in a git repo | "yes" / "no" |

## How to Update the System Prompt

### Option 1: Edit the default template (for all users)

Edit the `DEFAULT_SYSTEM_PROMPT_TEMPLATE` constant in:

```
wonop/apps/wonopcode/communityedition/crates/wonopcode-core/src/system_prompt.rs
```

The template uses [Tera syntax](https://keats.github.io/tera/docs/) (similar to Jinja2).

Example:
```tera
You are Wonopcode, a powerful AI coding assistant.

{% if agent_md %}
# Project Instructions
{{ agent_md }}
{% endif %}

# Environment
Working directory: {{ working_dir }}
Date: {{ date }}
Branch: {{ branch }}
```

### Option 2: Customize per-project via AGENTS.md

Create instruction files in your project that get injected as `{{ agent_md }}`:

- `.wonopcode/AGENTS.md` (preferred)
- `AGENTS.md`
- `CLAUDE.md`
- `.claude/CLAUDE.md`
- `.wonopcode/instructions.md`
- `.cursor/rules`

### Option 3: Use HMS templates for dynamic project instructions

For more advanced customization, use the Hierarchical Memory System:

1. Create `.wonopcode/memory.yaml` with key-value memories
2. Create `.wonopcode/AGENTS.TEMPLATE.md` as a Tera template
3. The HMS renders AGENTS.md from these, and the result becomes `{{ agent_md }}`

See the HMS documentation for details.

## Architecture Details

### Flow

```
Runner creates RunnerSystemPromptSource
    ↓
LoopContext.system_prompt_source = Some(source)
    ↓
StandardLoop.run_prompt() starts iteration loop
    ↓
Before each generate() call:
    source.render_system_prompt() is called
        ↓
        1. HMS renders AGENTS.md (or falls back to disk files)
        2. SystemPromptVars gathers date, time, branch, etc.
        3. Tera template is rendered with all variables
        ↓
    Rendered prompt + OM observations + RAG context = final system prompt
    ↓
provider.generate(messages, options_with_system_prompt)
```

### Key Files

| File | Purpose |
|------|---------|
| `wonopcode-core/src/system_prompt.rs` | Template, renderer, variables, `load_custom_instructions()` |
| `wonopcode-agent-loop/src/context.rs` | `SystemPromptSource` trait, `LoopContext.system_prompt_source` |
| `wonopcode-agent-loop/src/standard/mod.rs` | Uses `system_prompt_source` in the iteration loop |
| `wonopcode-runner/src/runner.rs` | `RunnerSystemPromptSource` implementation |
| `wonopcode-tools/src/hms/renderer.rs` | HMS AGENTS.md template rendering |

### Fallback Chain

1. **`system_prompt_source`** (dynamic, re-rendered each invocation) — used when available
2. **`config.system_prompt`** (static string) — fallback for backward compatibility
3. If neither is set, no system prompt is sent
