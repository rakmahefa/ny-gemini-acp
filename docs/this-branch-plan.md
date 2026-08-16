# Branch Plan — feat/acp-semantic-events

## Objective

Introduce an ACP semantic event layer inside `gemini-acp-runtime` to decouple runtime state changes from ACP transport notifications.

The goal is to provide a reliable internal event model for:

- assistant streaming
- thinking streaming
- tool lifecycle
- permissions
- cancellation
- terminal results

This layer becomes the source of truth before projecting events into ACP protocol messages.

---

# Current Architecture

Workspace:

```
crates/
├── gemini-acp-config
├── gemini-acp-runtime
├── gemini-acp-agent
└── gemini-acp-encaps
```

The semantic event system belongs in `gemini-acp-runtime` because it represents runtime state, not transport concerns.

Current flow:

```
Gemini
  |
  v
Runtime
  |
  v
ACP transport
```

Target flow:

```
Gemini
  |
  v
Runtime
  |
  v
Semantic Event Layer
  |
  +----------------+
  |                |
ACP Adapter     Internal consumers
```

---

# Implementation Plan

## Phase 1 — Event Model

Create:

```
crates/gemini-acp-runtime/src/events/
```

Modules:

```
events/
├── mod.rs
├── event.rs
├── context.rs
├── stream.rs
└── tests.rs
```

Introduce:

```rust
AcpSemanticEvent
```

Initial events:

- TurnStarted
- AssistantStarted
- AssistantDelta
- AssistantCompleted
- ThinkingStarted
- ThinkingDelta
- ThinkingCompleted
- ToolCallRequested
- PermissionRequested
- ToolExecutionStarted
- ToolResultReceived
- TurnCancelled
- TurnCompleted

---

## Phase 2 — Event Context

Every event must carry identity information.

Required fields:

```rust
EventContext {
    session_id,
    turn_id,
    sequence,
}
```

Tool events additionally track:

```rust
ToolEventContext {
    tool_call_id,
}
```

Purpose:

- preserve ordering
- correlate partial streams
- guarantee tool result integrity

---

## Phase 3 — Runtime Event Bus

Add an internal event bus.

Responsibilities:

- publish semantic events
- allow subscribers
- keep runtime independent from ACP transport

Expected integration:

```
AppState
  |
  +-- EventBus
```

---

## Phase 4 — Tool Lifecycle Integration

Connect existing lifecycle states:

```
Pending
  |
Permission
  |
Executing
  |
Completed
```

with semantic events:

```
ToolCallRequested
PermissionRequested
ToolExecutionStarted
ToolResultReceived
```

Cancellation paths:

```
Executing -> TurnCancelled
Permission -> TurnCancelled
Pending -> TurnCancelled
```

---

## Phase 5 — ACP Projection

Create mapping layer:

```
Semantic Event
       |
       v
ACP Notification
```

The runtime should never directly construct transport messages.

---

# Testing Strategy

Required tests:

## Event ordering

Validate:

```
TurnStarted
AssistantDelta
ToolCallRequested
ToolResultReceived
TurnCompleted
```

## Cancellation

Validate:

```
Executing
   |
cancel
   |
TurnCancelled
```

## Tool integrity

Guarantee:

```
ToolCallRequested(id=A)
ToolResultReceived(id=A)
```

No unrelated tool result should be accepted.

---

# Commit Sequence

1. `feat(runtime): add ACP semantic event model`
2. `feat(runtime): add semantic event bus`
3. `feat(tools): emit semantic lifecycle events`
4. `feat(agent): map semantic events to ACP notifications`
5. `test(runtime): validate semantic event ordering`

---

# Success Criteria

At the end of this branch:

- runtime emits typed semantic events
- tools lifecycle is observable
- ACP transport consumes events instead of owning business logic
- streaming state has deterministic ordering
- cancellation paths are explicit
- tool result integrity is preserved
