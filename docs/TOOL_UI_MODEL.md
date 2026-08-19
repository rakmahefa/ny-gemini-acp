# Tool UX / UI Model

This document defines the host-neutral presentation contract for agent tools.

The tool result must remain data. The UI must never infer meaning by parsing strings such as `[exit code 0]`, `[tool_result ...]`, or Markdown headings.

## Conceptual form

```text
ToolCard
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
└── details
    └── collapsible verbose output
```

## Rules

1. **Semantic first.** The runtime carries `ToolUiModel`; the ACP adapter or future UI maps it to widgets.
2. **No text parsing.** Tool names, statuses, file paths, commands, matches, and counts are structured fields.
3. **Small primary surface.** The title and summary must be readable in a compact card. Large stdout, file contents, and diffs belong in expandable details.
4. **Mutation clarity.** `file_write`, `file_edit`, and `replace_in_file` must communicate what changed, not only that execution succeeded.
5. **Execution clarity.** `shell_exec` must distinguish command, running state, exit status, timeout, and output.
6. **Search clarity.** `search`, `glob`, and `list_directory` should expose counts and paths without forcing the host to parse lines.
7. **Safety clarity.** A blocked or denied action must have a distinct failure state; it must not look like an ordinary provider error.
8. **Privacy by construction.** Large replacement strings, raw prompts, and verbose outputs should not be duplicated into primary UI metadata.

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
| `shell_exec` | `Shell` | command + lifecycle | stdout/stderr/exit status |
| `AskUserQuestion` | `AskUserQuestion` | question state | response form |

## UX target

The user should be able to answer three questions without opening details:

- **What is the agent doing?**
- **What did it do?**
- **Did it succeed?**

Everything else is secondary detail.
