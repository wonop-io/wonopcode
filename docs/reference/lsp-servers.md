# LSP Servers

Language Server Protocol (LSP) integration for code intelligence features.

---

## Overview

Wonopcode provides built-in LSP support for 35+ programming languages. LSP enables:

- **Go to definition** - Jump to where symbols are defined
- **Find references** - Find all usages of a symbol
- **Hover information** - View type info and documentation
- **Document symbols** - List all symbols in a file
- **Workspace symbols** - Search symbols across the project
- **Call hierarchy** - See incoming/outgoing function calls
- **Diagnostics** - View compiler errors and warnings

---

## How It Works

LSP servers are activated lazily when you access files of a supported type:

1. **File access** - You read or edit a file
2. **Detection** - Wonopcode detects the file extension
3. **Spawn** - The appropriate language server is started
4. **Intelligence** - Code features become available

Servers stay running for the session and are reused for subsequent requests.

---

## Supported Languages

### Tier 1: Primary Languages

| Language | Server | Command | Extensions |
|----------|--------|---------|------------|
| Rust | rust-analyzer | `rust-analyzer` | `.rs` |
| TypeScript/JavaScript | typescript-language-server | `typescript-language-server --stdio` | `.ts`, `.tsx`, `.js`, `.jsx` |
| Python | pyright | `pyright-langserver --stdio` | `.py` |
| Go | gopls | `gopls` | `.go` |
| C/C++ | clangd | `clangd` | `.c`, `.cpp`, `.cc`, `.cxx`, `.h`, `.hpp`, `.hxx` |
| Java | jdtls | `jdtls` | `.java` |
| C# | csharp-ls | `csharp-ls` | `.cs` |

### Tier 2: Web Development

| Language | Server | Command | Extensions |
|----------|--------|---------|------------|
| Vue | vue-language-server | `vue-language-server --stdio` | `.vue` |
| Svelte | svelteserver | `svelteserver --stdio` | `.svelte` |
| HTML | vscode-html-language-server | `vscode-html-language-server --stdio` | `.html`, `.htm` |
| CSS/SCSS/LESS | vscode-css-language-server | `vscode-css-language-server --stdio` | `.css`, `.scss`, `.less` |
| Tailwind CSS | tailwindcss-language-server | `tailwindcss-language-server --stdio` | `.html`, `.jsx`, `.tsx`, `.vue`, `.svelte` |
| GraphQL | graphql-lsp | `graphql-lsp server -m stream` | `.graphql`, `.gql` |
| Prisma | prisma-language-server | `prisma-language-server --stdio` | `.prisma` |

### Tier 3: Scripting Languages

| Language | Server | Command | Extensions |
|----------|--------|---------|------------|
| Ruby | solargraph | `solargraph stdio` | `.rb`, `.rake`, `.gemspec` |
| PHP | intelephense | `intelephense --stdio` | `.php` |
| Lua | lua-language-server | `lua-language-server` | `.lua` |
| Bash | bash-language-server | `bash-language-server start` | `.sh`, `.bash`, `.zsh` |

### Tier 4: Functional Languages

| Language | Server | Command | Extensions |
|----------|--------|---------|------------|
| Haskell | haskell-language-server | `haskell-language-server-wrapper --lsp` | `.hs`, `.lhs` |
| OCaml | ocamllsp | `ocamllsp` | `.ml`, `.mli` |
| Clojure | clojure-lsp | `clojure-lsp` | `.clj`, `.cljs`, `.cljc`, `.edn` |
| Elixir | elixir-ls | `elixir-ls` | `.ex`, `.exs` |
| Erlang | erlang_ls | `erlang_ls` | `.erl`, `.hrl` |
| Scala | metals | `metals` | `.scala`, `.sbt`, `.sc` |
| Gleam | gleam | `gleam lsp` | `.gleam` |

### Tier 5: Systems Languages

| Language | Server | Command | Extensions |
|----------|--------|---------|------------|
| Swift | sourcekit-lsp | `sourcekit-lsp` | `.swift` |
| Kotlin | kotlin-language-server | `kotlin-language-server` | `.kt`, `.kts` |
| Zig | zls | `zls` | `.zig` |
| Dart | dart | `dart language-server` | `.dart` |

### Tier 6: Configuration & Markup

