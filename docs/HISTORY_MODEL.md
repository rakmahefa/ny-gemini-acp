# History Model

The agent runtime owns the canonical conversation history. Providers and ACP are projections of that model; they must not become the persisted source of truth.

## Canonical entries

`HistoryEntry` currently has four semantic forms:

- `User { content }`
- `Assistant { content }`
- `ToolCall { id, name, arguments }`
- `ToolResult { id, name, content, is_ok }`

A tool call and its result are therefore persisted as structured data rather than as opaque text embedded in an assistant message.

## Compatibility boundary

The execution loop still exposes a temporary `Vec<(Role, String)>`-compatible view through `History`. This allows older runtime paths to continue pushing messages while the history model is migrated incrementally.

The compatibility view is never the persistence format. Before a turn is persisted, legacy entries are normalized into canonical entries. Existing session files containing tuple-form `messages` remain readable.

## Prompt projection

Prompt construction reads the canonical `HistoryEntry` sequence and renders it for the active LLM provider. The sliding window is turn-aware: it starts at user-message boundaries so a tool lifecycle is not accidentally cut in half.

## Invariants

History must preserve ordering. A canonical tool result keeps the semantic tool-call identity whenever it is available. Arbitrary tool output is treated as content and never parsed as protocol syntax except for the narrowly defined legacy compatibility markers written by the previous runtime.

This design keeps the runtime history independent from ACP, Gemini stream formatting, and tool-output text.
