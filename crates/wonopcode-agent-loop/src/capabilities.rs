//! Capabilities declaration for agent loops.

use std::collections::HashMap;

/// Describes the capabilities of an agent loop implementation.
///
/// Used to inform the system what features the loop supports,
/// enabling appropriate UI and behavior adjustments.
#[derive(Debug, Clone, Default)]
pub struct LoopCapabilities {
    /// Supports streaming text output.
    ///
    /// If true, the loop sends streaming text updates.
    pub streaming: bool,

    /// Supports extended thinking/reasoning.
    ///
    /// If true, the loop can handle thinking tokens from providers
    /// that support them (e.g., Claude with extended thinking).
    pub extended_thinking: bool,

    /// Supports parallel tool execution.
    ///
    /// If true, the loop executes multiple tool calls concurrently.
    pub parallel_tools: bool,

    /// Supports tool observation without execution.
    ///
    /// If true, the loop can show tool calls without executing them.
    pub tool_observation: bool,

    /// Maximum context length in tokens.
    ///
    /// 0 means unlimited (loop handles compaction internally).
    pub max_context: u32,

    /// Supports automatic message compaction.
    ///
    /// If true, the loop compacts messages when approaching limits.
    pub auto_compaction: bool,

    /// Custom capabilities for extensions.
    ///
    /// Implementations can add custom capability flags here.
    pub custom: HashMap<String, bool>,
}

impl LoopCapabilities {
    /// Create capabilities for a standard loop.
    pub fn standard() -> Self {
        Self {
            streaming: true,
            extended_thinking: true,
            parallel_tools: true,
            tool_observation: false,
            max_context: 0,
            auto_compaction: true,
            custom: HashMap::new(),
        }
    }

    /// Create minimal capabilities (for simple loops).
    pub fn minimal() -> Self {
        Self {
            streaming: false,
            extended_thinking: false,
            parallel_tools: false,
            tool_observation: false,
            max_context: 0,
            auto_compaction: false,
            custom: HashMap::new(),
        }
    }

    /// Check if a custom capability is enabled.
    pub fn has_capability(&self, name: &str) -> bool {
        self.custom.get(name).copied().unwrap_or(false)
    }

    /// Set a custom capability.
    pub fn set_capability(&mut self, name: impl Into<String>, enabled: bool) {
        self.custom.insert(name.into(), enabled);
    }

    /// Builder pattern: enable streaming.
    pub fn with_streaming(mut self, enabled: bool) -> Self {
        self.streaming = enabled;
        self
    }

    /// Builder pattern: enable parallel tools.
    pub fn with_parallel_tools(mut self, enabled: bool) -> Self {
        self.parallel_tools = enabled;
        self
    }

    /// Builder pattern: enable auto compaction.
    pub fn with_auto_compaction(mut self, enabled: bool) -> Self {
        self.auto_compaction = enabled;
        self
    }

    /// Builder pattern: set max context.
    pub fn with_max_context(mut self, max_tokens: u32) -> Self {
        self.max_context = max_tokens;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_capabilities() {
        let caps = LoopCapabilities::standard();
        assert!(caps.streaming);
        assert!(caps.extended_thinking);
        assert!(caps.parallel_tools);
        assert!(caps.auto_compaction);
        assert!(!caps.tool_observation);
        assert_eq!(caps.max_context, 0);
    }

    #[test]
    fn test_minimal_capabilities() {
        let caps = LoopCapabilities::minimal();
        assert!(!caps.streaming);
        assert!(!caps.extended_thinking);
        assert!(!caps.parallel_tools);
        assert!(!caps.auto_compaction);
    }

    #[test]
    fn test_custom_capability() {
        let mut caps = LoopCapabilities::default();
        assert!(!caps.has_capability("wasm_orchestration"));

        caps.set_capability("wasm_orchestration", true);
        assert!(caps.has_capability("wasm_orchestration"));
    }

    #[test]
    fn test_builder_pattern() {
        let caps = LoopCapabilities::minimal()
            .with_streaming(true)
            .with_parallel_tools(true)
            .with_max_context(200_000);

        assert!(caps.streaming);
        assert!(caps.parallel_tools);
        assert!(!caps.extended_thinking);
        assert_eq!(caps.max_context, 200_000);
    }
}
