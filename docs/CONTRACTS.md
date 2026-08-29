# Runtime contract surface

This document is the normative application-level contract for the provider-neutral runtime.

## Semantic and visual boundary

`agent-runtime` owns semantic lifecycle, tool identity and the canonical `ToolUiModel` presentation contract. `tools-provider/tool_ux` is the rich semantic builder used to create that model.

`tool_ux` MUST remain host-neutral: it MUST NOT construct ACP presentation types. ACP-native `ToolKind`, `ToolCallContent`, `ToolCallLocation`, `ToolCallStatus`, `ToolCall` and `ToolCallUpdate` values are created only at the ACP adapter boundary.

The canonical end-to-end visual path is:

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
Zed thread
```

There is exactly one visual contract. `ToolInfo` is an internal semantic presentation helper and is not an ACP contract.

## Security boundary

`agent-runtime` validates semantic lifecycle and tool identity. The ACP adaptor is a transport boundary and does not parse model tool syntax. Shell policy and command normalization are defensive application policies, not OS isolation.

The runtime MUST NOT claim host confinement unless an OS-level sandbox is actually configured and enforced.

## Persistence guarantees

Persisted session state is finalized through the runtime store boundary. A successful turn finalization means the store accepted the final state according to its atomic-write and synchronization contract. Persistence failures remain explicit runtime errors; they MUST NOT be silently converted into successful turns.

## Tool-result semantics

A `ToolResultReceived` event belongs to exactly one semantic tool call identity. Tool output is data and MUST remain separate from protocol syntax. Permission denial, cancellation and execution result are distinct terminal outcomes of the tool lifecycle.

Rich UI content, source locations, previews and summaries are presentation data associated with the same semantic `ToolCallId`; they are not alternative execution results.

## Cancellation semantics

Cancellation is terminal at the turn level. Open semantic scopes are closed before `TurnCancelled` is emitted, and open tool calls are terminalized as cancelled rather than fabricated as successful results.

## Failure semantics

`TurnFailed` is terminal. The runtime may preserve the underlying structured error for diagnostics while exposing only protocol-safe error data at the ACP boundary.

## Identifier ownership

`SessionId` identifies a session, `TurnId` identifies one turn within that session, and `ToolCallId` identifies one tool invocation within a turn. Tool identifiers are owned by the semantic runtime after validation and are never inferred from arbitrary tool-result text.

## Ordering and replay

Every semantic event carries a monotonically increasing per-turn sequence. A replay journal is valid only when session/turn identity is stable, sequence numbers are contiguous from zero, and exactly one terminal event occurs at the end.

## ACP projection

The ACP layer consumes validated semantic events and projects the canonical `ToolUiModel` into ACP-native messages. ACP transport failure is observable and MUST prevent a successful mandatory transport publication.

## Documentation invariant

Any future tool UX feature MUST update the semantic `ToolUiModel` contract first. A new direct `tools-provider → ACP` visual pipeline is prohibited. Rich tool presentation may evolve in `tool_ux`, but it must remain semantic until the ACP projection boundary.
