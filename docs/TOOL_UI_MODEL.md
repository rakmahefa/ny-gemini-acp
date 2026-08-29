# Tool UX / UI Model

This document defines the canonical host-neutral presentation contract for agent tools.

## Canonical ownership

`tool_ux` is the rich semantic builder for concrete tool invocations. It produces presentation data for `ToolUiModel` and MUST NOT construct ACP protocol values.

`ToolUiModel` is the canonical runtime presentation model. The ACP adaptor is the only layer that converts it into ACP-native `ToolCall`, `ToolCallUpdate`, `ToolCallContent`, `ToolCallLocation`, `ToolKind` and `ToolCallStatus` values.

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

## Conceptual form

```text
ToolUiModel
├── identity
│   ├── semantic tool call id
│   └── tool kind
├── lifecycle
│   ├── Pending
│   ├── Running
│   ├── Succeeded
│   ├── Failed
│   └── Cancelled
├── primary
│   ├── title
│   └── short user-facing summary
├── input
│   └── small structured facts safe for display
├── output
│   └── optional structured facts + raw data
├── content
│   └── host-neutral rich content
├── locations
│   └── host-neutral source locations
└── details
    └── collapsible verbose output
```

## Rules

1. **Semantic first.** `tool_ux` builds semantic presentation data; the runtime carries `ToolUiModel`; the ACP adapter maps it to widgets.
2. **No text parsing.** Tool names, statuses, file paths, commands, matches, counts, and identities are structured fields.
3. **Small primary surface.** The title and summary must be readable in a compact card. Large stdout, file contents, and diffs belong in expandable details.
4. **Mutation clarity.** `file_write`, `file_edit`, and `replace_in_file` must communicate what changed, not only that execution succeeded.
5. **Execution clarity.** `shell_exec` must distinguish command, running state, exit status, timeout, policy denial, and output.
6. **Search clarity.** `search`, `glob`, and `list_directory` should expose counts and paths without forcing the host to parse lines.
7. **Safety clarity.** A blocked or denied action must have a distinct semantic failure state and structured reason; it must not look like an ordinary provider error.
8. **Privacy by construction.** Large replacement strings, raw prompts, and verbose outputs should not be duplicated into primary UI metadata.

## Rich presentation

`tool_ux` may provide rich semantic content for file reads, writes, edits and replacement diffs; search, glob and directory results; shell execution and terminal references; user questions and follow-ups; permission/risk indicators; bounded previews; summaries; and source locations. These values remain host-neutral until the ACP projection boundary.

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

## Builtin mapping

| Tool | UI kind | Primary surface | Details |
|---|---|---|---|
| `file_read` | `FileRead` | file + requested range | file contents |
| `file_write` | `FileWrite` | created/updated file | content statistics |
| `file_edit` | `FileEdit` | edited file + replacement count | diff/details |
| `replace_in_file` | `ReplaceInFile` | edited file | diff/details |
| `glob` | `Glob` | pattern + match count | paths |
| `list_directory` | `DirectoryList` | directory + entry count | entries |
| `search` | `Search` | pattern + match count | matches |
| `search_and_read` | `SearchAndRead` | pattern + excerpts | excerpts |
| `shell_exec` | `Shell` | command + lifecycle + safety outcome | stdout/stderr/exit status/reason |
| `AskUserQuestion` | `AskUserQuestion` | question state | response form |

## Zed surface

The user should be able to answer three questions without opening details:

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

In particular, `tools-provider/src/tools/tool_ux` must remain free of ACP presentation imports. A provider may still depend on ACP for unrelated protocol operations; that dependency must not become a second visual contract.
