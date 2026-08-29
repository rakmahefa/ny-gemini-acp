# Runtime contracts

This document defines the normative application-level contracts for the provider-neutral runtime and its ACP boundary.

## Single visual pipeline

There is exactly one visual contract:

```text
Tool implementation
    ↓
tool_ux
    ↓
ToolUiModel
    ↓
SemanticEvent
    ↓
Runtime integrity
    ↓
acp-adaptor
    ↓
ACP ToolCall / ToolCallUpdate
    ↓
Zed thread
```

The responsibilities are deliberately separated:

- `tool_ux` is the **rich semantic visual builder**. It knows what the user should see: tool kind, title, summary, lifecycle presentation, permission and risk signals, rich cards, diffs, terminal references, locations, and bounded input/output previews.
- `ToolUiModel` is the **canonical runtime visual contract**. It is host-neutral and carries the structured visual information needed by the UI without requiring string parsing.
- `SemanticEvent` is the **canonical lifecycle/event transport**. Tool lifecycle events carry the same validated `ToolCallId` and may carry the associated `ToolUiModel`.
- Runtime integrity validates ordering, turn identity and tool identity before protocol projection.
- `acp-adaptor` is the **only ACP visual renderer**. It is the sole layer that maps `ToolUiModel` semantic values to ACP `ToolKind`, `ToolCallContent`, `ToolCallLocation`, `ToolCallStatus`, `ToolCall`, and `ToolCallUpdate`.

The following pipeline is forbidden:

```text
tool_ux
    ↓
ACP ToolKind / ToolCallContent / ToolCallLocation
    ↓
ToolUiModel
    ↓
ACP again
```

ACP presentation types must not leak into `tools-provider/src/tools/tool_ux`.

## Tool-result semantics

A `ToolResultReceived` event belongs to exactly one semantic tool call identity. Tool output is data and remains separate from protocol syntax. Permission denial, cancellation, policy rejection, execution failure, and execution success are distinct lifecycle outcomes.

For a tool result, the final `ToolUiModel` preserves the status and structured visual surface produced by the tool builder. The result remains correlated with the same `ToolCallId` that was established by `ToolCallRequested`.

## Runtime event lifecycle

Representative lifecycle:

```text
ToolCallRequested
    → PermissionRequested (when required)
    → ToolExecutionStarted
    → ToolResultReceived
```

All tool-scoped events use the same `ToolCallId`. The runtime does not reconstruct tool identity from arbitrary result text.

## Ordering and replay

Every semantic event carries a monotonically increasing per-turn sequence. A replay journal is valid only when session/turn identity is stable, sequence numbers are contiguous from zero, and exactly one terminal event occurs at the end.

## Security boundary

`agent-runtime` validates semantic lifecycle and tool identity. Shell policy and command normalization are application policies, not OS isolation. The runtime MUST NOT claim host confinement unless an OS-level sandbox is actually configured and enforced.

## Persistence guarantees

A successful turn finalization means the store accepted the final state according to its atomic-write and synchronization contract. Persistence failures remain explicit runtime errors and MUST NOT be silently converted into successful turns.

## ACP projection

ACP projection is explicit and centralized at the adaptor boundary. Rich semantic content that cannot be projected into a supported ACP representation is a structured projection failure rather than silently dropped data. Text fallback is used only for the intentionally simple raw-output surface.
