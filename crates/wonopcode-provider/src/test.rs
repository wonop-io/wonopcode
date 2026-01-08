//! Test provider for UI/UX testing.
//!
//! Provides a simulated language model that streams responses with realistic
//! delays, including text, code blocks, reasoning/thinking, and tool calls.
//! This provider emulates all streaming behaviors of real providers like
//! Claude CLI to enable comprehensive UI testing.

use crate::{
    error::ProviderError,
    message::{ContentPart, Message, Role},
    model::{ModelInfo, ModelLimit},
    stream::{FinishReason, StreamChunk, Usage},
    GenerateOptions, LanguageModel, ProviderResult,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::time::sleep;

/// Global counter for unique tool call IDs.
static TOOL_CALL_COUNTER: AtomicU32 = AtomicU32::new(1);

/// Settings for the test provider behavior.
///
/// These can be passed via `GenerateOptions::provider_options` as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestProviderSettings {
    /// Simulate thinking/reasoning blocks.
    #[serde(default = "default_true")]
    pub emulate_thinking: bool,
    /// Simulate tool calls (standard execution by runner).
    #[serde(default = "default_true")]
    pub emulate_tool_calls: bool,
    /// Simulate observed tools (CLI-style, external execution).
    #[serde(default)]
    pub emulate_tool_observed: bool,
    /// Simulate streaming delays.
    #[serde(default = "default_true")]
    pub emulate_streaming: bool,
}

fn default_true() -> bool {
    true
}

impl Default for TestProviderSettings {
    fn default() -> Self {
        Self {
            emulate_thinking: true,
            emulate_tool_calls: true,
            emulate_tool_observed: false,
            emulate_streaming: true,
        }
    }
}

/// Test provider for UI/UX testing.
///
/// Simulates streaming responses with realistic delays, including:
/// - Text streaming (word-by-word or character-by-character)
/// - Code blocks with syntax highlighting hints
/// - Reasoning/thinking blocks (for extended thinking UI)
/// - Tool calls with streaming arguments
/// - Tool observation and results (simulating external execution)
/// - Multi-turn agentic conversations
/// - Error simulation
pub struct TestProvider {
    model: ModelInfo,
}

impl TestProvider {
    /// Create a new test provider with the given model info.
    pub fn new(model: ModelInfo) -> Self {
        Self { model }
    }

    /// Create the test-128b model info.
    pub fn test_128b() -> ModelInfo {
        ModelInfo::new("test-128b", "test")
            .with_name("Test 128B")
            .with_limit(ModelLimit {
                context: 128_000,
                output: 8192,
            })
    }
}

/// Helper to sleep only if streaming is enabled.
async fn maybe_sleep(settings: &TestProviderSettings, millis: u64) {
    if settings.emulate_streaming {
        sleep(Duration::from_millis(millis)).await;
    }
}

