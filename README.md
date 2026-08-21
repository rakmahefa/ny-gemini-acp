# ny-gemini-acp

A Rust-based **Agent Client Protocol (ACP)** agent designed for **Zed**, with **Gemini** as the current LLM provider and **builtin + MCP tools** as the tool capability layer.

The `dev/agent-runtime-architecture` branch establishes a provider-neutral runtime architecture. The central design goal is to keep **ACP protocol concerns, agent orchestration, model integration, and tool execution independently bounded** so the runtime can evolve without becoming coupled to Zed, ACP, or Gemini implementation details.

> **Architectural reference:** DeepSeek Harness is used only as a source of architectural ideas. `ny-gemini-acp` does not depend on or attempt to reproduce DeepSeek Harness or Cordis.

## Architecture

The workspace is organized around four principal crates:

```text
                             Zed
                              │
                             ACP
                              │
                     ┌────────▼────────┐
                     │   acp-adaptor   │
                     │                  │
                     │ protocol        │
                     │ presentation    │
                     │ validation      │
                     │ ACP ↔ runtime   │
                     └────────┬─────────┘
                              │
                     ┌────────▼────────┐
                     │  agent-runtime  │
                     │                  │
                     │ sessions        │
                     │ turns / threads │
                     │ agent loop      │
                     │ prompt/context  │
                     │ SemanticEvent   │
                     │ cancellation    │
                     │ replay/lifecycle│
                     │ provider traits │
                     └───────┬───┬──────┘
                             │   │
                ┌────────────┘   └─────────────┐
                │                              │
       ┌────────▼────────┐             ┌───────▼────────┐
       │  llm-provider   │             │ tools-provider │
       │                 │             │                 │
       │ Gemini          │             │ builtin tools   │
       │ provider stream │             │ MCP             │
       │ auth/settings   │             │ process/HTTP    │
       └─────────────────┘             └─────────────────┘
```

### Core invariant

> **ACP is an adapter, not the agent runtime.**

`acp-adaptor` owns ACP protocol semantics and host presentation. `agent-runtime` owns the semantic execution model. `llm-provider` owns Gemini/provider-specific behavior. `tools-provider` owns concrete tool implementations and MCP transport.

The runtime therefore remains host- and provider-neutral even though the product currently targets Zed + Gemini.

## Workspace

The Cargo workspace currently contains exactly these four members:

| Crate | Role |
|---|---|
| `acp-adaptor` | ACP boundary, requests, notifications, permissions, presentation and protocol validation |
| `agent-runtime` | Provider-neutral agent execution, sessions, turns, events, lifecycle, cancellation and provider contracts |
| `llm-provider` | Gemini implementation, authentication/configuration, provider streaming and provider-native parsing |
| `tools-provider` | Builtin tools, MCP integration, tool session configuration and host-neutral tool UI models |

The workspace targets Rust `1.82+`, edition `2021`, and is released under MIT.

## Release artifact

The project intentionally exposes a single product executable:

```text
gemini-acp
```

It is the ACP agent launched by Zed. Internal crates exist to enforce architecture and testability; they are not independent release products.

## Crate responsibilities

### `acp-adaptor`

The ACP-facing boundary.

Responsibilities:

- ACP `initialize` and capability negotiation;
- session and prompt request handling;
- ACP configuration normalization;
- permission and elicitation interaction;
- ACP-specific errors and validation;
- projection of runtime semantic events into ACP notifications;
- projection of host-neutral tool UI into native ACP `ToolCallContent` / `ToolCallLocation` structures.

It must **not** own the core agent loop, provider-specific model parsing, or concrete MCP transport logic.

The crate re-exports the runtime API for adapter-facing integration, while keeping ACP-specific modules under the adapter boundary.

### `agent-runtime`

The semantic core of the application.

Responsibilities include:

- agent loop and turn orchestration;
- thread and turn ownership;
- session lifecycle and state;
- prompt/context construction;
- cancellation and termination;
- provider-neutral model/tool orchestration;
- canonical model contracts;
- canonical `SemanticEvent` lifecycle;
- event streaming and sinks;
- tool presentation model (`ToolUiModel`);
- replay-oriented durable semantics.

The runtime does not import ACP protocol types or Gemini implementation types.

### `llm-provider`

The current model implementation layer.

The crate exposes `GeminiProvider` and provider-specific modules for:

- authentication/cookie/session handling;
- Gemini configuration and settings;
- request construction;
- model selection;
- reasoning configuration;
- provider-native streaming;
- response parsing;
- image upload;
- provider-specific errors.

