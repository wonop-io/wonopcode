# Event System Improvements Plan

> **Status**: Implemented - Phases 1, 2, and 3 completed.

## Current Architecture Analysis

### How Events Flow Today

1. **Local TUI Mode (Direct Channels)**
   - Runner sends `AppUpdate` via `mpsc::UnboundedSender<AppUpdate>`
   - TUI receives updates through `mpsc::UnboundedReceiver<AppUpdate>`
   - Updates are immediate but single-client only

2. **Remote TUI Mode (HTTP/SSE)**
   - Server has an event bus (`wonopcode_core::Bus`) using `tokio::sync::broadcast`
   - SSE endpoint (`/events`) subscribes to `bus.subscribe_all()` and streams events
   - TUI's `RemoteBackend` connects to SSE and parses events into `AppUpdate`

3. **Todo Update Flow**
   - `todowrite` tool stores todos in `SharedFileTodoStore` (temp file)
   - After tool execution completes, runner reads todos and sends `AppUpdate::TodosUpdated`
   - This happens at `runner.rs:2661-2684` inside the tool execution future

### Identified Problems

#### 1. TODO Updates Not Appearing Immediately
The TODO sync happens **after** each tool completes within the parallel execution:
```rust
// Inside tool future at runner.rs:2661-2684
if base_tool_name == "todowrite" && success {
    let todos = todo::get_todos(todo_store.as_ref(), &cwd);
    // ... convert to TodoUpdate ...
    let _ = update_tx.send(AppUpdate::TodosUpdated(todo_updates));
}
```

**Issue**: The update is sent from inside an async task. If the channel is backed up or the TUI is busy rendering, the update may be delayed. Also, `update_tx.send()` uses `let _ =` which silently ignores errors.

#### 2. No State Recovery for Late-Joining Clients
When a client connects after the LLM has started:
- They subscribe to SSE and only receive **future** events
- Current state (todos, modified files, active tools) is lost
- The `/state` endpoint exists but doesn't include runtime state like todos

#### 3. SSE Reliability Issues
- `broadcast::channel` can **lag** - if receiver is slow, events are dropped
- SSE already logs this: `"SSE stream lagged by {} events"` (sse.rs:24)
- No replay mechanism for missed events
- One-way communication limits client control

#### 4. Dual Storage Mechanisms
- Local mode: `SharedFileTodoStore` (temp file shared via env var)
- Server mode: `SharedTodoStore` (`Arc<RwLock<Vec<TodoItem>>>`)
- These don't sync - server's `session_todo` route has fallback logic

---

## Proposed Solutions

### Solution 1: Immediate Event Publishing (Quick Fix)

**Goal**: Make TODO updates appear immediately without waiting for tool completion.

**Changes**:
1. Publish `TodoUpdated` event on the bus **immediately** when `todowrite` executes
2. Have the TUI subscribe to this bus event

**Implementation**:
```rust
// In TodoWriteTool::execute() after saving to store
if let Some(bus) = ctx.bus.as_ref() {
    bus.publish(TodoUpdated {
        session_id: ctx.session_id.clone(),
        items: items.iter().map(|t| /* convert */).collect(),
    }).await;
}
```

**Pros**: Simple, uses existing infrastructure
**Cons**: Requires passing `Bus` through `ToolContext`, doesn't solve multi-client state recovery

---

### Solution 2: State Snapshots + Event Stream (Recommended)

**Goal**: Support late-joining clients with full state recovery and reliable event delivery.

#### 2.1 Enhanced State Endpoint

Create a comprehensive `/api/v1/state` endpoint that returns ALL runtime state:

```rust
#[derive(Serialize)]
struct FullState {
    // Session info
    session_id: String,
    status: SessionStatus,
    
    // Runtime state
    todos: Vec<TodoInfo>,
    modified_files: Vec<ModifiedFileInfo>,
    active_tools: Vec<ActiveToolInfo>,
    
    // Connection state
    lsp_servers: Vec<LspInfo>,
    mcp_servers: Vec<McpInfo>,
    sandbox: Option<SandboxStatus>,
    
    // Token usage
    token_usage: Option<TokenUsage>,
    
    // Event sequence number (for syncing)
    last_event_seq: u64,
}
```

#### 2.2 Sequenced Events

Add sequence numbers to events for reliable delivery:

```rust
#[derive(Clone, Serialize)]
struct SequencedEvent {
    seq: u64,
    timestamp: i64,
    event: BusEvent,
}
```

Store recent events in a ring buffer (e.g., last 1000 events) so clients can request replay.

#### 2.3 Event Replay Endpoint

```
GET /api/v1/events/replay?from_seq={seq}&limit={limit}
```

Returns events from the given sequence number, allowing clients to catch up.

---

### Solution 3: WebSocket Transport (Best for Real-time)

**Goal**: Bidirectional communication with better connection management.

#### 3.1 WebSocket Protocol

