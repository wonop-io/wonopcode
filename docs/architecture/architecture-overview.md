# Architecture Overview

The project is split into multiple crates, each with a distinct purpose. Below is an overview:

## Crates
1. **wonopcode-core**
   - Houses core functionality and data structures shared across the project.
   - Manages core logic for reading/writing data, central abstractions, etc.

2. **wonopcode-auth**
   - Handles authentication and authorization features.
   - Integrates with external auth providers if required.

3. **wonopcode-lsp**
   - LSP (Language Server Protocol) related functionalities.
   - Provides symbol analysis, references, completions, etc.

4. **wonopcode-acp**
   - Possibly stands for auto-completion or analysis?
   - If any placeholders exist, you can fill them with details about the crate’s actual function.

5. **wonopcode** (main crate or CLI?)
   - Could be the main binary or aggregator crate.
   - Hosts high-level commands or orchestrations.

## Cross-Links
You can find each crate in the `crates/` directory:
```
crates/
  wonopcode/
  wonopcode-acp/
  wonopcode-auth/
  wonopcode-core/
  wonopcode-lsp/
```

For additional details:
• [Security Model](security-model.md) – explains permission checks in each crate.
• [Tools API](../reference/tools-api.md) – references how Tools may interact with different crates.
• [Configuration Schema](../reference/config-schema.md) – shows how crates can read config.

## Placeholders / Future Expansion
• Some crates may still have incomplete docs or placeholders (e.g., `wonopcode-acp`).
• Feel free to 
  - open issues or
  - create PRs to add more details to this document or the crate-level READMEs.

_End of Architecture Overview_