Provider-native data is normalized before it reaches the runtime contract.

### `tools-provider`

The concrete tool capability layer.

Current scope:

```text
Tool Provider
├── Builtin tools
│   ├── filesystem / file operations
│   ├── shell
│   ├── search / glob / directory operations
│   └── interactive tool support
│
└── MCP
    ├── process / stdio transport
    └── HTTP transport
```

The runtime only knows the generic `ToolProvider` contract and `ToolServerConfig`. MCP-specific configuration and transport stay outside `agent-runtime`.

## Semantic execution model

The architecture separates **transport**, **semantic events**, and **host presentation**.

```text
Gemini wire stream
      │
      ▼
llm-provider
      │
      │ ModelEvent
      ▼
agent-runtime
      │
      │ SemanticEvent
      ├───────────────► EventBus / EventStream / sinks
      │
      └───────────────► ACP projection
                                │
                                ▼
                               Zed
```

### Canonical model contract

The runtime does not consume arbitrary provider chunks as its semantic LLM interface. The provider boundary is expressed through `ModelRequest`, `ModelEvent` and `LlmProvider`.

Current canonical model events are:

```text
TextDelta
ReasoningDelta
ToolCall
Usage
```

The runtime can therefore orchestrate model output without knowing Gemini framing or wire-level details.

### Semantic event contract

`SemanticEvent` is the runtime lifecycle contract. It currently covers:

```text
TurnStarted
AssistantStarted
AssistantDelta
AssistantCompleted
ThinkingStarted
ThinkingDelta
ThinkingCompleted
ToolCallRequested
PermissionRequested
ToolExecutionStarted
ToolResultReceived
TurnCancelled
TurnFailed
TurnCompleted
```

Each event carries explicit runtime context, and tool-related events carry a `ToolEventContext` plus optional `ToolUiModel` data.

This is the important separation:

```text
ModelEvent     = what the model/provider emitted
SemanticEvent  = what the runtime means by it
ACP update     = how the host should receive/display it
```

ACP is therefore a projection of runtime meaning, not the source of truth for the agent lifecycle.

## Tool identity and integrity

Tool lifecycle is correlated by canonical runtime call identity rather than relying on provider-local display text.

The runtime tracks the progression:

```text
ToolCallRequested
        ↓
PermissionRequested        (when required)
        ↓
ToolExecutionStarted
        ↓
ToolResultReceived
```

The tool contract carries a `call_id`, `session_id`, tool name, structured arguments and cancellation state. Results carry both raw execution content and optional presentation metadata.

This keeps raw tool output separate from rich UI projection and prevents presentation formatting from becoming part of the semantic execution protocol.

## Tool UX model

Tool presentation is now a first-class, host-neutral runtime concept through `ToolUiModel`.

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

Supported semantic kinds include file read/write/edit, search, glob, directory listing, shell, search-and-read, replace-in-file, user questions, and a generic fallback.

Lifecycle status is explicit:

```text
Pending → Running → Succeeded
                  ├→ Failed
                  └→ Cancelled
```

The host may map these semantic values to icons, compact cards, expandable details, locations, or other native UI without changing runtime behavior.

Rich tool content and locations are intentionally kept distinct from raw tool output. The ACP adapter converts these host-neutral values to native ACP structures at the final boundary.

## MCP boundary

MCP is a **tool capability**, not an LLM feature.

When Zed provides MCP servers through ACP, the data flow is:

```text
ACP McpServer
      ↓
acp-adaptor normalization
      ↓
ToolServerConfig
      ↓
tools-provider
      ↓
MCP process / HTTP transport
```

The adapter is responsible for translating ACP-specific server representations. The tool provider owns concrete MCP transport and discovery behavior.

Architectural invariants:

- preserve the complete configured server set;
- keep server and tool identities deterministic;
- isolate independent MCP discovery failures where possible;
- never make `agent-runtime` depend on ACP `McpServer` types;
- never make `llm-provider` depend on ACP protocol types;
- keep MCP transport details inside `tools-provider`;
- keep tool presentation host-neutral until the ACP projection boundary.

## Sessions, threads and turns

Sessions are runtime state, not ACP implementation details.

The execution model distinguishes durable/session-level state from transient streaming notifications:

```text
Session
  └── Thread
       ├── Turn
       │    ├── model interaction
       │    ├── semantic events
       │    ├── tool calls
       │    └── terminal state
       └── future turns
```

The runtime exposes explicit thread/turn handles and cancellation/ownership mechanisms. This allows ACP request handling to remain thin while the runtime maintains execution invariants.

