# Visual Pipeline Contract

## Status

This document defines the canonical end-to-end visual contract for tool execution.

## Canonical pipeline

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

`tool_ux` owns rich, host-neutral tool presentation semantics. It may derive titles, summaries, lifecycle presentation, risk/permission information, rich content, locations and terminal references, but it MUST NOT construct ACP protocol types.

`ToolUiModel` is the canonical runtime representation of tool presentation. ACP-native `ToolKind`, `ToolCallContent`, `ToolCallLocation`, `ToolCallStatus` and related protocol values are created only by `acp-adaptor`.

## Result lifecycle

```text
ToolCallRequested
   → ToolUiModel(Pending)
   → ToolCall

ToolExecutionStarted
   → ToolUiModel(Running)
   → ToolCallUpdate

ToolResultReceived
   → ToolUiModel(Succeeded | Failed | Cancelled)
   → ToolCallUpdate
```

The canonical `ToolCallId` is preserved through the entire path. UI content and locations remain correlated with that identity.

## Presentation rules

The primary Zed surface MUST communicate:

1. what the tool is doing;
2. what it did;
3. whether it succeeded, failed or was cancelled.

Large stdout, file contents, diffs and other verbose material belong in structured expandable content when supported by the host.

Permission and safety state MUST remain distinguishable from ordinary execution failure.

## Forbidden second pipeline

The repository MUST NOT use this path as an independent visual contract:

```text
Tool implementation
      ↓
tool_ux
      ↓
ACP ToolCallContent / ToolKind / ToolCallLocation
      ↓
ToolUiModel / SemanticEvent
      ↓
ACP again
```

In particular, `tools-provider` MUST NOT import ACP presentation types merely to construct tool UI. Protocol rendering belongs at the ACP boundary.

## Compatibility

The rich presentation capabilities previously implemented by `tool_ux` remain part of the contract, including diffs, terminal references, locations, bounded input/output previews, lifecycle/risk/permission labels and tool-specific summaries. These capabilities are represented semantically first and projected to ACP only once.
