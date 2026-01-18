//! LSP server configuration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Configuration for a language server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspConfig {
    /// Language identifier (e.g., "rust", "typescript").
    pub language: String,

    /// File extensions handled by this server.
    pub extensions: Vec<String>,

    /// Command to run the server.
    pub command: String,

    /// Arguments for the command.
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Root patterns to detect workspace root (e.g., ["Cargo.toml"]).
    #[serde(default)]
    pub root_patterns: Vec<String>,

    /// Whether the server is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl LspConfig {
    /// Create a new LSP configuration.
    pub fn new(
        language: impl Into<String>,
        command: impl Into<String>,
        extensions: Vec<impl Into<String>>,
    ) -> Self {
        Self {
            language: language.into(),
            extensions: extensions.into_iter().map(|e| e.into()).collect(),
            command: command.into(),
            args: Vec::new(),
            env: HashMap::new(),
            root_patterns: Vec::new(),
            enabled: true,
        }
    }

    /// Add command arguments.
    pub fn with_args(mut self, args: Vec<impl Into<String>>) -> Self {
        self.args = args.into_iter().map(|a| a.into()).collect();
        self
    }

    /// Add root patterns.
    pub fn with_root_patterns(mut self, patterns: Vec<impl Into<String>>) -> Self {
        self.root_patterns = patterns.into_iter().map(|p| p.into()).collect();
        self
    }

    /// Create configuration for Rust (rust-analyzer).
    pub fn rust() -> Self {
        Self::new("rust", "rust-analyzer", vec!["rs"]).with_root_patterns(vec!["Cargo.toml"])
    }

    /// Create configuration for TypeScript.
    pub fn typescript() -> Self {
        Self::new(
            "typescript",
            "typescript-language-server",
            vec!["ts", "tsx", "js", "jsx"],
        )
        .with_args(vec!["--stdio"])
        .with_root_patterns(vec!["package.json", "tsconfig.json"])
    }

    /// Create configuration for Python (pyright).
    pub fn python() -> Self {
        Self::new("python", "pyright-langserver", vec!["py"])
            .with_args(vec!["--stdio"])
            .with_root_patterns(vec!["pyproject.toml", "setup.py", "requirements.txt"])
    }

    /// Create configuration for Go (gopls).
    pub fn go() -> Self {
        Self::new("go", "gopls", vec!["go"]).with_root_patterns(vec!["go.mod"])
    }

    /// Create configuration for C/C++ (clangd).
    pub fn cpp() -> Self {
        Self::new(
            "cpp",
            "clangd",
            vec!["c", "cpp", "cc", "cxx", "h", "hpp", "hxx"],
        )
        .with_root_patterns(vec!["compile_commands.json", "CMakeLists.txt", "Makefile"])
    }

    /// Create configuration for Java (jdtls).
    pub fn java() -> Self {
        Self::new("java", "jdtls", vec!["java"]).with_root_patterns(vec![
            "pom.xml",
            "build.gradle",
            "settings.gradle",
        ])
    }

    /// Create configuration for C# (csharp-ls or omnisharp).
    pub fn csharp() -> Self {
        Self::new("csharp", "csharp-ls", vec!["cs"]).with_root_patterns(vec!["*.csproj", "*.sln"])
    }

    /// Create configuration for Ruby (solargraph).
    pub fn ruby() -> Self {
        Self::new("ruby", "solargraph", vec!["rb", "rake", "gemspec"])
            .with_args(vec!["stdio"])
            .with_root_patterns(vec!["Gemfile", "*.gemspec"])
    }

    /// Create configuration for PHP (intelephense).
    pub fn php() -> Self {
        Self::new("php", "intelephense", vec!["php"])
            .with_args(vec!["--stdio"])
            .with_root_patterns(vec!["composer.json"])
    }

    /// Create configuration for Swift (sourcekit-lsp).
    pub fn swift() -> Self {
        Self::new("swift", "sourcekit-lsp", vec!["swift"])
            .with_root_patterns(vec!["Package.swift", "*.xcodeproj"])
    }

    /// Create configuration for Kotlin (kotlin-language-server).
    pub fn kotlin() -> Self {
        Self::new("kotlin", "kotlin-language-server", vec!["kt", "kts"]).with_root_patterns(vec![
            "build.gradle.kts",
            "build.gradle",
            "pom.xml",
        ])
    }

    /// Create configuration for Scala (metals).
    pub fn scala() -> Self {
        Self::new("scala", "metals", vec!["scala", "sbt", "sc"])
            .with_root_patterns(vec!["build.sbt", "build.sc"])
    }

    /// Create configuration for Elixir (elixir-ls).
    pub fn elixir() -> Self {
        Self::new("elixir", "elixir-ls", vec!["ex", "exs"]).with_root_patterns(vec!["mix.exs"])
    }

    /// Create configuration for Erlang (erlang_ls).
    pub fn erlang() -> Self {
        Self::new("erlang", "erlang_ls", vec!["erl", "hrl"])
            .with_root_patterns(vec!["rebar.config", "erlang.mk"])
    }

    /// Create configuration for Haskell (haskell-language-server).
    pub fn haskell() -> Self {
        Self::new(
            "haskell",
            "haskell-language-server-wrapper",
            vec!["hs", "lhs"],
        )
        .with_args(vec!["--lsp"])
        .with_root_patterns(vec!["*.cabal", "stack.yaml", "cabal.project"])
    }

    /// Create configuration for OCaml (ocamllsp).
    pub fn ocaml() -> Self {
        Self::new("ocaml", "ocamllsp", vec!["ml", "mli"])
            .with_root_patterns(vec!["dune-project", "*.opam"])
    }

    /// Create configuration for Clojure (clojure-lsp).
    pub fn clojure() -> Self {
        Self::new("clojure", "clojure-lsp", vec!["clj", "cljs", "cljc", "edn"])
            .with_root_patterns(vec!["deps.edn", "project.clj", "shadow-cljs.edn"])
    }

    /// Create configuration for Lua (lua-language-server).
    pub fn lua() -> Self {
        Self::new("lua", "lua-language-server", vec!["lua"])
            .with_root_patterns(vec![".luarc.json", ".luacheckrc"])
    }

    /// Create configuration for Zig (zls).
    pub fn zig() -> Self {
        Self::new("zig", "zls", vec!["zig"]).with_root_patterns(vec!["build.zig", "zls.json"])
    }

    /// Create configuration for Vue (vue-language-server).
    pub fn vue() -> Self {
        Self::new("vue", "vue-language-server", vec!["vue"])
            .with_args(vec!["--stdio"])
            .with_root_patterns(vec!["package.json", "vite.config.ts", "vue.config.js"])
    }

    /// Create configuration for Svelte (svelte-language-server).
    pub fn svelte() -> Self {
        Self::new("svelte", "svelteserver", vec!["svelte"])
            .with_args(vec!["--stdio"])
            .with_root_patterns(vec!["svelte.config.js", "package.json"])
    }

    /// Create configuration for Dart (dart language-server).
    pub fn dart() -> Self {
        Self::new("dart", "dart", vec!["dart"])
            .with_args(vec!["language-server"])
            .with_root_patterns(vec!["pubspec.yaml"])
    }

    /// Create configuration for YAML (yaml-language-server).
    pub fn yaml() -> Self {
        Self::new("yaml", "yaml-language-server", vec!["yaml", "yml"]).with_args(vec!["--stdio"])
    }

    /// Create configuration for JSON (vscode-json-language-server).
    pub fn json() -> Self {
        Self::new("json", "vscode-json-language-server", vec!["json", "jsonc"])
            .with_args(vec!["--stdio"])
    }

    /// Create configuration for Bash (bash-language-server).
    pub fn bash() -> Self {
        Self::new("bash", "bash-language-server", vec!["sh", "bash", "zsh"])
            .with_args(vec!["start"])
    }

    /// Create configuration for Terraform (terraform-ls).
    pub fn terraform() -> Self {
        Self::new("terraform", "terraform-ls", vec!["tf", "tfvars"])
            .with_args(vec!["serve"])
            .with_root_patterns(vec!["*.tf", "terraform.tfstate"])
    }

    /// Create configuration for Docker (docker-langserver).
    pub fn docker() -> Self {
        Self::new("dockerfile", "docker-langserver", vec!["dockerfile"])
            .with_args(vec!["--stdio"])
            .with_root_patterns(vec!["Dockerfile", "docker-compose.yml"])
    }

    /// Create configuration for SQL (sqls).
    pub fn sql() -> Self {
        Self::new("sql", "sqls", vec!["sql"])
    }

    /// Create configuration for LaTeX (texlab).
    pub fn latex() -> Self {
        Self::new("latex", "texlab", vec!["tex", "bib", "sty", "cls"])
            .with_root_patterns(vec!["*.tex", "latexmkrc"])
    }

    /// Create configuration for Nix (nixd or nil).
    pub fn nix() -> Self {
        Self::new("nix", "nixd", vec!["nix"]).with_root_patterns(vec![
            "flake.nix",
            "default.nix",
            "shell.nix",
        ])
    }

    /// Create configuration for Gleam (gleam lsp).
    pub fn gleam() -> Self {
        Self::new("gleam", "gleam", vec!["gleam"])
            .with_args(vec!["lsp"])
            .with_root_patterns(vec!["gleam.toml"])
    }

    /// Create configuration for Deno (deno lsp).
    pub fn deno() -> Self {
        Self::new("deno", "deno", vec!["ts", "tsx", "js", "jsx"])
            .with_args(vec!["lsp"])
            .with_root_patterns(vec!["deno.json", "deno.jsonc"])
    }

    /// Create configuration for CSS/SCSS/LESS (vscode-css-language-server).
    pub fn css() -> Self {
        Self::new(
            "css",
            "vscode-css-language-server",
            vec!["css", "scss", "less"],
        )
        .with_args(vec!["--stdio"])
    }

    /// Create configuration for HTML (vscode-html-language-server).
    pub fn html() -> Self {
        Self::new("html", "vscode-html-language-server", vec!["html", "htm"])
            .with_args(vec!["--stdio"])
    }

    /// Create configuration for Markdown (marksman).
    pub fn markdown() -> Self {
        Self::new("markdown", "marksman", vec!["md", "markdown"])
    }

    /// Create configuration for Prisma (prisma-language-server).
    pub fn prisma() -> Self {
        Self::new("prisma", "prisma-language-server", vec!["prisma"])
            .with_args(vec!["--stdio"])
            .with_root_patterns(vec!["schema.prisma"])
    }

    /// Create configuration for GraphQL (graphql-lsp).
    pub fn graphql() -> Self {
        Self::new("graphql", "graphql-lsp", vec!["graphql", "gql"])
            .with_args(vec!["server", "-m", "stream"])
            .with_root_patterns(vec![".graphqlrc", ".graphqlrc.yml", "graphql.config.js"])
    }

    /// Create configuration for Tailwind CSS (tailwindcss-language-server).
    pub fn tailwindcss() -> Self {
        Self::new(
            "tailwindcss",
            "tailwindcss-language-server",
            vec!["html", "jsx", "tsx", "vue", "svelte"],
        )
        .with_args(vec!["--stdio"])
        .with_root_patterns(vec!["tailwind.config.js", "tailwind.config.ts"])
    }

    /// Check if this server handles the given file extension.
    pub fn handles_extension(&self, ext: &str) -> bool {
        self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
    }

    /// Find workspace root for a file using root patterns.
    pub fn find_workspace_root(&self, file_path: &std::path::Path) -> Option<PathBuf> {
        let mut current = file_path.parent()?;

        loop {
            for pattern in &self.root_patterns {
                if current.join(pattern).exists() {
                    return Some(current.to_path_buf());
                }
            }

            current = current.parent()?;
        }
    }
}

