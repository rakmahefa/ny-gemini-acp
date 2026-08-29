# Tool UX / UI Model

This document defines the canonical host-neutral presentation contract for agent tools.

## Canonical ownership

`tool_ux` is the rich semantic builder for concrete tool invocations. It produces `ToolUiModel`-compatible presentation data and MUST NOT construct ACP protocol values.

`ToolUiModel` is the canonical runtime presentation model. The ACP adaptor is the only layer that converts this model into ACP-native `ToolCall`, `ToolCallUpdate`, `ToolCallContent`, `ToolCallLocation`, `ToolKind` and `ToolCallStatus` values.

```text
Tool implementation
      ↓
tool_ux semantic builder
      ↓
ToolUiModel
      ↓
SemanticEvent
      ↓
ACP adaptor
      ↓
Zed thread
```

## Model

```text
ToolUiModel
├── kind
├── title
├── summary
├── status
├── input
├── output
├── content
├── locations
└── expandable
```

The model deliberately separates execution data from rich presentation. Tool result data remains data; the UI must never infer lifecycle or identity by parsing strings such as exit-code decorations or Markdown headings.

## Lifecycle

```text
Pending → Running → Succeeded
                  ├→ Failed
                  └→ Cancelled
```

The canonical `ToolCallId` is preserved separately by the runtime and associates the request, execution and result events with the same tool invocation.

## Rich presentation

`tool_ux` may build rich semantic content for:

- file reads, writes, edits and replacement diffs;
- search, glob and directory results;
- shell execution and terminal references;
- user questions and follow-ups;
- permission and risk indicators;
- bounded previews, summaries and source locations.

Those concepts are represented as host-neutral values first. The ACP-specific rendering step happens only in `acp-adaptor`.

## Zed surface

The primary tool surface should answer, without opening verbose details:

- What is the agent doing?
- What did it do?
- Did it succeed, fail or get cancelled?

Large stdout, file contents, diffs and other verbose information belong in expandable structured content when supported by the host.

## Forbidden architecture

This is not a valid pipeline:

```text
Tool
 ↓
tool_ux
 ↓
ACP ToolCallContent / ToolKind / ToolCallLocation
 ↓
ToolUiModel
 ↓
ACP again
```

In particular, `tools-provider/src/tools/tool_ux` must remain free of ACP presentation imports. A provider may still depend on ACP for an unrelated protocol bridge (for example interactive elicitation), but that dependency must not leak into the visual builder.

## Safety presentation

The runtime distinguishes `Failed` and `Cancelled`. ACP may have a more limited native status vocabulary; any loss of distinction at the protocol boundary must be deliberate and documented rather than reintroduced into the semantic model.

`PolicyDenied` and `ConfinementUnavailable` are semantic safety outcomes and must not silently become ordinary successful execution states.
