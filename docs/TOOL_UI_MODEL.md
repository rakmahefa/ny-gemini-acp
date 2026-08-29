# Tool UX / UI Model

This document defines the canonical host-neutral presentation contract for agent tools.

`tool_ux` is the rich semantic builder for concrete tool invocations. It produces presentation data for `ToolUiModel`; it does not construct ACP protocol values.

`ToolUiModel` is the canonical runtime presentation model. The ACP adaptor is the only layer that converts it into ACP-native `ToolCall`, `ToolCallUpdate`, `ToolCallContent`, `ToolCallLocation`, `ToolKind` and `ToolCallStatus` values.

The canonical end-to-end path is:

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

The tool result must remain data. The UI must never infer meaning by parsing strings such as `[exit code 0]`, `[tool_result ...]`, or Markdown headings.

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

## Rules

1. **Semantic first.** `tool_ux` builds semantic presentation data; the runtime carries `ToolUiModel`; the ACP adaptor maps it to protocol widgets.
2. **No text parsing for lifecycle or identity.** Tool names, statuses, file paths, commands, matches, counts and tool-call identities remain structured.
3. **Small primary surface.** The title and summary must be readable in a compact card. Large stdout, file contents and diffs belong in expandable details.
4. **Mutation clarity.** `file_write`, `file_edit` and `replace_in_file` must communicate what changed, not only that execution succeeded.
5. **Execution clarity.** `shell_exec` must distinguish command, running state, exit status, timeout, policy denial and output.
6. **Search clarity.** `search`, `glob` and `list_directory` should expose counts and paths without forcing the host to parse arbitrary output.
7. **Safety clarity.** A blocked or denied action must have a distinct semantic outcome and structured reason; it must not look like an ordinary provider error.
8. **Privacy by construction.** Large replacement strings, raw prompts and verbose outputs should not be duplicated into primary UI metadata.

## Rich presentation

`tool_ux` may provide rich semantic content for file diffs, terminal references, source locations, bounded previews, lifecycle/risk/permission labels, search results, user questions and follow-ups. These remain semantic values until the ACP projection boundary.

## Shell safety semantics

The shell policy is a semantic execution gate, not an OS isolation mechanism.

The UI may distinguish these outcomes:

```text
Allowed
Running
Succeeded
Failed
Cancelled
PolicyDenied
ConfinementUnavailable
```

`PolicyDenied` means the command was rejected before execution by the application policy. `ConfinementUnavailable` is reserved for a future execution backend when OS-level confinement is required but unavailable. The current shell policy does **not** claim host isolation.

## Zed surface

The primary tool surface should answer three questions without opening verbose details:

- **What is the agent doing?**
- **What did it do?**
- **Did it succeed, fail or get cancelled?**

Large stdout, file contents, diffs and other verbose information belong in structured expandable content when supported by the host.

## Forbidden architecture

This is not a valid pipeline:

```text
Tool
 ↓
tool_ux
 ↓
ACP ToolCallContent / ToolKind / ToolCallLocation
 ↓
ToolUiModel / SemanticEvent
 ↓
ACP again
```

In particular, `tools-provider/src/tools/tool_ux` must remain free of ACP presentation imports. A provider may still depend on ACP for an unrelated protocol operation; that dependency must not leak into the visual builder.

## Safety presentation

The runtime distinguishes `Failed` and `Cancelled`. ACP may have a more limited native status vocabulary; any loss of distinction at the protocol boundary must be deliberate and documented rather than reintroduced into the semantic model.
