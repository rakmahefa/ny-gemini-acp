# ny-gemini-acp

A Rust-based Agent Client Protocol (ACP) agent designed first for **Zed**, with Gemini as the initial LLM provider and MCP as the primary external tool integration.

The project is evolving toward a provider-oriented **agent runtime** architecture. The goal is to keep Zed/ACP compatibility stable while making the agent loop, model integration, tools, sessions, and future providers independently replaceable.

> **Architectural reference:** DeepSeek Harness is used as an architectural reference only. This project does not depend on or attempt to reproduce DeepSeek Harness or Cordis.

## Direction

The long-term architecture is:

```text
                           Zed
                            │
                           ACP
                            │
                    ┌───────▼────────┐
                    │   ACP Adapter  │
                    │                │
                    │ protocol ↔    │
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

The most important boundary is that **ACP is an adapter, not the agent runtime**.

Zed remains the primary interactive client. The runtime must not become coupled to Zed-specific presentation or ACP transport details.

## Architectural layers

### 1. Zed

Zed is the primary host and user interface.

Zed owns the editor experience: workspace, transcript presentation, approvals, model/configuration controls, and ACP transport lifecycle.

`ny-gemini-acp` must continue to behave as a first-class ACP agent for Zed.

The architecture must therefore preserve the ACP contract even while the internal runtime evolves.

### 2. ACP Adapter

The ACP Adapter translates between the ACP protocol and the internal Agent Runtime.

Its responsibilities are deliberately narrow:

- ACP `initialize` and capability negotiation;
- session lifecycle requests;
- prompt input decoding;
- ACP permission and interaction requests;
- conversion of runtime events into ACP session updates;
- conversion of ACP tool/server configuration into runtime configuration;
- protocol-level validation and errors.

The ACP Adapter should **not** own the agent loop, model-specific parsing, MCP execution, or persistent business logic.

The intended flow is:

```text
ACP request
    ↓
ACP Adapter
    ↓
Runtime command / input
    ↓
Agent Runtime
    ↓
Runtime events
    ↓
ACP Adapter
    ↓
ACP notification / response
```

This makes it possible to add another frontend later without changing the core agent loop.

### 3. Agent Runtime

The Agent Runtime is the center of the system.

It owns the semantics of an agent turn independently of ACP or any particular LLM provider.

Core responsibilities include:

- agent loop and multi-step turns;
- session state and lifecycle;
- context construction and compaction;
- canonical model/agent events;
- tool dispatch;
- permission policy;
- cancellation and turn lifecycle;
- persistence and replay semantics;
- provider-independent orchestration.

The runtime should evolve toward a clear distinction between **commands/inputs**, **durable session facts**, and **live runtime events**.

A long-term target is a canonical event vocabulary such as:

```text
turn started
user message
assistant delta
thinking delta
model tool call
tool result
follow-up request
turn completed
turn failed
turn cancelled
```

Provider-specific streams should be normalized into this vocabulary before the ACP layer sees them.

### 4. LLM Provider

An LLM Provider is the model-facing adapter used by the Agent Runtime.

The first and reference implementation is **Gemini**.

The provider boundary should isolate model-specific behavior such as:

- authentication/session handling;
- request construction;
- model selection;
- thinking/reasoning configuration;
- streaming transport;
- provider-specific response parsing;
- provider-specific tool-call syntax;
- usage reporting;
- provider-specific errors.

The runtime should consume a canonical provider interface rather than depending directly on Gemini's wire format.

Conceptually:

```text
Agent Runtime
     │
     ▼
 LLM Provider
     │
     ├── Gemini provider (current)
     └── future providers
```

Gemini remains the baseline during the architectural transition. Adding another provider should eventually require changes primarily inside the provider implementation, not inside session management, MCP, or ACP handlers.

### 5. Tool Provider

A Tool Provider exposes capabilities that the Agent Runtime can make available to the model.

The first categories are:

```text
Tool Provider
├── Builtin
│   ├── filesystem
│   ├── shell
│   ├── search
│   └── interactive tools
│
└── MCP
    ├── stdio
    └── HTTP
