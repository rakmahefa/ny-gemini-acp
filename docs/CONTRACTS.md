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

- `tool_ux` is the **rich semantic visual builder**. It decides what the user should see: tool kind, title, summary, lifecycle presentation, permission/risk signals, rich cards, diffs, terminal references, locations, and bounded input/output previews.
- `ToolUiModel` is the **canonical runtime visual contract**. It is host-neutral and carries structured visual meaning without requiring a downstream host to parse strings.
- `SemanticEvent` is the **canonical lifecycle/event transport**. `ToolCallRequested`, `PermissionRequested`, `ToolExecutionStarted`, and `ToolResultReceived` preserve the same validated `ToolCallId`; a `ToolUiModel` is attached where the lifecycle stage needs visual state.
- Runtime integrity validates event ordering, session/turn identity, and tool-call identity before protocol presentation.
- `acp-adaptor` is the **only ACP visual renderer**. It is the only layer that maps semantic `ToolUiModel` values to ACP `ToolKind`, `ToolCallContent`, `ToolCallLocation`, `ToolCallStatus`, `ToolCall`, and `ToolCallUpdate`.

### ACP interaction exception

A protocol interaction such as `session/request_permission` is allowed to use ACP request/response types in executor code because that code implements an ACP protocol interaction. This does not make ACP part of the semantic visual contract: its visual payload is projected from host-neutral `ToolInfo`/`ToolUiModel` data rather than defined inside `tool_ux`.

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

A `ToolResultReceived` event belongs to exactly one semantic tool call identity. The final result is not reconstructed from arbitrary display text and is not allowed to fork into a second visual pipeline.

A final tool result preserves, as applicable:

- final lifecycle status;
- structured output and bounded previews;
- rich content/cards;
- file diff information;
- structured source locations;
- terminal reference and terminal lifecycle metadata.

The result remains correlated with the same `ToolCallId` established by `ToolCallRequested`.

Representative tool families retain their semantic identities:

```text
FileRead   → read presentation
FileWrite  → write presentation + diff
FileEdit   → edit presentation + diff
Glob       → path/search presentation + locations
Search     → search presentation + locations
Shell      → execution presentation + terminal reference
AskUser    → user-interaction presentation
FollowUp   → follow-up presentation
```

## Runtime event lifecycle

Representative lifecycle:

```text
ToolCallRequested
    → PermissionRequested (when required)
    → ToolExecutionStarted
    → ToolResultReceived
```

All tool-scoped events use the same `ToolCallId`. The runtime does not reconstruct identity from display text, tool names, or result contents.

## Ordering and replay

Every semantic event carries a monotonically increasing per-turn sequence. Replay remains valid only when session/turn identity is stable, sequence numbers are contiguous from zero, and terminal lifecycle ordering is respected.

Persisted `ToolCall` and `ToolResult` history entries retain the semantic tool identifier so the ACP adaptor can replay the same tool identity when a session is loaded again.

## ACP projection

ACP projection is explicit and centralized at the adaptor boundary.

Supported examples include:

```text
ToolUiModel(FileRead)
    → ACP ToolKind::Read + ToolCallLocation + content/output

ToolUiModel(FileEdit/FileWrite)
    → ACP ToolKind::Edit + Diff + ToolCallLocation

ToolUiModel(Shell)
    → ACP ToolKind::Execute + Terminal + terminal metadata
```

Malformed or unsupported rich semantic values are structured projection errors. Important content is never silently discarded with `filter_map(...).ok()` semantics. A simple text fallback is permitted only for the intentionally simple raw-output surface.

## Architecture guardrail

The repository contains an automated architecture test covering `tools-provider/src/tools/tool_ux`. The test rejects ACP presentation references such as `ToolKind`, `ToolCallContent`, `ToolCallLocation`, `ToolCallStatus`, `Diff`, and `Terminal` in that builder layer.

This guardrail exists specifically to prevent a second visual pipeline from being reintroduced during future refactors.

## Security boundary

`agent-runtime` validates semantic lifecycle and tool identity. Shell policy and command normalization are application policies, not OS isolation. The runtime MUST NOT claim host confinement unless an OS-level sandbox is actually configured and enforced.

## Persistence guarantees

A successful turn finalization means the store accepted the final state according to its atomic-write and synchronization contract. Persistence failures remain explicit runtime errors and MUST NOT be silently converted into successful turns.
