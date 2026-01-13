# Fix: Client-Server Reconnection Issues

## Summary

Two issues occurred when a client reconnects to a headless server:

1. **In-progress messages lost on reconnect** - If disconnecting while LLM is generating, the partial message was not shown when reconnecting
2. **Wrong scroll position** - After loading messages, user saw oldest messages instead of most recent

## Root Cause Analysis

### Issue 1: In-Progress Message Not in State

The server was tracking in-progress messages in **local variables** inside a spawned task:

```rust
tokio::spawn(async move {
    // These were LOCAL variables - not part of the shared state!
    let mut current_message_segments: Vec<MessageSegment> = Vec::new();
    let mut current_message_id: Option<String> = None;
    // ...
});
```

Messages were only added to `state.session.messages` on `Completed`. When a client called `GET /state`, they only got completed messages - the in-progress message was invisible.

### Issue 2: Wrong Scroll Position

In `set_messages()`, scroll was reset to 0 (top):
```rust
self.scroll = 0;  // User sees oldest messages first
```

## Implemented Fixes

### Fix 1: Added Streaming State to Protocol

Added new fields to `SessionState` in `wonopcode-protocol/src/state.rs`:

```rust
pub struct SessionState {
    // ... existing fields ...
    
    /// Whether the assistant is currently streaming a response.
    #[serde(default)]
    pub is_streaming: bool,

    /// The in-progress message being streamed (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming_message: Option<Message>,
}
```

### Fix 2: Server Tracks In-Progress Message in Shared State

Modified the update handler in `wonopcode/src/main.rs` to store streaming state in the shared state instead of local variables:

- **On `Started`**: Creates `streaming_message` in shared state, sets `is_streaming = true`
- **On `TextDelta`**: Appends to `streaming_message.content`
- **On `ToolStarted`**: Adds tool segment to `streaming_message.content`
- **On `ToolCompleted`**: Updates tool status in `streaming_message.content`
- **On `Completed`**: Moves `streaming_message` to `messages` list, sets `is_streaming = false`
- **On `Error`**: Clears streaming state

### Fix 3: Client Restores Streaming State on Connect

Modified `run_connect()` in `wonopcode/src/main.rs` to check for streaming state:

```rust
// If there's an in-progress streaming message, restore the streaming state
if session.is_streaming {
    if let Some(ref streaming_msg) = session.streaming_message {
        // Send Started to put TUI in streaming mode
        update_tx.send(AppUpdate::Started)?;
        
        // Send accumulated content as events
        for segment in &streaming_msg.content {
            match segment {
                MessageSegment::Text { text } => {
                    update_tx.send(AppUpdate::TextDelta(text.clone()))?;
                }
                MessageSegment::Tool { tool } => {
                    update_tx.send(AppUpdate::ToolStarted { ... })?;
                    if tool.status == "completed" || tool.status == "failed" {
                        update_tx.send(AppUpdate::ToolCompleted { ... })?;
                    }
                }
                // ...
            }
        }
    }
}
```

### Fix 4: Scroll to Bottom After Loading

Modified `set_messages()` in `wonopcode-tui/src/widgets/messages.rs`:

```rust
pub fn set_messages(&mut self, messages: Vec<DisplayMessage>) {
    // ... existing code ...
    
    // Scroll to bottom to show most recent messages
    self.scroll_to_bottom();
}
```

## Files Modified

| File | Changes |
|------|---------|
| `crates/wonopcode-protocol/src/state.rs` | Added `streaming_message` and `is_streaming` fields to `SessionState` |
| `crates/wonopcode/src/main.rs` | Updated server to track streaming in shared state; Updated client to restore streaming state |
| `crates/wonopcode-tui/src/widgets/messages.rs` | Changed `set_messages()` to scroll to bottom |

## Testing

- [ ] Connect to idle server - should show completed messages, scroll to bottom
- [ ] Connect while LLM is generating text - should show partial text, continue streaming
- [ ] Connect while LLM is running a tool - should show tool in progress
- [ ] Disconnect and reconnect during generation - should resume showing stream
- [ ] LLM completes while disconnected - should show completed message on reconnect
- [ ] LLM errors while disconnected - should show error state on reconnect