```rust
// Client -> Server
#[derive(Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    Subscribe { events: Vec<String> },
    Unsubscribe { events: Vec<String> },
    RequestState,
    Ping,
}

// Server -> Client
#[derive(Serialize)]
#[serde(tag = "type")]
enum ServerMessage {
    Event { seq: u64, event: BusEvent },
    State { state: FullState },
    Pong,
    Error { message: String },
}
```

#### 3.2 Connection Lifecycle

1. Client connects to `ws://server/api/v1/ws`
2. Server sends initial `State` message with full state + `last_event_seq`
3. Server streams `Event` messages as they occur
4. Client can request state refresh anytime
5. Heartbeat with `Ping`/`Pong` for connection health

#### 3.3 Implementation with Axum

```rust
use axum::extract::ws::{WebSocket, WebSocketUpgrade};

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    // 1. Send initial state
    let full_state = build_full_state(&state).await;
    let _ = socket.send(ServerMessage::State { state: full_state }.to_ws_message()).await;
    
    // 2. Subscribe to events
    let mut event_rx = state.bus.subscribe_all();
    
    // 3. Event loop
    loop {
        tokio::select! {
            // Handle incoming messages
            Some(msg) = socket.recv() => {
                match parse_client_message(msg) {
                    ClientMessage::RequestState => {
                        let state = build_full_state(&state).await;
                        let _ = socket.send(ServerMessage::State { state }.to_ws_message()).await;
                    }
                    ClientMessage::Ping => {
                        let _ = socket.send(ServerMessage::Pong.to_ws_message()).await;
                    }
                    // ...
                }
            }
            // Forward events
            Ok(event) = event_rx.recv() => {
                let _ = socket.send(ServerMessage::Event { seq, event }.to_ws_message()).await;
            }
        }
    }
}
```

---

## Recommended Implementation Plan

### Phase 1: Quick Fixes (Immediate)

1. **Fix silent channel errors**
   - Change `let _ = update_tx.send(...)` to proper error logging
   - Location: `runner.rs` lines 2652, 2683, etc.

2. **Add Bus to ToolContext**
   - Pass `Bus` through `ToolContext` 
   - Publish `TodoUpdated` event immediately in `TodoWriteTool::execute()`

3. **Unify todo storage**
   - Use single `SharedTodoStore` in server mode
   - Remove file-based fallback complexity

### Phase 2: State Recovery (Short-term)

1. **Enhance `/state` endpoint**
   - Include todos, modified files, active tools
   - Add `last_event_seq` for sync coordination

2. **Add event sequence numbers**
   - Modify `Bus` to track sequence
   - Store recent events for replay

3. **Create `/events/replay` endpoint**
   - Allow clients to catch up on missed events

### Phase 3: WebSocket Transport (Medium-term)

1. **Add axum WebSocket support**
   ```toml
   # wonopcode-server/Cargo.toml
   axum = { version = "0.7", features = ["ws"] }
   ```

2. **Implement WebSocket handler**
   - Initial state on connect
   - Event streaming
   - Client message handling

3. **Update TUI backend**
   - Add `WebSocketBackend` option
   - Prefer WebSocket when available, fallback to SSE

### Phase 4: Advanced Features (Long-term)

1. **Event filtering**
   - Allow clients to subscribe to specific event types
   - Reduce bandwidth for clients that don't need all events

2. **Presence tracking**
   - Know which clients are connected
   - Broadcast presence to other clients

3. **Collaborative editing support**
   - Multiple clients can see same session
   - Conflict resolution for actions

---

## File Changes Summary

### Phase 1 Files
- `crates/wonopcode/src/runner.rs` - Better error handling, bus publishing
- `crates/wonopcode-tools/src/lib.rs` - Add `Bus` to `ToolContext`
- `crates/wonopcode-tools/src/todo.rs` - Publish events on write

### Phase 2 Files
- `crates/wonopcode-core/src/bus.rs` - Add sequence numbers, event storage
- `crates/wonopcode-server/src/routes.rs` - Enhanced state, replay endpoints
- `crates/wonopcode-server/src/state.rs` - Runtime state tracking

### Phase 3 Files
- `crates/wonopcode-server/src/ws.rs` - New WebSocket module
- `crates/wonopcode-server/src/lib.rs` - Export ws module
- `crates/wonopcode-server/src/routes.rs` - Add ws route
- `crates/wonopcode-tui/src/backend.rs` - Add WebSocketBackend
- `crates/wonopcode-protocol/src/lib.rs` - WebSocket message types

---

## Testing Plan

1. **Unit tests**
   - Event sequencing
   - State snapshot accuracy
   - WebSocket message parsing

2. **Integration tests**
   - Client connects mid-session, receives full state
   - Multiple clients receive same events
   - Event replay after reconnection

3. **Manual testing**
   - Start LLM task, connect second TUI
   - Kill connection, reconnect, verify state
   - TODO updates appear immediately

---

## Migration Notes

- SSE endpoint remains for backwards compatibility
- WebSocket is additive, not replacing SSE
- Phase 1 changes are non-breaking
- Protocol additions are backwards compatible (new optional fields)
