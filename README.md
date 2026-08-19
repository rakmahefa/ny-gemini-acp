# ny-gemini-acp

A Rust-based Agent Client Protocol (ACP) agent designed first for **Zed**, with **Gemini** as the initial LLM provider and **MCP** as the primary external tool integration.

The project is now organized around a provider-neutral agent runtime. The goal is to keep Zed/ACP compatibility stable while making the agent loop, model integration, tools, sessions, and future providers independently replaceable.

> **Architectural reference:** DeepSeek Harness is used as an architectural reference only. This project does not depend on or attempt to reproduce DeepSeek Harness or Cordis.

## Architecture

The repository is intentionally organized around four principal crates:

```text
                           Zed
                            │
                           ACP
                            │
                    ┌───────▼────────┐
                    │   ACP Adapter  │
                    │                │
                    │ protocol ↔     │
                    │ runtime        │
                    └───────┬────────┘
                            │
                    ┌───────▼────────┐
                    │  Agent Runtime │
                    │                │
                    │ Agent Loop     │
                    │ Session        │
                    │ Context        │
                    │ Events         │
                    │ Lifecycle      │
                    │ Policies       │
                    └───────┬────────┘
                            │
                 ┌──────────┴──────────┐
                 │                     │
        ┌────────▼────────┐   ┌────────▼────────┐
        │  LLM Provider   │   │  Tool Provider  │
        │                 │   │                 │
        │ Gemini (base)   │   │ Builtin tools   │
        │ future models   │   │ MCP             │
        └─────────────────┘   └─────────────────┘
```

The most important invariant is:

> **ACP is an adapter, not the agent runtime.**

Zed remains the primary interactive client. The runtime must not become coupled to Zed-specific presentation or ACP transport details.

## Release artifact

The project intentionally releases a single executable:

```text
gemini-acp
```

It is the ACP agent used by Zed. Development utilities and provider implementation modules are not shipped as separate release binaries unless they acquire an independent product role.

## Crate responsibilities

### `acp-adaptor`

The ACP-facing boundary.

Responsibilities:

- ACP `initialize` and capability negotiation;
- session lifecycle requests;
- prompt decoding and ACP presentation;
- permission and interaction handling;
- conversion between ACP configuration and runtime configuration;
- projection of semantic runtime/model events into ACP updates;
- protocol-level validation and errors.

It must not own the core agent loop or provider implementations.

### `agent-runtime`

The provider-neutral semantic core.

Responsibilities:

- agent turn orchestration;
- session state and lifecycle;
- context construction and compaction;
- canonical runtime events;
- cancellation and turn ownership;
- persistence and replay semantics;
- provider-independent tool/model orchestration.

The runtime depends only on provider contracts. It does not directly depend on Gemini, ACP, or the concrete MCP implementation.

### `llm-provider`

The model-facing provider boundary.

Gemini is the current implementation.

Provider-specific responsibilities include:

- authentication and cookie/session handling;
- request construction;
- model selection;
- thinking/reasoning configuration;
- provider-native streaming;
- provider response parsing;
- provider-specific tool-call syntax;
- usage reporting;
- provider-specific errors.

Gemini output is normalized into canonical model events before the ACP presentation layer consumes it.

### `tools-provider`

The runtime tool capability implementation.

Current categories:

```text
Tool Provider
├── Builtin
│   ├── filesystem
│   ├── shell
│   ├── search
│   └── interactive tools
│
└── MCP
    ├── stdio/process transport
    └── HTTP transport
```

The runtime sees a generic tool-provider contract. MCP-specific configuration and transport implementation remain inside the tool provider boundary.

## Canonical model contract

The provider boundary no longer exposes raw string chunks as the semantic LLM contract.

The intended flow is:

```text
Gemini wire stream
      ↓
Gemini provider
      ↓
ModelRequest / ModelEvent
      ↓
Agent Runtime
      ↓
ACP projection
```

The canonical model stream includes semantic categories such as:

```text
TextDelta
ReasoningDelta
ToolCall
Usage
```

This is important because transport chunks are provider implementation details. The runtime should reason about model semantics, not Gemini framing.

A future provider such as OpenAI, DeepSeek, Claude, or another implementation should target the same runtime-facing model contract rather than introduce another provider-shaped stream type.

## Tool configuration boundary

MCP is a **tool capability**, not an LLM feature.

