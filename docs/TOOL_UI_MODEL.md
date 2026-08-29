# Tool UX / UI Model

This document defines the host-neutral presentation contract for agent tools. `ToolUiModel` is the canonical semantic visual model used by the runtime; ACP is only a downstream renderer.

## Responsibility boundaries

```text
tool_ux
    = rich semantic visual builder

ToolUiModel
    = canonical runtime visual contract

SemanticEvent
    = canonical lifecycle/event transport

acp-adaptor
    = only ACP visual renderer
```

The unique end-to-end pipeline is:

```text
Tool implementation
    ↓
tool_ux
    ↓
ToolUiModel
    ↓
SemanticEvent
    ↓
integrity
    ↓
acp-adaptor
    ↓
ACP
    ↓
Zed thread
```

`tool_ux` decides **what** the user should see. It must remain host-neutral and must not construct ACP presentation values. `ToolUiModel` carries that meaning in structured runtime data. `SemanticEvent` carries lifecycle state with the same `ToolCallId`. The ACP adaptor decides **how** to express the model through ACP.

## Canonical model

```text
ToolUiModel
├── kind
├── title
├── summary
├── status
│   ├── Pending
│   ├── Running
│   ├── Succeeded
│   ├── Failed
│   └── Cancelled
├── input
├── output
├── content
├── locations
└── expandable
```

The model is the single runtime visual contract. Hosts must not recover semantic meaning by parsing formatted result strings, Markdown headings, exit-code decorations, or provider-specific prefixes.

## Rich presentation surface

The semantic builder intentionally preserves the existing rich UX vocabulary:

- semantic tool kind, title, and summary;
- lifecycle presentation, permission state, and risk level;
- bounded input/output previews and rich cards;
- file diffs for writes and edits;
- terminal references and terminal lifecycle metadata for shell execution;
- structured source locations for reads, searches, glob, and filesystem results;
- distinct interaction presentations for `AskUserQuestion` and `FollowUp`.

The existing visual identities remain semantic concepts:

```text
📖 File Read
📝 File Write
✏️ File Edit
🧭 Glob
📁 Directory
🔎 Search
▣ Shell
⚙️ Ask User
↪ Follow-up
```

## Lifecycle and identity

A representative lifecycle is:

```text
ToolCallRequested
    → PermissionRequested (when required)
    → ToolExecutionStarted
    → ToolResultReceived
```

Every tool-scoped event uses the same `ToolCallId`. The final `ToolResultReceived` carries the result-side `ToolUiModel` where visual state is required. Runtime integrity validates lifecycle ordering and identity before the model reaches a protocol renderer.

## Tool-result contract

A final result keeps its structured semantic information instead of collapsing it to text. Depending on the tool, this includes:

```text
status
output
content
locations
terminal metadata
```

Examples:

```text
FileRead
    ToolUiModel(kind=FileRead, locations=[...], output=...)

FileEdit / FileWrite
    ToolUiModel(kind=FileEdit|FileWrite, content=[card,diff], locations=[...])

Shell
    ToolUiModel(kind=Shell, content=[card,terminal], output=...)

Search / Glob
    ToolUiModel(kind=Search|Glob, locations=[...], output=...)
```

The result remains associated with the same `ToolCallId` as the request.

## ACP projection

Only `acp-adaptor` converts the semantic model into ACP presentation types.

```text
ToolUiModel(FileRead)
    → ACP ToolKind::Read + ToolCallLocation + content/output

ToolUiModel(FileEdit/FileWrite)
    → ACP ToolKind::Edit + Diff + ToolCallLocation

ToolUiModel(Shell)
    → ACP ToolKind::Execute + Terminal + terminal metadata
```

The projection is explicit and testable. A malformed or unsupported rich semantic value produces a structured projection error rather than being silently discarded. A simple text fallback is used only when no rich content exists for the raw-output surface.

## Forbidden architecture

The following model flow must never return:

```text
tool_ux
   ↓
ACP ToolKind / ToolCallContent / ToolCallLocation
   ↓
ToolUiModel
   ↓
ACP again
```

ACP presentation types do not belong in `tools-provider/src/tools/tool_ux`. ACP protocol interactions such as permission requests may still use ACP request/response types in protocol-facing executor code; their visual payload must originate from host-neutral semantic data.

## UX target

The user should be able to answer three questions without opening details:

- **What is the agent doing?**
- **What did it do?**
- **Did it succeed?**

Everything else is secondary detail.