The durable session model is intended to remain the source of truth for conversation history, tool history, metadata, cancellation state and future replay/projection needs.

## Event transport and projection

The runtime has an explicit event transport layer around `SemanticEvent`:

```text
Agent execution
      ↓
TurnEventEmitter
      ↓
TurnEventSink / EventBus
      ↓
EventStream
      ↓
ACP adapter projection
```

This design allows the same semantic lifecycle to feed multiple consumers without making the agent loop directly aware of ACP notification APIs.

## Provider independence

The intended dependency direction is:

```text
Zed
  ↓
ACP Adapter
  ↓
Agent Runtime
  ├── LLM Provider
  └── Tool Provider
```

Not:

```text
Gemini
  └── ACP
      └── runtime
          └── tools
```

A future provider should implement the existing runtime-facing contracts instead of creating provider-shaped branches inside `agent-runtime`.

## Architecture guardrails

The repository includes `scripts/audit-provider-neutral.sh` as an executable architecture audit.

The audit enforces the most important boundaries, including:

- `agent-runtime` production code contains no ACP protocol references;
- `agent-runtime` production code contains no Gemini-specific references;
- `llm-provider` has no direct ACP protocol dependency;
- provider entry points do not expose ACP types;
- ACP MCP configuration is normalized in the adapter;
- provider traits remain centralized in `agent-runtime`;
- legacy `gemini-acp-*` architecture identities do not return to production code;
- tool-provider session ownership remains behind `ToolProvider`.

Warnings from the audit are intentionally informational for areas where the contract still contains some generic values such as `serde_json::Value`.

## Validation

Run the architecture baseline from the repository root:

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets
./scripts/audit-provider-neutral.sh
```

The expected result is:

```text
PASS: no hard-boundary failures
```

The audit can report non-blocking warnings while the architecture is being progressively strengthened.

## Migration status

### Established

- four-crate provider-neutral workspace;
- dependency-injected `LlmProvider` and `ToolProvider` contracts;
- runtime/provider separation;
- canonical `ModelRequest` / `ModelEvent` contract;
- canonical `SemanticEvent` lifecycle;
- explicit event bus/stream/sink architecture;
- session/thread/turn execution primitives;
- canonical tool call identity and tool lifecycle events;
- host-neutral `ToolUiModel` and explicit tool UI lifecycle;
- generic `ToolServerConfig` and MCP normalization at the ACP boundary;
- Gemini isolated in `llm-provider`;
- builtin + MCP implementations isolated in `tools-provider`;
- provider-neutral architecture audit script;
- workspace validation command defined as the branch baseline.

### Current architectural focus

The architecture should now favor **consolidation and integrity** rather than another physical crate split.

Priority areas are:

1. strengthen the end-to-end integrity of `SemanticEvent → ACP` projection;
2. keep tool IDs, raw results, rich tool UI content and locations correlated deterministically;
3. reduce weakly typed contract fields where a stable semantic type can replace generic JSON/string surfaces;
4. preserve replay and cancellation invariants as execution paths expand;
5. validate new model providers against the existing provider contracts instead of branching the runtime;
6. keep ACP handlers focused on protocol and presentation rather than agent-loop policy.

## Non-goals

This architecture does not currently aim to:

- replace Zed;
- replace ACP;
- depend on DeepSeek Harness or Cordis;
- introduce multiple LLM providers before the runtime contracts are stable;
- move MCP into the LLM provider;
- make ACP handlers responsible for the core agent loop;
- encode host-specific visual behavior directly into `agent-runtime`.

## Success criteria

The architecture is successful when:

1. Zed continues to operate through ACP without behavioral regressions.
2. Gemini remains fully functional through `LlmProvider`.
3. Builtin and MCP tools share the same runtime tool abstraction.
4. Tool call identity and tool lifecycle remain correlated from model request through ACP presentation.
5. `SemanticEvent` remains the runtime lifecycle source of truth.
6. ACP handlers contain protocol/presentation concerns rather than the core agent loop.
7. Provider-specific parsing stays isolated from `agent-runtime`.
8. Tool UI can evolve without contaminating the raw execution contract.
9. Session/thread state can be replayed and projected consistently.
10. A second LLM provider can be added without redesigning ACP, sessions, or MCP.

## Reference

DeepSeek Harness is used only as an architectural reference for ideas such as explicit seams, lifecycle-oriented events, composable providers, and separation between transport adapters and agent execution. The implementation of `ny-gemini-acp` remains independently designed around Rust, ACP/Zed compatibility, Gemini, and the provider-neutral runtime described above.
