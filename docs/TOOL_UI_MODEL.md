# Tool UX / UI Model

This document defines the host-neutral presentation contract for agent tools. It is the canonical semantic visual model used by the runtime; ACP is only one downstream renderer.

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

`tool_ux` decides **what** the user should see. It must remain host-neutral and must not construct ACP presentation values. `ToolUiModel` carries that meaning in structured runtime data. `SemanticEvent` carries the lifecycle with the same `ToolCallId`. The ACP adaptor decides **how** to express the model through ACP.

The following is explicitly forbidden:

```text
tool_ux
   ↓
ACP ToolKind / ToolCallContent / ToolCallLocation
   ↓
ToolUiModel
   ↓
ACP again
```

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

The model must remain structured. Hosts must never recover semantic meaning by parsing formatted result strings, Markdown headings, exit-code decorations, or provider-specific prefixes.

## Rich presentation surface

`tool_ux` owns the rich semantic vocabulary required by the current experience, including:

- semantic tool kind and human-readable title/summary;
- lifecycle presentation, permission state, and risk level;
- bounded input/output previews and rich cards;
- file diffs for writes and edits;
- terminal references for shell execution;
- structured source locations for reads, searches, and filesystem results;
- distinct user-interaction presentations for `AskUserQuestion` and `FollowUp`.

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

## Tool-result contract

A final tool result remains associated with the same `ToolCallId` established by the request. Its `ToolUiModel` preserves status, output, rich content, locations, and applicable terminal metadata.

Representative projections are:

```text
FileRead
    ToolUiModel(kind=FileRead, locations=[...], output=...)
        → ACP ToolKind::Read + ToolCallLocation + output/content

FileEdit / FileWrite
    ToolUiModel(kind=FileEdit|FileWrite, content=[card,diff], locations=[...])
        → ACP ToolKind::Edit + Diff + ToolCallLocation

Shell
    ToolUiModel(kind=Shell, content=[card,terminal])
        → ACP ToolKind::Execute + Terminal
```

These ACP objects are produced only at the adaptor boundary.

## Projection rules

The adaptor performs explicit semantic conversion for the supported rich content variants. A malformed or unsupported rich semantic value is a projection error, not silently discarded data. Simple raw textual output may still be used as an intentional fallback when no rich content exists.

## UX target

The user should be able to answer three questions without opening details:

- **What is the agent doing?**
- **What did it do?**
- **Did it succeed?**

Everything else is secondary detail.