```

Tool providers own tool discovery, schemas, execution, and provider-specific transport details.

The Agent Runtime should see a common tool abstraction rather than treating MCP as a special case of the Gemini provider.

This is particularly important for Zed: the `mcpServers` list received from `session/new` is session configuration and should become a deterministic set of session-scoped tool providers.

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

The second shape creates protocol and provider coupling that becomes increasingly expensive once multiple model providers or frontends are introduced.

## Canonical model events

One of the key architectural goals is a provider-neutral event layer.

Gemini currently emits provider-specific streaming data. That data must progressively be normalized before entering the Agent Runtime's semantic lifecycle.

```text
Gemini stream
     ↓
Gemini provider adapter
     ↓
Canonical model events
     ↓
Agent Runtime
     ↓
ACP projection
```

This keeps model-specific parsing out of the ACP adapter and allows future providers to share the same agent lifecycle.

The existing semantic stream and lifecycle hardening work is therefore considered foundational rather than temporary plumbing.

## Session and context direction

The runtime should progressively move toward an event-oriented session model.

The durable session history should be the source of truth from which the runtime can derive:

- model history;
- ACP replay;
- tool history;
- titles and metadata;
- usage/telemetry views;
- future UI projections.

A future session model should distinguish clearly between durable facts and transient streaming notifications.

This follows the same useful architectural principle found in DeepSeek Harness: model-visible state should be reconstructable from the session/event history.

## MCP direction

MCP is a Tool Provider, not an LLM feature.

When Zed sends:

```text
session/new
  └── mcpServers[]
```

the ACP Adapter passes the complete list to the Agent Runtime.

The runtime creates session-scoped MCP providers, discovers their tools, and exposes the resulting definitions through the common Tool Provider interface.

Important invariants:

- preserve the complete Zed-provided server list;
- preserve deterministic server identity and tool identity;
- isolate failures between independent MCP servers where possible;
- keep MCP transport details out of the LLM provider;
- make tool discovery stable before publishing a session registry;
- never silently reinterpret MCP results as model protocol.

## Architectural principles

### Stable outside, evolvable inside

Zed/ACP compatibility is the external contract. Internal implementation may evolve aggressively as long as the observable ACP behavior remains correct.

### Providers are replaceable

Gemini is the baseline provider, not the architecture itself.

### Capabilities are independent

Tools and MCP are capabilities available to the Agent Runtime. They are not properties of a particular model implementation.

### Events carry semantics

Streaming transport chunks are implementation details. The runtime should reason in canonical semantic events.

### Persistence is a first-class concern

Session replay and model context must derive from consistent durable state rather than ad-hoc presentation strings.

### DeepSeek Harness is a reference, not a dependency

We borrow useful ideas such as composable providers, explicit seams, event-driven lifecycle, and separation between the harness and transport adapters. We do not mirror its framework, runtime, or implementation choices.

## Migration strategy

The architecture will be introduced incrementally.

### Phase 1 — Provider boundary

Place the existing Gemini implementation behind an explicit LLM provider interface without changing Zed behavior.

### Phase 2 — Canonical model events

Normalize Gemini streaming output into provider-neutral events. Keep current semantic lifecycle guarantees intact.

### Phase 3 — Tool provider boundary

Unify builtin and MCP tools behind a common capability surface.

### Phase 4 — Runtime extraction

Move turn orchestration, session semantics, context handling, and cancellation toward an ACP-independent Agent Runtime.

### Phase 5 — Additional providers

Only after the provider boundary is stable should additional model providers be introduced.

Gemini remains the **baseline and reference provider throughout these phases**.

## Non-goals

This direction does not currently aim to:

- replace Zed;
- replace ACP;
- depend on DeepSeek Harness;
- reproduce Cordis;
- rewrite the project in TypeScript;
- introduce multiple LLM providers before the Gemini provider boundary is stable.

## Success criteria

The architecture will be considered successful when:

1. Zed continues to operate through ACP without regressions.
2. Gemini remains fully functional through the new provider boundary.
3. MCP tools are available through the same runtime tool abstraction as builtin tools.
4. ACP handlers no longer contain core agent-loop logic.
5. Provider-specific parsing is isolated from the Agent Runtime.
6. Session state can be replayed and projected consistently.
7. Adding a second LLM provider does not require redesigning ACP, sessions, or MCP.

## Current baseline

At the start of this architectural phase:

- **Frontend:** Zed
- **Protocol:** ACP
- **Agent runtime:** existing Rust runtime, being progressively extracted and decoupled
- **LLM provider:** Gemini
- **Tool providers:** builtin + MCP
- **Architectural reference:** DeepSeek Harness
- **Primary objective:** evolve the runtime without breaking Zed compatibility