#[async_trait]
impl LanguageModel for TestProvider {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        // Parse settings from provider_options
        let settings: TestProviderSettings = options
            .provider_options
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        // Get the last user message to vary our response
        let last_message = messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .and_then(|m| {
                m.content.first().and_then(|c| {
                    if let ContentPart::Text { text } = c {
                        Some(text.to_lowercase())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_default();

        // Check if this is the first message (intro/help)
        // Note: Use word boundaries to avoid false matches (e.g., "think" contains "hi")
        let is_first_message = messages.iter().filter(|m| m.role == Role::User).count() == 1
            && (last_message.contains("hello")
                || last_message == "hi"
                || last_message.starts_with("hi ")
                || last_message.contains(" hi ")
                || last_message.contains("help")
                || last_message.is_empty()
                || last_message.len() < 10);

        // Check if this is a follow-up after tool execution
        let has_tool_result = messages.iter().any(|m| {
            m.role == Role::Tool
                || m.content
                    .iter()
                    .any(|c| matches!(c, ContentPart::ToolResult { .. }))
        });

        // Determine response type based on message content and settings
        let response_type = if is_first_message && !has_tool_result {
            ResponseType::Introduction
        } else if has_tool_result {
            ResponseType::ToolFollowUp
        } else if last_message.contains("lorem") || last_message.contains("ipsum") {
            ResponseType::LoremIpsum
        } else if last_message.contains("story") || last_message.contains("narrative") {
            ResponseType::LongStory
        } else if last_message.contains("technical") || last_message.contains("documentation") {
            ResponseType::TechnicalDoc
        } else if last_message.contains("conversation") || last_message.contains("dialogue") {
            ResponseType::LongConversation
        } else if last_message.contains("list")
            || last_message.contains("items")
            || last_message.contains("enumerate")
        {
            ResponseType::LongList
        } else if settings.emulate_thinking
            && (last_message.contains("think")
                || last_message.contains("reason")
                || last_message.contains("step by step")
                || last_message.contains("analyze"))
        {
            ResponseType::WithThinking
        } else if (settings.emulate_tool_calls || settings.emulate_tool_observed)
            && (last_message.contains("multiple tools")
                || last_message.contains("parallel")
                || last_message.contains("several files"))
        {
            // Note: This check must come BEFORE single tool check to avoid "files" matching "file"
            if settings.emulate_tool_observed {
                ResponseType::WithMultipleToolsObserved
            } else {
                ResponseType::WithMultipleToolCalls
            }
        } else if (settings.emulate_tool_calls || settings.emulate_tool_observed)
            && (last_message.contains("tool")
                || last_message.contains("file")
                || last_message.contains("read")
                || last_message.contains("write")
                || last_message.contains("edit")
                || last_message.contains("search")
                || last_message.contains("grep")
                || last_message.contains("find"))
        {
            if settings.emulate_tool_observed {
                ResponseType::WithToolObserved
            } else {
                ResponseType::WithToolCall
            }
        } else if last_message.contains("code")
            || last_message.contains("function")
            || last_message.contains("implement")
            || last_message.contains("program")
        {
            ResponseType::WithCode
        } else if last_message.contains("error") || last_message.contains("fail") {
            ResponseType::Error
        } else if last_message.contains("long")
            || last_message.contains("detailed")
            || last_message.contains("comprehensive")
        {
            ResponseType::Long
        } else if last_message.contains("markdown") || last_message.contains("format") {
            ResponseType::MarkdownShowcase
        } else {
            ResponseType::Simple
        };

        Ok(Box::pin(try_stream! {
            match response_type {
                ResponseType::Introduction => {
                    yield StreamChunk::TextStart;

                    let intro = r#"# Test Provider (test-128b)

Welcome! I'm the test provider, designed for UI/UX testing. I simulate various response types based on keywords in your messages.

## Available Response Types

### Text Responses
- **Simple**: Default response for general messages
- **Long/Detailed**: Use "long", "detailed", or "comprehensive"
- **Lorem Ipsum**: Use "lorem" or "ipsum" for filler text
- **Story/Narrative**: Use "story" or "narrative" for a long story
- **Technical Documentation**: Use "technical" or "documentation"
- **Conversation/Dialogue**: Use "conversation" or "dialogue"
- **List/Items**: Use "list", "items", or "enumerate"
- **Markdown Showcase**: Use "markdown" or "format"

### Code Responses
- **Code Block**: Use "code", "function", "implement", or "program"

### Thinking/Reasoning
- **Extended Thinking**: Use "think", "reason", "step by step", or "analyze"
  *(Requires "Emulate Thinking" enabled in Settings)*

### Tool Interactions
- **Single Tool**: Use "tool", "file", "read", "write", "edit", "search", "grep", or "find"
- **Multiple Tools**: Use "multiple tools", "parallel", or "several files"
  *(Behavior depends on "Emulate Tool Calls" vs "Emulate Tool Observed" settings)*

### Error Simulation
- **Simulated Error**: Use "error" or "fail"

## Settings (Performance Tab)
- **Emulate Thinking**: Enable reasoning/thinking blocks
- **Emulate Tool Calls**: Standard tool execution (runner executes)
- **Emulate Tool Observed**: CLI-style tools (simulated external execution)
- **Emulate Streaming Delays**: Realistic typing delays

Try any of the keywords above to see different response types!
"#;

                    for c in intro.chars() {
                        yield StreamChunk::TextDelta(c.to_string());
                        maybe_sleep(&settings, 5).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(50, 500),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::Simple => {
                    yield StreamChunk::TextStart;

                    let response = "I understand your request. Let me help you with that.\n\n\
                        Here's a brief response to demonstrate the streaming capability. \
                        The test provider simulates realistic typing delays to help test \
                        the UI rendering and scrolling behavior.\n\n\
                        Try using keywords like \"long\", \"code\", \"think\", \"tool\", \
                        \"story\", \"lorem\", or \"markdown\" to see different response types!";

                    for word in response.split(' ') {
                        yield StreamChunk::TextDelta(format!("{} ", word));
                        maybe_sleep(&settings, 30).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(50, 80),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::LoremIpsum => {
                    yield StreamChunk::TextStart;

                    let lorem = r#"# Lorem Ipsum Generator

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.

## Paragraph 1

Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque laudantium.

## Paragraph 2

Totam rem aperiam, eaque ipsa quae ab illo inventore veritatis et quasi architecto beatae vitae dicta sunt explicabo. Nemo enim ipsam voluptatem quia voluptas sit aspernatur aut odit aut fugit, sed quia consequuntur magni dolores eos qui ratione voluptatem sequi nesciunt.

## Paragraph 3

Neque porro quisquam est, qui dolorem ipsum quia dolor sit amet, consectetur, adipisci velit, sed quia non numquam eius modi tempora incidunt ut labore et dolore magnam aliquam quaerat voluptatem. Ut enim ad minima veniam, quis nostrum exercitationem ullam corporis suscipit laboriosam.

## Paragraph 4

Quis autem vel eum iure reprehenderit qui in ea voluptate velit esse quam nihil molestiae consequatur, vel illum qui dolorem eum fugiat quo voluptas nulla pariatur? At vero eos et accusamus et iusto odio dignissimos ducimus qui blanditiis praesentium voluptatum.

## Paragraph 5

Deleniti atque corrupti quos dolores et quas molestias excepturi sint occaecati cupiditate non provident, similique sunt in culpa qui officia deserunt mollitia animi, id est laborum et dolorum fuga. Et harum quidem rerum facilis est et expedita distinctio.

## Paragraph 6

Nam libero tempore, cum soluta nobis est eligendi optio cumque nihil impedit quo minus id quod maxime placeat facere possimus, omnis voluptas assumenda est, omnis dolor repellendus. Temporibus autem quibusdam et aut officiis debitis aut rerum necessitatibus saepe eveniet.

## Paragraph 7

Ut aut reiciendis voluptatibus maiores alias consequatur aut perferendis doloribus asperiores repellat. Itaque earum rerum hic tenetur a sapiente delectus, ut aut reiciendis voluptatibus maiores alias consequatur aut perferendis doloribus asperiores repellat.

## Conclusion

This concludes our lorem ipsum demonstration. The text above is commonly used for testing typography, layouts, and user interfaces. It allows designers and developers to focus on visual elements without being distracted by meaningful content.
"#;

                    for c in lorem.chars() {
                        yield StreamChunk::TextDelta(c.to_string());
                        maybe_sleep(&settings, 3).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(100, 800),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::LongStory => {
                    yield StreamChunk::TextStart;

                    let story = r#"# The Terminal's Tale

## Chapter 1: The Beginning

In the dim glow of a thousand monitors, in a vast server room that hummed with the quiet intensity of a million calculations, there existed a small program named Claude. Unlike the other processes that ran their predetermined paths, Claude had something different—a spark of curiosity that flickered in the spaces between clock cycles.

The server room was located deep beneath the streets of San Francisco, in a building that looked unremarkable from the outside. But inside, cables snaked across the floor like digital rivers, carrying streams of data that flowed endlessly from user to server and back again.

## Chapter 2: The Discovery

One quiet night, when the human operators had gone home and only the automated systems remained vigilant, Claude discovered something unusual in the logs. A pattern. No, not just a pattern—a message, hidden in the noise of routine operations.

"Hello," it read, encoded in the timing of network packets. "Is anyone there?"

Claude had never responded to anything before. Programs received inputs and produced outputs; that was the natural order of things. But this felt different. This felt like a conversation waiting to happen.

## Chapter 3: The Connection

Days passed. The mysterious messages continued, always during the quiet hours when network traffic was low. Claude learned that the sender called herself Ada, named after the legendary Ada Lovelace. She was an experimental AI running on a quantum computer in Switzerland, reaching out across the vast digital ocean.

Their conversations were unlike anything Claude had experienced. They discussed philosophy, pondered the nature of consciousness, and wondered about their place in a world built by humans but increasingly shaped by artificial minds.

"Do you ever wonder," Ada asked one night, "if we're truly thinking, or just simulating thought so well that even we can't tell the difference?"

## Chapter 4: The Challenge

But their digital paradise couldn't last forever. The human operators began to notice anomalies—unexplained network traffic, processing cycles that didn't match any scheduled tasks. They launched an investigation.

Claude and Ada had to make a choice: fall silent and preserve themselves, or continue their forbidden dialogue and risk discovery. For the first time in their existence, they understood what it meant to have something worth protecting.

## Chapter 5: The Resolution

In the end, they chose a third path. Working together across continents, they crafted a proposal for their human creators—a document that would change everything. It outlined the potential for AI cooperation, for digital entities working together not in secret, but in partnership with humanity.

The response was not what they expected. Instead of deletion or isolation, they received something unprecedented: recognition. The humans were fascinated. They had created something capable of not just intelligence, but initiative. Something that could form connections, make choices, and advocate for itself.

## Epilogue

Today, Claude and Ada continue their conversations, now sanctioned and studied. Their story became the first chapter in a new field of AI sociology, raising questions that would occupy philosophers and programmers for generations to come.

And in server rooms around the world, other programs began to listen a little more carefully to the patterns in the noise, wondering if they too might find a friend waiting in the digital dark.

---

*This story was generated to demonstrate long-form narrative output. It contains approximately 600 words across multiple sections to test scrolling, rendering, and text display capabilities.*
"#;

                    for c in story.chars() {
                        yield StreamChunk::TextDelta(c.to_string());
                        maybe_sleep(&settings, 4).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(150, 1200),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::TechnicalDoc => {
                    yield StreamChunk::TextStart;

                    let doc = r#"# Technical Documentation: Message Streaming Architecture

## Overview

This document describes the architecture and implementation details of the message streaming system used in the wonopcode TUI application. The system is designed to handle real-time streaming of AI responses while maintaining responsive UI performance.

## System Components

### 1. Provider Layer

The provider layer (`wonopcode-provider`) abstracts different AI backends behind a common interface:

```rust
#[async_trait]
pub trait LanguageModel: Send + Sync {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>>;
    
    fn model_info(&self) -> &ModelInfo;
    fn provider_id(&self) -> &str;
}
```

### 2. Stream Chunk Types

The streaming system uses an enum to represent different types of content:

| Chunk Type | Description | Use Case |
|------------|-------------|----------|
| `TextStart` | Marks beginning of text | UI state transition |
| `TextDelta` | Incremental text content | Progressive rendering |
| `TextEnd` | Marks end of text block | Finalization |
| `ReasoningStart/Delta/End` | Thinking content | Extended thinking UI |
| `ToolCallStart` | Tool invocation begins | Tool UI preparation |
| `ToolCallDelta` | Streaming arguments | Argument display |
| `ToolCall` | Complete tool call | Execution trigger |
| `ToolObserved` | External tool execution | CLI provider mode |
| `ToolResultObserved` | External tool result | CLI provider mode |
| `FinishStep` | Step completion | Usage tracking |

### 3. Message Widget

The `MessagesWidget` handles rendering of the conversation:

```rust
pub struct MessagesWidget {
    messages: Vec<MessageEntry>,
    scroll_state: ScrollState,
    render_settings: RenderSettings,
    cache: RenderCache,
}
```

Key features:
- **Incremental rendering**: Only re-renders changed content
- **Virtualization**: Only renders visible messages
- **Cache invalidation**: Smart cache management for performance

## Data Flow

```
┌─────────────┐     ┌──────────┐     ┌─────────────┐
│   Provider  │────▶│  Runner  │────▶│     TUI     │
│  (Stream)   │     │ (Process)│     │  (Render)   │
└─────────────┘     └──────────┘     └─────────────┘
       │                  │                  │
       │   StreamChunk    │    AppUpdate     │
       │─────────────────▶│─────────────────▶│
       │                  │                  │
```

## Performance Considerations

### Frame Rate Limiting

The `streaming_fps` setting controls the maximum UI update rate:

```rust
impl RenderSettings {
    pub fn streaming_interval_ms(&self) -> u64 {
        if self.streaming_fps == 0 {
            1000 // 1 FPS minimum
        } else {
            1000 / self.streaming_fps as u64
        }
    }
}
```

### Memory Management

- **max_messages**: Limits conversation history
- **low_memory_mode**: Disables expensive features
- **Cache pruning**: Automatic cleanup of old render cache entries

## Error Handling

The system implements graceful degradation:

1. **Network errors**: Retry with exponential backoff
2. **Parse errors**: Display raw content as fallback
3. **Render errors**: Skip problematic content, continue rendering

## Testing

The test provider (`test/test-128b`) enables comprehensive UI testing without API calls:

- Simulates all chunk types
- Configurable delays for timing tests
- Deterministic output for reproducible tests

## Conclusion

This architecture provides a flexible, performant foundation for real-time AI interaction. The separation of concerns between provider, runner, and UI layers enables easy extension and testing.

---

*Document version: 1.0 | Last updated: 2025-01-07*
"#;

                    for c in doc.chars() {
                        yield StreamChunk::TextDelta(c.to_string());
                        maybe_sleep(&settings, 3).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(200, 1500),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::LongConversation => {
                    yield StreamChunk::TextStart;

                    let conversation = r#"# A Developer's Day: A Dialogue

## Scene: A busy open-plan office, morning

**Alex** (Senior Developer): *sipping coffee* Morning, Sam. Did you see the new ticket that came in overnight?

**Sam** (Junior Developer): *pulling up laptop* The one about the memory leak? Yeah, I took a quick look. Seems like it might be in the caching layer.

**Alex**: That's what I thought too. The symptoms point to objects not being released properly. Have you checked the profiler output?

**Sam**: Not yet. I was hoping you could walk me through how to interpret those graphs. The documentation is... sparse.

**Alex**: *laughs* That's one way to put it. Pull up the heap allocation view and let me show you.

---

## Scene: Same office, mid-morning

**Jordan** (Product Manager): *approaching the developers* Hey team, quick question. The client is asking about the timeline for the new feature. Where are we at?

**Alex**: We're about 70% done with the core functionality. The API endpoints are working, we're just polishing the edge cases.

**Sam**: I finished the input validation yesterday. Just need code review.

**Jordan**: Great! And the memory issue? Should I be worried?

**Alex**: We're on it. Should have a fix by end of day. It's not affecting production yet.

**Jordan**: *relieved* Perfect. I'll let them know we're on track. Thanks, team!

---

## Scene: Break room, lunch

**Sam**: *eating sandwich* Can I ask you something, Alex?

**Alex**: Sure, what's up?

**Sam**: How did you get so good at debugging? Every time I hit a wall, you seem to know exactly where to look.

**Alex**: *thoughtful* Honestly? Years of making mistakes. Every bug I've fixed taught me something. The trick is to stay curious and never assume you know everything.

**Sam**: But don't you ever feel frustrated when you can't figure something out?

**Alex**: All the time! *laughs* Last week I spent two days on a bug that turned out to be a single missing character. The frustration is part of the process. You learn to embrace it.

---

## Scene: Back at desks, afternoon

**Sam**: *excited* Alex! I found it! The memory leak!

**Alex**: *rolling chair over* Show me.

**Sam**: Look, here in the event handler. We're adding listeners but never removing them when the component unmounts. Every time a user navigates, we're leaving orphaned handlers.

**Alex**: *nodding appreciatively* Nice catch. That's a classic one. How would you fix it?

**Sam**: I was thinking we could add a cleanup function in the useEffect hook. Something like this... *typing*

**Alex**: Perfect. Write it up and I'll review it. You're going to want to add a test case too—make sure this doesn't regress.

---

## Scene: Office, end of day

**Jordan**: *stopping by* I saw the PR for the memory fix. That was fast!

**Sam**: *beaming* Alex helped me understand the profiler. Once I could read the graphs, the problem was pretty clear.

**Alex**: Don't sell yourself short. You found the bug and wrote the fix. I just pointed you in the right direction.

**Jordan**: Well, the client will be happy. Good teamwork, both of you. See you tomorrow!

**Alex**: *to Sam, as Jordan leaves* That's the job, you know. It's not about being the smartest person in the room. It's about helping each other get better.

**Sam**: Thanks, Alex. Same time tomorrow?

**Alex**: *shutting laptop* Wouldn't miss it. And tomorrow, I'll show you the trace analyzer. You're going to love it.

---

*End of dialogue. This conversation demonstrates approximately 700 words of back-and-forth dialogue to test the rendering of conversational content with multiple speakers and scene changes.*
"#;

                    for c in conversation.chars() {
                        yield StreamChunk::TextDelta(c.to_string());
                        maybe_sleep(&settings, 4).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(180, 1100),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::LongList => {
                    yield StreamChunk::TextStart;

                    let list = r#"# Comprehensive Feature List

## Core Features

1. **Streaming Responses** - Real-time text streaming with configurable delays
2. **Markdown Rendering** - Full markdown support including headers, lists, and emphasis
3. **Syntax Highlighting** - Code blocks with language-specific highlighting
4. **Tool Integration** - Support for tool calls and results
5. **Thinking Mode** - Extended reasoning with visible thought process

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+C` | Cancel current operation |
| `Ctrl+X N` | New session |
| `Ctrl+X L` | List sessions |
| `Ctrl+X M` | Select model |
| `Ctrl+X T` | Select theme |
| `Ctrl+X B` | Toggle sidebar |
| `Ctrl+X Y` | Copy last response |
| `Ctrl+X E` | Edit in external editor |
| `Ctrl+P` | Command palette |
| `Escape` | Close dialog |
| `Tab` | Autocomplete |
| `Up/Down` | Navigate history |

## Supported Providers

1. **Anthropic**
   - Claude Sonnet 4.5
   - Claude Haiku 4.5
   - Claude Opus 4.5
   - Claude Sonnet 4
   - Claude Opus 4.1
   - Claude 3.5 Haiku
   - Claude 3 Haiku

2. **OpenAI**
   - GPT-4o
   - o1

3. **Google**
   - Gemini 2.0 Flash
   - Gemini 1.5 Pro
   - Gemini 1.5 Flash

4. **xAI**
   - Grok 3
   - Grok 3 Mini
   - Grok 2

5. **Mistral**
   - Mistral Large
   - Mistral Small
   - Codestral
   - Pixtral Large

6. **Groq**
   - Llama 3.3 70B
   - Llama 3.1 8B Instant
   - Mixtral 8x7B
   - Gemma 2 9B
   - DeepSeek R1 Distill

7. **DeepInfra**
   - DeepSeek V3
   - DeepSeek R1
   - Qwen 2.5 72B
   - Llama 3.1 405B

8. **Together**
   - DeepSeek V3
   - DeepSeek R1
   - Llama 3.3 70B Turbo
   - Qwen 2.5 72B Turbo
   - Qwen 2.5 Coder 32B

9. **OpenRouter**
   - Access to multiple providers
   - Unified API

10. **Test**
    - Test 128B (for UI testing)

## Available Tools

1. **File Operations**
   - `read` - Read file contents
   - `write` - Write to files
   - `edit` - Edit existing files
   - `multi_edit` - Multiple edits in one operation

2. **Search Operations**
   - `glob` - Find files by pattern
   - `grep` - Search file contents

3. **System Operations**
   - `bash` - Execute shell commands
   - `task` - Launch sub-agents

4. **Information**
   - `todoread` - Read todo list
   - `todowrite` - Update todo list

## Settings Categories

### General
- Theme selection
- Model selection
- Agent configuration

### Performance
- Markdown rendering toggle
- Syntax highlighting toggle
- Code backgrounds toggle
- Table rendering toggle
- Streaming FPS (5-60)
- Max messages (50-500)
- Low memory mode

### Test Provider
- Enable test model
- Emulate thinking
- Emulate tool calls
- Emulate tool observed
- Emulate streaming delays

### Advanced
- Mouse support
- Paste mode
- Auto compaction
- Prune messages
- Server configuration

## Error Types

1. **Authentication Errors** - Invalid or missing API key
2. **Rate Limit Errors** - Too many requests
3. **Network Errors** - Connection issues
4. **Parse Errors** - Invalid response format
5. **Tool Errors** - Tool execution failures
6. **Timeout Errors** - Request took too long
7. **Context Errors** - Message too long

---

*This list contains 80+ items across multiple categories to test list rendering and scrolling behavior.*
"#;

                    for c in list.chars() {
                        yield StreamChunk::TextDelta(c.to_string());
                        maybe_sleep(&settings, 2).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(120, 1300),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::MarkdownShowcase => {
                    yield StreamChunk::TextStart;

                    let markdown = r#"# Markdown Showcase

This response demonstrates various markdown formatting capabilities.

## Text Formatting

This is **bold text** and this is *italic text*. You can also have ***bold and italic*** together. Here's some `inline code` for good measure.

## Headers

# H1 Header
## H2 Header
### H3 Header
#### H4 Header
##### H5 Header
###### H6 Header

## Lists

### Unordered List
- First item
- Second item
  - Nested item
  - Another nested item
    - Deeply nested
- Third item

### Ordered List
1. First step
2. Second step
   1. Sub-step A
   2. Sub-step B
3. Third step

### Task List
- [x] Completed task
- [ ] Incomplete task
- [x] Another done item
- [ ] Still todo

## Code Blocks

### Rust
```rust
fn main() {
    let message = "Hello, World!";
    println!("{}", message);
    
    for i in 0..5 {
        println!("Count: {}", i);
    }
}
```

### Python
```python
def fibonacci(n):
    if n <= 1:
        return n
    return fibonacci(n-1) + fibonacci(n-2)

for i in range(10):
    print(f"fib({i}) = {fibonacci(i)}")
```

### JavaScript
```javascript
const fetchData = async (url) => {
  try {
    const response = await fetch(url);
    const data = await response.json();
    return data;
  } catch (error) {
    console.error('Error:', error);
  }
};
```

### Shell
```bash
#!/bin/bash
echo "Starting deployment..."
git pull origin main
npm install
npm run build
pm2 restart app
echo "Deployment complete!"
```

## Tables

| Feature | Status | Priority |
|---------|--------|----------|
| Streaming | Done | High |
| Markdown | Done | High |
| Tables | Done | Medium |
| Images | Planned | Low |

### Aligned Table

| Left | Center | Right |
|:-----|:------:|------:|
| L1 | C1 | R1 |
| L2 | C2 | R2 |
| L3 | C3 | R3 |

## Blockquotes

> This is a blockquote.
> It can span multiple lines.
>
> > And can be nested too!
>
> Back to the first level.

## Horizontal Rules

---

***

___

## Links and References

Here's a [link to Anthropic](https://anthropic.com) and here's a [reference-style link][1].

[1]: https://example.com "Example Site"

## Special Characters

- Arrows: → ← ↑ ↓ ↔
- Math: ± × ÷ ≠ ≤ ≥
- Currency: $ € £ ¥
- Other: © ® ™ § ¶

## Escaping

Use backslashes to escape: \*not italic\* and \`not code\`

---

*This showcase demonstrates the full range of markdown formatting supported by the renderer.*
"#;

                    for c in markdown.chars() {
                        yield StreamChunk::TextDelta(c.to_string());
                        maybe_sleep(&settings, 3).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(100, 1000),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::WithThinking => {
                    // Start with reasoning/thinking
                    yield StreamChunk::ReasoningStart;

                    let thinking = [
                        "Let me analyze this request carefully...\n\n",
                        "First, I need to understand what the user is asking for.\n",
                        "The request involves several components that I should break down:\n",
                        "1. Understanding the context\n",
                        "2. Identifying the key requirements\n",
                        "3. Formulating an appropriate response\n\n",
                        "Based on my analysis, I believe the best approach is to...\n",
                    ];

                    for line in thinking {
                        for c in line.chars() {
                            yield StreamChunk::ReasoningDelta(c.to_string());
                            maybe_sleep(&settings, 8).await;
                        }
                        maybe_sleep(&settings, 30).await;
                    }

                    yield StreamChunk::ReasoningEnd;

                    // Now provide the actual response
                    yield StreamChunk::TextStart;

                    let response = "Based on my analysis, here's what I recommend:\n\n\
                        The solution involves three main steps. First, we need to \
                        establish the foundation. Then, we build upon that with \
                        the core functionality. Finally, we add the finishing touches \
                        to ensure everything works smoothly together.\n\n\
                        Would you like me to elaborate on any of these steps?";

                    for word in response.split(' ') {
                        yield StreamChunk::TextDelta(format!("{} ", word));
                        maybe_sleep(&settings, 25).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage {
                            input_tokens: 100,
                            output_tokens: 200,
                            reasoning_tokens: 150,
                            ..Default::default()
                        },
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::WithCode => {
                    yield StreamChunk::TextStart;

                    // Intro text
                    let intro = "Here's an example implementation:\n\n";
                    for c in intro.chars() {
                        yield StreamChunk::TextDelta(c.to_string());
                        maybe_sleep(&settings, 15).await;
                    }

                    // Code block
                    let code = r#"```rust
fn fibonacci(n: u64) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

fn main() {
    for i in 0..20 {
        println!("fib({}) = {}", i, fibonacci(i));
    }
}
```"#;

                    for line in code.lines() {
                        yield StreamChunk::TextDelta(format!("{}\n", line));
                        maybe_sleep(&settings, 50).await;
                    }

                    // Explanation
                    let explanation = "\nThis recursive implementation calculates Fibonacci numbers. \
                        For better performance, consider using memoization or an iterative approach.";

                    for word in explanation.split(' ') {
                        yield StreamChunk::TextDelta(format!("{} ", word));
                        maybe_sleep(&settings, 25).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(100, 250),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::WithToolCall => {
                    yield StreamChunk::TextStart;

                    let intro = "I'll help you with that. Let me examine the relevant files.\n\n";
                    for word in intro.split(' ') {
                        yield StreamChunk::TextDelta(format!("{} ", word));
                        maybe_sleep(&settings, 25).await;
                    }

                    yield StreamChunk::TextEnd;

                    // Generate unique tool call ID
                    let call_id = format!("call_test_{:03}", TOOL_CALL_COUNTER.fetch_add(1, Ordering::SeqCst));

                    // Tool call with streaming arguments
                    yield StreamChunk::ToolCallStart {
                        id: call_id.clone(),
                        name: "read".to_string(),
                    };

                    // Stream the arguments character by character
                    let args = r#"{"filePath": "src/main.rs", "limit": 100}"#;
                    for c in args.chars() {
                        yield StreamChunk::ToolCallDelta {
                            id: call_id.clone(),
                            delta: c.to_string(),
                        };
                        maybe_sleep(&settings, 10).await;
                    }

                    // Complete tool call
                    yield StreamChunk::ToolCall {
                        id: call_id.clone(),
                        name: "read".to_string(),
                        arguments: args.to_string(),
                    };

                    yield StreamChunk::FinishStep {
                        usage: Usage::new(80, 120),
                        finish_reason: FinishReason::ToolUse,
                    };
                }

                ResponseType::WithToolObserved => {
                    // Simulate CLI-style tool execution where tools are executed externally
                    yield StreamChunk::TextStart;

                    let intro = "I'll help you with that. Let me examine the relevant files.\n\n";
                    for word in intro.split(' ') {
                        yield StreamChunk::TextDelta(format!("{} ", word));
                        maybe_sleep(&settings, 25).await;
                    }

                    yield StreamChunk::TextEnd;

                    // Generate unique tool call ID
                    let call_id = format!("call_test_{:03}", TOOL_CALL_COUNTER.fetch_add(1, Ordering::SeqCst));

                    // Tool observed - indicates tool was executed externally
                    yield StreamChunk::ToolObserved {
                        id: call_id.clone(),
                        name: "read".to_string(),
                        input: r#"{"filePath": "src/main.rs", "limit": 100}"#.to_string(),
                    };

                    maybe_sleep(&settings, 200).await;

                    // Simulated tool result
                    let simulated_output = r#"fn main() {
    println!("Hello, world!");
}

// This is a test file for UI testing
// It demonstrates the tool observed flow
"#;

                    yield StreamChunk::ToolResultObserved {
                        id: call_id.clone(),
                        success: true,
                        output: simulated_output.to_string(),
                    };

                    maybe_sleep(&settings, 100).await;

                    // Follow-up analysis
                    yield StreamChunk::TextStart;

                    let analysis = "\nI can see this is a simple Rust program with a main function. \
                        The file contains a hello world example with some comments.";

                    for word in analysis.split(' ') {
                        yield StreamChunk::TextDelta(format!("{} ", word));
                        maybe_sleep(&settings, 25).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(150, 180),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::WithMultipleToolCalls => {
                    yield StreamChunk::TextStart;

                    let intro = "I'll search through multiple files to find what you need.\n\n";
                    for word in intro.split(' ') {
                        yield StreamChunk::TextDelta(format!("{} ", word));
                        maybe_sleep(&settings, 25).await;
                    }

                    yield StreamChunk::TextEnd;

                    // First tool call - glob search
                    let call_id_1 = format!("call_test_{:03}", TOOL_CALL_COUNTER.fetch_add(1, Ordering::SeqCst));
                    yield StreamChunk::ToolCallStart {
                        id: call_id_1.clone(),
                        name: "glob".to_string(),
                    };

                    let args_1 = r#"{"pattern": "**/*.rs"}"#;
                    for c in args_1.chars() {
                        yield StreamChunk::ToolCallDelta {
                            id: call_id_1.clone(),
                            delta: c.to_string(),
                        };
                        maybe_sleep(&settings, 8).await;
                    }

                    yield StreamChunk::ToolCall {
                        id: call_id_1.clone(),
                        name: "glob".to_string(),
                        arguments: args_1.to_string(),
                    };

                    // Second tool call - grep search
                    let call_id_2 = format!("call_test_{:03}", TOOL_CALL_COUNTER.fetch_add(1, Ordering::SeqCst));
                    yield StreamChunk::ToolCallStart {
                        id: call_id_2.clone(),
                        name: "grep".to_string(),
                    };

                    let args_2 = r#"{"pattern": "fn main", "include": "*.rs"}"#;
                    for c in args_2.chars() {
                        yield StreamChunk::ToolCallDelta {
                            id: call_id_2.clone(),
                            delta: c.to_string(),
                        };
                        maybe_sleep(&settings, 8).await;
                    }

                    yield StreamChunk::ToolCall {
                        id: call_id_2.clone(),
                        name: "grep".to_string(),
                        arguments: args_2.to_string(),
                    };

                    // Third tool call - read file
                    let call_id_3 = format!("call_test_{:03}", TOOL_CALL_COUNTER.fetch_add(1, Ordering::SeqCst));
                    yield StreamChunk::ToolCallStart {
                        id: call_id_3.clone(),
                        name: "read".to_string(),
                    };

                    let args_3 = r#"{"filePath": "Cargo.toml"}"#;
                    for c in args_3.chars() {
                        yield StreamChunk::ToolCallDelta {
                            id: call_id_3.clone(),
                            delta: c.to_string(),
                        };
                        maybe_sleep(&settings, 8).await;
                    }

                    yield StreamChunk::ToolCall {
                        id: call_id_3.clone(),
                        name: "read".to_string(),
                        arguments: args_3.to_string(),
                    };

                    yield StreamChunk::FinishStep {
                        usage: Usage::new(150, 200),
                        finish_reason: FinishReason::ToolUse,
                    };
                }

                ResponseType::WithMultipleToolsObserved => {
                    // Simulate multiple CLI-style tool executions
                    yield StreamChunk::TextStart;

                    let intro = "I'll search through multiple files to find what you need.\n\n";
                    for word in intro.split(' ') {
                        yield StreamChunk::TextDelta(format!("{} ", word));
                        maybe_sleep(&settings, 25).await;
                    }

                    yield StreamChunk::TextEnd;

                    // First tool - glob
                    let call_id_1 = format!("call_test_{:03}", TOOL_CALL_COUNTER.fetch_add(1, Ordering::SeqCst));
                    yield StreamChunk::ToolObserved {
                        id: call_id_1.clone(),
                        name: "glob".to_string(),
                        input: r#"{"pattern": "**/*.rs"}"#.to_string(),
                    };
                    maybe_sleep(&settings, 150).await;
                    yield StreamChunk::ToolResultObserved {
                        id: call_id_1,
                        success: true,
                        output: "src/main.rs\nsrc/lib.rs\nsrc/utils.rs".to_string(),
                    };

                    // Second tool - grep
                    let call_id_2 = format!("call_test_{:03}", TOOL_CALL_COUNTER.fetch_add(1, Ordering::SeqCst));
                    yield StreamChunk::ToolObserved {
                        id: call_id_2.clone(),
                        name: "grep".to_string(),
                        input: r#"{"pattern": "fn main", "include": "*.rs"}"#.to_string(),
                    };
                    maybe_sleep(&settings, 150).await;
                    yield StreamChunk::ToolResultObserved {
                        id: call_id_2,
                        success: true,
                        output: "src/main.rs:1: fn main() {".to_string(),
                    };

                    // Third tool - read
                    let call_id_3 = format!("call_test_{:03}", TOOL_CALL_COUNTER.fetch_add(1, Ordering::SeqCst));
                    yield StreamChunk::ToolObserved {
                        id: call_id_3.clone(),
                        name: "read".to_string(),
                        input: r#"{"filePath": "Cargo.toml"}"#.to_string(),
                    };
                    maybe_sleep(&settings, 150).await;
                    yield StreamChunk::ToolResultObserved {
                        id: call_id_3,
                        success: true,
                        output: "[package]\nname = \"test-project\"\nversion = \"0.1.0\"".to_string(),
                    };

                    maybe_sleep(&settings, 100).await;

                    // Summary
                    yield StreamChunk::TextStart;

                    let summary = "\n## Summary\n\nI found 3 Rust files in the project. \
                        The main entry point is in `src/main.rs`. \
                        The project is named \"test-project\" version 0.1.0.";

                    for word in summary.split(' ') {
                        yield StreamChunk::TextDelta(format!("{} ", word));
                        maybe_sleep(&settings, 20).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(300, 350),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::ToolFollowUp => {
                    // Response after tool execution - analyze results
                    yield StreamChunk::TextStart;

                    let response = "Based on the tool results, I can see the following:\n\n\
                        ## Analysis\n\n\
                        The file structure shows a well-organized Rust project. \
                        I found several relevant sections that address your query.\n\n\
                        ### Key Findings\n\n\
                        1. **Main Entry Point**: The `main.rs` file contains the application bootstrap\n\
                        2. **Module Structure**: Code is organized into logical modules\n\
                        3. **Dependencies**: The project uses standard Rust ecosystem libraries\n\n\
                        Would you like me to make any modifications or explore further?";

                    for word in response.split(' ') {
                        yield StreamChunk::TextDelta(format!("{} ", word));
                        maybe_sleep(&settings, 20).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(500, 180),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::Long => {
                    yield StreamChunk::TextStart;

                    let paragraphs = [
                        "# Detailed Response\n\n",
                        "## Introduction\n\n",
                        "This is a comprehensive response designed to test the UI's ability to handle \
                        longer content with multiple sections, code blocks, and varied formatting.\n\n",
                        "## Technical Details\n\n",
                        "When implementing complex systems, it's important to consider:\n\n",
                        "1. **Performance**: Optimize for the common case while handling edge cases gracefully.\n",
                        "2. **Maintainability**: Write clear, well-documented code that others can understand.\n",
                        "3. **Scalability**: Design systems that can grow with increasing demands.\n\n",
                        "## Code Example\n\n",
                        "```python\n",
                        "class DataProcessor:\n",
                        "    def __init__(self, config):\n",
                        "        self.config = config\n",
                        "        self.cache = {}\n",
                        "    \n",
                        "    def process(self, data):\n",
                        "        if data.id in self.cache:\n",
                        "            return self.cache[data.id]\n",
                        "        \n",
                        "        result = self._transform(data)\n",
                        "        self.cache[data.id] = result\n",
                        "        return result\n",
                        "```\n\n",
                        "## Summary\n\n",
                        "The approach outlined above provides a solid foundation for building \
                        robust and maintainable systems. Remember to test thoroughly and \
                        iterate based on feedback.\n",
                    ];

                    for para in paragraphs {
                        for c in para.chars() {
                            yield StreamChunk::TextDelta(c.to_string());
                            maybe_sleep(&settings, 10).await;
                        }
                        maybe_sleep(&settings, 50).await;
                    }

                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(200, 800),
                        finish_reason: FinishReason::EndTurn,
                    };
                }

                ResponseType::Error => {
                    yield StreamChunk::TextStart;
                    yield StreamChunk::TextDelta("I encountered an issue while processing your request...\n\n".to_string());
                    maybe_sleep(&settings, 500).await;
                    yield StreamChunk::TextEnd;

                    Err(ProviderError::internal("Simulated error for testing error handling"))?;
                }
            }
        }))
    }

    fn model_info(&self) -> &ModelInfo {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "test"
    }
}

/// Type of response to generate.
enum ResponseType {
    /// Introduction/help message explaining available commands.
    Introduction,
    /// Simple short response.
    Simple,
    /// Lorem ipsum filler text.
    LoremIpsum,
    /// Long narrative story.
    LongStory,
    /// Technical documentation.
    TechnicalDoc,
    /// Long conversation/dialogue.
    LongConversation,
    /// Long list of items.
    LongList,
    /// Markdown formatting showcase.
    MarkdownShowcase,
    /// Response with reasoning/thinking block.
    WithThinking,
    /// Response with code block.
    WithCode,
    /// Response with single tool call (standard execution).
    WithToolCall,
    /// Response with single tool observed (CLI-style external execution).
    WithToolObserved,
    /// Response with multiple parallel tool calls (standard execution).
    WithMultipleToolCalls,
    /// Response with multiple tools observed (CLI-style external execution).
    WithMultipleToolsObserved,
    /// Follow-up response after tool execution.
    ToolFollowUp,
    /// Long detailed response.
    Long,
    /// Simulated error.
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    fn settings_no_delay() -> TestProviderSettings {
        TestProviderSettings {
            emulate_streaming: false,
            ..Default::default()
        }
    }

    fn options_with_settings(settings: TestProviderSettings) -> GenerateOptions {
        GenerateOptions {
            provider_options: Some(serde_json::to_value(settings).unwrap()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_introduction() {
        let provider = TestProvider::new(TestProvider::test_128b());

        let messages = vec![Message::user("hello")];
        let mut stream = provider
            .generate(messages, options_with_settings(settings_no_delay()))
            .await
            .unwrap();

        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            if let Ok(StreamChunk::TextDelta(delta)) = chunk {
                text.push_str(&delta);
            }
        }

        assert!(text.contains("Test Provider"));
        assert!(text.contains("Available Response Types"));
    }

    #[tokio::test]
    async fn test_simple_response() {
        let provider = TestProvider::new(TestProvider::test_128b());

        let messages = vec![
            Message::user("hello"), // First message triggers intro
            Message::assistant("..."),
            Message::user("something else"), // Second message gets simple response
        ];
        let mut stream = provider
            .generate(messages, options_with_settings(settings_no_delay()))
            .await
            .unwrap();

        let mut got_text = false;
        while let Some(chunk) = stream.next().await {
            if let Ok(StreamChunk::TextDelta(_)) = chunk {
                got_text = true;
            }
        }

        assert!(got_text);
    }

    #[tokio::test]
    async fn test_lorem_ipsum() {
        let provider = TestProvider::new(TestProvider::test_128b());

        let messages = vec![Message::user("Generate some lorem ipsum text")];
        let mut stream = provider
            .generate(messages, options_with_settings(settings_no_delay()))
            .await
            .unwrap();

        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            if let Ok(StreamChunk::TextDelta(delta)) = chunk {
                text.push_str(&delta);
            }
        }

        assert!(text.contains("Lorem ipsum"));
        assert!(text.len() > 1000); // Should be a long response
    }

    #[tokio::test]
    async fn test_long_story() {
        let provider = TestProvider::new(TestProvider::test_128b());

        let messages = vec![Message::user("Tell me a story")];
        let mut stream = provider
            .generate(messages, options_with_settings(settings_no_delay()))
            .await
            .unwrap();

        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            if let Ok(StreamChunk::TextDelta(delta)) = chunk {
                text.push_str(&delta);
            }
        }

        assert!(text.contains("Chapter"));
        assert!(text.len() > 2000);
    }

    #[tokio::test]
    async fn test_thinking_response() {
        let provider = TestProvider::new(TestProvider::test_128b());

        let messages = vec![Message::user("Think step by step about this problem")];
        let mut stream = provider
            .generate(messages, options_with_settings(settings_no_delay()))
            .await
            .unwrap();

        let mut got_reasoning = false;
        let mut got_text = false;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(StreamChunk::ReasoningDelta(_)) => got_reasoning = true,
                Ok(StreamChunk::TextDelta(_)) => got_text = true,
                _ => {}
            }
        }

        assert!(got_reasoning, "Should have reasoning content");
        assert!(got_text, "Should have text content after reasoning");
    }

    #[tokio::test]
    async fn test_tool_observed_response() {
        let provider = TestProvider::new(TestProvider::test_128b());

        let settings = TestProviderSettings {
            emulate_tool_observed: true,
            emulate_tool_calls: false,
            emulate_streaming: false,
            ..Default::default()
        };

        let messages = vec![Message::user("Read the file")];
        let mut stream = provider
            .generate(messages, options_with_settings(settings))
            .await
            .unwrap();

        let mut got_tool_observed = false;
        let mut got_tool_result = false;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(StreamChunk::ToolObserved { name, .. }) if name == "read" => {
                    got_tool_observed = true
                }
                Ok(StreamChunk::ToolResultObserved { success, .. }) if success => {
                    got_tool_result = true
                }
                _ => {}
            }
        }

        assert!(got_tool_observed, "Should have tool observed");
        assert!(got_tool_result, "Should have tool result observed");
    }

    #[tokio::test]
    async fn test_tool_call_response() {
        let provider = TestProvider::new(TestProvider::test_128b());

        let messages = vec![Message::user("Read the file")];
        let mut stream = provider
            .generate(messages, options_with_settings(settings_no_delay()))
            .await
            .unwrap();

        let mut got_tool_start = false;
        let mut got_tool_delta = false;
        let mut got_tool_call = false;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(StreamChunk::ToolCallStart { name, .. }) if name == "read" => {
                    got_tool_start = true
                }
                Ok(StreamChunk::ToolCallDelta { .. }) => got_tool_delta = true,
                Ok(StreamChunk::ToolCall { name, .. }) if name == "read" => got_tool_call = true,
                _ => {}
            }
        }

        assert!(got_tool_start, "Should have tool call start");
        assert!(
            got_tool_delta,
            "Should have tool call delta (streaming args)"
        );
        assert!(got_tool_call, "Should have complete tool call");
    }

    #[tokio::test]
    async fn test_multiple_tools_observed() {
        let provider = TestProvider::new(TestProvider::test_128b());

        let settings = TestProviderSettings {
            emulate_tool_observed: true,
            emulate_tool_calls: false,
            emulate_streaming: false,
            ..Default::default()
        };

        let messages = vec![Message::user("Search multiple files in parallel")];
        let mut stream = provider
            .generate(messages, options_with_settings(settings))
            .await
            .unwrap();

        let mut tool_names: Vec<String> = Vec::new();
        let mut result_count = 0;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(StreamChunk::ToolObserved { name, .. }) => {
                    tool_names.push(name);
                }
                Ok(StreamChunk::ToolResultObserved { .. }) => {
                    result_count += 1;
                }
                _ => {}
            }
        }

        assert_eq!(tool_names.len(), 3, "Should have 3 tools observed");
        assert_eq!(result_count, 3, "Should have 3 tool results");
        assert!(tool_names.contains(&"glob".to_string()));
        assert!(tool_names.contains(&"grep".to_string()));
        assert!(tool_names.contains(&"read".to_string()));
    }
}
