# Visual Pipeline Contract

## Status

This is the canonical end-to-end visual contract for tool execution and tool results.

## Pipeline

```text
Tool implementation
      ↓
tool_ux semantic builder
      ↓
ToolUiModel
      ↓
SemanticEvent
      ↓
Runtime integrity validation
      ↓
ACP adaptor projection
      ↓
ACP ToolCall / ToolCallUpdate
      ↓
Zed thread
```

### Ownership

- `tool_ux` owns rich, host-neutral presentation semantics.
- `agent-runtime` owns the canonical `ToolUiModel`, lifecycle and `ToolCallId` integrity.
- `acp-adaptor` owns all ACP-native visual projection.
- Zed renders the resulting ACP protocol messages.

### Result lifecycle

```text
ToolCallRequested
   → ToolUiModel(Pending)
   → SemanticEvent
   → ACP ToolCall

ToolExecutionStarted
   → ToolUiModel(Running)
   → SemanticEvent
   → ACP ToolCallUpdate

ToolResultReceived
   → ToolUiModel(Succeeded | Failed | Cancelled)
   → SemanticEvent
   → ACP ToolCallUpdate
```

The same canonical `ToolCallId` identifies the invocation through the complete lifecycle.

### Rich content

The following are semantic presentation values until the ACP boundary:

```text
text card
file diff
source location
terminal reference
bounded preview
risk / permission indicator
```

The ACP adaptor converts those values into ACP-native content only once.

### Forbidden second pipeline

```text
Tool
 ↓
tool_ux
 ↓
ACP-native visual object
 ↓
ToolUiModel
 ↓
ACP again
```

`tools-provider/src/tools/tool_ux` must not import ACP presentation types. This invariant is architectural, not merely stylistic.

### Zed contract

The primary thread surface must communicate:

1. what the agent is doing;
2. what the tool did;
3. whether it succeeded, failed or was cancelled.

Verbose stdout, diffs, file contents and similar details should remain structured/expandable where the host supports it.
