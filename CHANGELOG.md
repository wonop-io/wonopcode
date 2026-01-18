# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2025-01-06

### Added

- Initial release of Wonopcode, inspired by [OpenCode](https://github.com/sst/opencode)
- **Multi-provider AI support**: Anthropic Claude, OpenAI, Google Gemini, OpenRouter, Azure OpenAI, AWS Bedrock, xAI Grok, Mistral, Groq
- **Rich Terminal UI**: Markdown rendering, syntax highlighting, split panes
- **Built-in tools**: File operations (read, write, edit, multiedit, patch), code search (glob, grep), shell execution (bash), web fetching, LSP integration
- **MCP Protocol support**: Extensible tool integration via Model Context Protocol (local and remote with OAuth)
- **Session management**: Undo/redo, forking, conversation compaction
- **ACP Protocol**: IDE integration with VSCode, Zed, and Cursor
- **Snapshot system**: Track and revert file changes
- **Sandbox mode**: Isolated Docker container execution for secure tool operations
- **Custom agents**: Configurable agent personalities and tool permissions
- **Custom commands**: Define reusable prompts via config or markdown files
- **Authentication**: Built-in OAuth support for AI providers