/// Default configurations for common languages.
pub fn default_configs() -> Vec<LspConfig> {
    vec![
        // Tier 1: Most common languages
        LspConfig::rust(),
        LspConfig::typescript(),
        LspConfig::python(),
        LspConfig::go(),
        LspConfig::cpp(),
        LspConfig::java(),
        LspConfig::csharp(),
        // Tier 2: Web development
        LspConfig::vue(),
        LspConfig::svelte(),
        LspConfig::html(),
        LspConfig::css(),
        LspConfig::tailwindcss(),
        LspConfig::graphql(),
        LspConfig::prisma(),
        // Tier 3: Scripting languages
        LspConfig::ruby(),
        LspConfig::php(),
        LspConfig::lua(),
        LspConfig::bash(),
        // Tier 4: Functional languages
        LspConfig::haskell(),
        LspConfig::ocaml(),
        LspConfig::clojure(),
        LspConfig::elixir(),
        LspConfig::erlang(),
        LspConfig::scala(),
        LspConfig::gleam(),
        // Tier 5: Systems languages
        LspConfig::swift(),
        LspConfig::kotlin(),
        LspConfig::zig(),
        LspConfig::dart(),
        // Tier 6: Configuration & markup
        LspConfig::yaml(),
        LspConfig::json(),
        LspConfig::terraform(),
        LspConfig::docker(),
        LspConfig::nix(),
        LspConfig::latex(),
        LspConfig::markdown(),
        LspConfig::sql(),
        // Note: Deno uses same extensions as TypeScript, disabled by default
        // to avoid conflicts. Users can enable it explicitly.
        // LspConfig::deno(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_config() {
        let config = LspConfig::rust();
        assert_eq!(config.language, "rust");
        assert_eq!(config.command, "rust-analyzer");
        assert!(config.handles_extension("rs"));
        assert!(!config.handles_extension("py"));
    }

    #[test]
    fn test_typescript_config() {
        let config = LspConfig::typescript();
        assert!(config.handles_extension("ts"));
        assert!(config.handles_extension("tsx"));
        assert!(config.handles_extension("js"));
    }

    #[test]
    fn test_custom_config() {
        let config = LspConfig::new("custom", "custom-lsp", vec!["cst"]).with_args(vec!["--stdio"]);

        assert_eq!(config.language, "custom");
        assert_eq!(config.args, vec!["--stdio"]);
    }
}