| Language | Server | Command | Extensions |
|----------|--------|---------|------------|
| YAML | yaml-language-server | `yaml-language-server --stdio` | `.yaml`, `.yml` |
| JSON | vscode-json-language-server | `vscode-json-language-server --stdio` | `.json`, `.jsonc` |
| Terraform | terraform-ls | `terraform-ls serve` | `.tf`, `.tfvars` |
| Docker | docker-langserver | `docker-langserver --stdio` | `Dockerfile` |
| Nix | nixd | `nixd` | `.nix` |
| LaTeX | texlab | `texlab` | `.tex`, `.bib`, `.sty`, `.cls` |
| Markdown | marksman | `marksman` | `.md`, `.markdown` |
| SQL | sqls | `sqls` | `.sql` |

### Special Cases

| Language | Server | Command | Notes |
|----------|--------|---------|-------|
| Deno | deno | `deno lsp` | Conflicts with TypeScript; enable explicitly |

---

## Installation

Language servers must be installed separately. Here are common installation methods:

### Rust
```bash
rustup component add rust-analyzer
```

### TypeScript/JavaScript
```bash
npm install -g typescript-language-server typescript
```

### Python
```bash
pip install pyright
# or
npm install -g pyright
```

### Go
```bash
go install golang.org/x/tools/gopls@latest
```

### C/C++
```bash
# macOS
brew install llvm

# Ubuntu/Debian
apt install clangd

# Arch
pacman -S clang
```

### Ruby
```bash
gem install solargraph
```

### Rust-analyzer (standalone)
```bash
# macOS
brew install rust-analyzer

# Or download from GitHub releases
```

---

## Configuration

### Custom LSP Server

Add custom servers in your config:

```json
{
  "lsp": {
    "custom-lang": {
      "language": "custom",
      "command": "custom-lsp",
      "args": ["--stdio"],
      "extensions": ["cst"],
      "root_patterns": ["custom.config"],
      "enabled": true
    }
  }
}
```

### Configuration Options

| Option | Type | Description |
|--------|------|-------------|
| `language` | string | Language identifier |
| `command` | string | LSP server executable |
| `args` | array | Command arguments |
| `extensions` | array | File extensions to handle |
| `root_patterns` | array | Files that indicate project root |
| `env` | object | Environment variables |
| `enabled` | boolean | Enable/disable the server |

### Disable a Built-in Server

```json
{
  "lsp": {
    "typescript": {
      "enabled": false
    }
  }
}
```

### Override Server Command

```json
{
  "lsp": {
    "python": {
      "command": "pylsp",
      "args": []
    }
  }
}
```

---

## Usage

### In Prompts

Ask the AI to use LSP features:

```
Go to the definition of the `UserService` class
Find all references to the `authenticate` function
What type is the variable `config` on line 42?
```

### Tool Parameters

The LSP tool accepts:

| Parameter | Required | Description |
|-----------|----------|-------------|
| `operation` | Yes | `definition`, `references`, `hover`, `symbols` |
| `file` | Yes | File path |
| `line` | Yes* | Line number (0-based) |
| `column` | Yes* | Column number (0-based) |

*Required for `definition`, `references`, `hover`

---

## Workspace Detection

LSP servers need to know the project root. Wonopcode detects this using root patterns:

| Language | Root Patterns |
|----------|---------------|
| Rust | `Cargo.toml` |
| TypeScript | `package.json`, `tsconfig.json` |
| Python | `pyproject.toml`, `setup.py`, `requirements.txt` |
| Go | `go.mod` |
| Java | `pom.xml`, `build.gradle`, `settings.gradle` |
| C# | `*.csproj`, `*.sln` |
| Ruby | `Gemfile`, `*.gemspec` |

---

## Troubleshooting

### Server Not Starting

1. **Check installation**: Ensure the server is installed and in PATH
   ```bash
   which rust-analyzer
   ```

2. **Check permissions**: Server must be executable
   ```bash
   chmod +x /path/to/server
   ```

3. **View logs**: Enable debug logging
   ```bash
   RUST_LOG=debug wonopcode
   ```

### Wrong Server for File Type

If the wrong server activates (e.g., TypeScript server for Deno files):

```json
{
  "lsp": {
    "typescript": {
      "extensions": ["ts", "tsx"]
    },
    "deno": {
      "enabled": true,
      "extensions": ["ts", "tsx"],
      "root_patterns": ["deno.json", "deno.jsonc"]
    }
  }
}
```

### Server Crashes

Wonopcode tracks broken servers and avoids repeated spawn failures. To reset:

1. Restart wonopcode
2. Or wait for the cooldown period

### Slow Performance

1. **Reduce scope**: Some servers index the entire workspace
2. **Exclude directories**: Use `.gitignore` or server-specific config
3. **Increase resources**: Some servers need more memory

---

## See Also

- [Tools Overview](../guides/tools-overview.md) - All available tools
- [Configuration](../CONFIGURATION.md) - Full config reference