When Zed sends `mcpServers[]` through ACP, the adapter validates and normalizes the protocol representation into the generic runtime `ToolServerConfig` contract. The concrete `tools-provider` then maps that generic configuration to its MCP implementation.

```text
ACP McpServer
      ↓
ACP Adapter normalization
      ↓
ToolServerConfig
      ↓
Tool Provider
      ↓
MCP transport / builtin registry
```

This keeps MCP types and transport details from leaking into the core runtime contract.

Important invariants:

- preserve the complete Zed-provided server list;
- preserve deterministic server and tool identity;
- isolate independent MCP discovery failures where possible;
- keep transport details out of `agent-runtime`;
- keep MCP out of the LLM provider boundary;
- treat tool results as opaque data at the model/protocol presentation boundary.

## Semantic lifecycle

The project already contains semantic stream and lifecycle hardening that is considered foundational architecture, not temporary plumbing.

The runtime tracks canonical lifecycle events independently of ACP presentation, including:

```text
turn started
assistant started
assistant delta
thinking started / delta / completed
tool call requested
permission requested
tool execution started
tool result received
turn completed
turn failed
turn cancelled
```

Tool identities are semantic and scoped to a turn, rather than trusting a provider's stream-local IDs. This prevents provider-local ID reuse from corrupting the runtime lifecycle.

The ACP adapter then projects these semantics into Zed/ACP notifications.

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
      └── Tool system
```

The second shape would couple protocol behavior, agent orchestration, and a single model provider, making future providers unnecessarily expensive to add.

## Session and persistence model

Sessions remain a first-class runtime concern.

The durable session state is the source of truth for:

- model history;
- ACP replay;
- tool history;
- titles and metadata;
- turn ownership and cancellation state;
- future UI projections.

The direction is to keep durable facts separate from transient streaming notifications so that replay and live execution remain consistent.

## Migration status

The provider-neutral architecture has progressed beyond a physical crate split into actual contract hardening.

### Completed

- four principal crates established: `acp-adaptor`, `agent-runtime`, `llm-provider`, `tools-provider`;
- runtime/provider construction is dependency-injected;
- `agent-runtime` no longer directly depends on ACP or Gemini implementations;
- generic `LlmProvider` and `ToolProvider` boundaries are established;
- canonical `ModelRequest` / `ModelEvent` streaming contract is in place;
- runtime tool-server configuration is generic rather than MCP-named;
- ACP MCP configuration is normalized at the adapter boundary;
- historical `gemini_acp_*` compatibility aliases were removed from the ACP adapter;
- semantic stream and tool lifecycle hardening remains preserved;
- workspace validation is green with `cargo test --workspace --all-targets`.

## Validation

The baseline validation command for this architecture is:

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets
./scripts/audit-provider-neutral.sh
```

The provider-neutral audit is intentionally kept in the repository to guard the architecture against accidental re-coupling.

## Current direction

The next architectural work should focus on **consolidation rather than another crate split**.

Priority areas are:

1. continue moving remaining agent-loop semantics out of the ACP adapter;
2. strengthen canonical model/tool contracts where generic JSON or provider-shaped data remains;
3. keep provider-specific parsing and transport code inside provider crates;
4. preserve semantic lifecycle and replay invariants while adding additional providers;
5. validate future providers against the same runtime contract rather than branching the runtime around provider-specific behavior.

Gemini remains the baseline/reference provider while these boundaries mature.

## Non-goals

This architecture does not currently aim to:

- replace Zed;
- replace ACP;
- depend on DeepSeek Harness;
- reproduce Cordis;
- introduce multiple providers before the provider contracts are stable;
- move MCP into the LLM provider;
- make ACP handlers responsible for the core agent loop.

## Success criteria

The architecture is successful when:

1. Zed continues to operate through ACP without behavioral regressions.
2. Gemini remains fully functional through the provider boundary.
3. Builtin and MCP tools share the same runtime tool abstraction.
4. ACP handlers contain protocol/presentation concerns rather than the core agent loop.
5. Provider-specific parsing is isolated from `agent-runtime`.
6. Session state can be replayed and projected consistently.
7. Adding a second LLM provider does not require redesigning ACP, sessions, or MCP.

## Reference

DeepSeek Harness is used only as a reference for useful architectural ideas such as explicit seams, composable providers, lifecycle-oriented events, and separation between transport adapters and the agent harness. The implementation and architecture of `ny-gemini-acp` remain independently designed around ACP/Zed compatibility and the Rust workspace.
