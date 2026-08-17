# Architecture Skill

## Purpose

This skill defines the architectural contract for `ny-gemini-acp` and provides a repeatable way to design, modify, review, and validate changes without weakening ACP, streaming, MCP, or runtime semantics.

It is intended for coding agents and maintainers working on the repository. It is normative for architecture decisions and diagnostic guidance, but it does not replace Rust type safety, protocol specifications, or the test suite.

## Repository Mission

`ny-gemini-acp` is a Rust implementation that translates Gemini agent behavior into ACP-compatible sessions while integrating runtime services and MCP tool infrastructure.

The architecture must preserve four properties at all times:

1. **Semantic integrity** — Gemini stream meaning must not be changed accidentally by transport, filtering, or presentation code.
2. **Protocol integrity** — ACP-visible content must never contain internal protocol envelopes unless explicitly represented as user content.
3. **Lifecycle integrity** — thinking, assistant, tool-call, tool-result, completion, cancellation, and failure states must remain ordered and coherent.
4. **Module integrity** — responsibilities must stay isolated so protocol, semantic interpretation, runtime execution, and presentation can evolve independently.

## Architectural Layers

The repository is organized as a dependency-oriented pipeline:

```text
Gemini / external sources
        |
        v
+-------------------------+
| Acquisition / Transport |
+-------------------------+
        |
        v
+-------------------------+
| Semantic Interpretation  |
| - thought lifecycle      |
| - tool-call detection    |
| - stream contract        |
+-------------------------+
        |
        v
+-------------------------+
| Runtime / Tool Services  |
| - execution              |
| - catalog                |
| - MCP lifecycle          |
+-------------------------+
        |
        v
+-------------------------+
| ACP Presentation         |
| - events                 |
| - notifications          |
| - errors                 |
+-------------------------+
```

The dependency rule is directional:

```text
presentation -> semantic -> acquisition
runtime services <-> semantic contracts
configuration -> all layers through explicit APIs
```

A lower layer must not import a higher-level presentation concern merely to simplify implementation.

## Workspace Boundaries

The workspace currently contains four crates:

- `gemini-acp-config`: configuration and static environment-facing definitions.
- `gemini-acp-runtime`: execution-oriented services and shared runtime infrastructure.
- `gemini-acp-agent`: Gemini-facing agent logic, prompt/stream semantics, lifecycle handling, and ACP-facing orchestration.
- `gemini-acp-encaps`: encapsulation boundary and protocol integration helpers.

When introducing a feature, first identify the owning crate. Avoid adding a new dependency merely because a nearby crate already contains a convenient helper.

### Ownership rule

A piece of code belongs in the crate that owns its **semantic responsibility**, not the crate that happens to call it most frequently.

For example:

- parsing a Gemini tool-call envelope belongs with Gemini stream semantics;
- executing a parsed tool call belongs with runtime tool infrastructure;
- serializing an ACP notification belongs at the ACP presentation edge;
- reading configuration belongs in the configuration layer.

## Prompt and Stream Architecture

The prompt subsystem is deliberately modular. Relevant responsibilities include:

```text
prompt/
├── build.rs
├── content.rs
├── error.rs
├── follow_up.rs
├── notify.rs
├── protocol.rs
├── protocol_filter.rs
├── stream.rs
├── stream_contract.rs
└── title.rs
```

### `protocol.rs`

Defines protocol markers and constants. It is the single source of truth for internal marker strings.

Rules:

- do not duplicate marker literals in other modules;
- use named constants when matching or validating protocol syntax;
- changing a marker requires updating protocol-focused tests.

### `protocol_filter.rs`

Owns the ACP presentation barrier for internal protocol syntax.

It is incremental and stateful because stream chunks are arbitrary.

It must:

- buffer partial protocol prefixes;
- buffer partial closing fences;
- remove internal tool-result envelopes;
- remove internal tool-call envelopes;
- remove assistant/user protocol labels;
- preserve ordinary Markdown and source-language syntax;
- hide unclosed internal protocol blocks at EOF.

It must **not** parse tool payload semantics. Semantic parsing belongs to the detector/contract layer.

### `stream_contract.rs`

Owns the semantic contract between raw protocol detection and ACP presentation.

It must be the single coordination point when two stream machines consume the same response bytes.

Current contract responsibilities:

- feed raw content into the tool detector;
- feed raw content into the presentation filter;
- validate tool-call identity;
- reject malformed tool-call names or IDs;
- re-key duplicate stream-local tool-call IDs;
- reject protocol syntax that escapes the presentation barrier.

A new stream consumer should not independently interpret the same raw stream without an explicit contract review.

### `stream.rs`

Owns orchestration, not protocol parsing.

Its responsibilities are:

- receive stream items;
- coordinate cancellation;
- bridge thought events and response events;
- call the semantic stream contract;
- emit semantic/ACP lifecycle events;
- collect the normalized stream result.

Avoid adding marker-specific logic here.

## Semantic Event Model

The architectural direction for future stream work is a unified semantic event model.

Conceptually:

```text
StreamEvent::ThinkingStarted
StreamEvent::ThinkingDelta(text)
StreamEvent::ThinkingCompleted
StreamEvent::AssistantStarted
StreamEvent::AssistantDelta(text)
StreamEvent::ToolCall(call)
StreamEvent::ToolResult(result)
StreamEvent::AssistantCompleted
StreamEvent::Cancelled
StreamEvent::Failed(error)
```

This model is intentionally independent from ACP wire types.

The semantic layer should answer **what happened**. The presentation layer should answer **how ACP represents it**.

### Event invariants

For a normal successful turn:

```text
AssistantStarted
  -> zero or more Thinking events
  -> zero or more AssistantDelta events
  -> zero or more ToolCall events
  -> optional tool result lifecycle
  -> AssistantCompleted
```

Thinking must not remain active after response completion.

A cancellation must not emit a normal assistant completion.

A failed stream must expose an actionable error without inventing successful completion semantics.

## Tool Architecture

Tool handling has three distinct stages:

```text
Detection -> Validation/Normalization -> Execution
```

### Detection

Converts raw Gemini protocol into `ParsedToolCall`-like semantic data.

Detection must be incremental and chunk-boundary invariant.

### Validation / normalization

Checks:

- non-empty IDs;
- non-empty names;
- valid arguments;
- stream-local uniqueness;
- lifecycle consistency.

If normalization changes identity, the transformation must be deterministic and observable through diagnostics.

### Execution

Runtime code executes validated calls. It must not need to understand how the original Gemini stream was chunked or how protocol markers were encoded.

Tool execution errors are runtime events, not text-filtering events.

## MCP Architecture

MCP infrastructure is intentionally split into modules rather than concentrated in one file.

The conceptual decomposition is:

```text
configuration / descriptors
        |
        v
JSON-RPC protocol helpers
        |
        +--> stdio transport
        +--> HTTP transport
        |
        v
client lifecycle
        |
        v
catalog / discovery lifecycle
        |
        v
result rendering
```

### MCP design rules

- transport code does not own catalog policy;
- discovery code does not own result rendering;
- result rendering does not execute tools;
- protocol helpers do not contain business policy;
- mutable client state must have explicit synchronization semantics;
- asynchronous server/client state should prefer Tokio-aware synchronization where blocking mutexes would cross await points.

A module facade may re-export the public surface, but the facade should remain thin.

## Configuration

Configuration must be represented as explicit typed data.

Prefer:

```text
parse -> validate -> normalize -> consume
```

Avoid repeatedly parsing environment/configuration at arbitrary call sites.

Configuration defaults must be deterministic and testable.

## ACP Presentation Boundary

ACP is an external protocol boundary. Treat ACP types as an adapter target rather than the internal semantic model.

Rules:

- do not leak internal marker syntax to ACP clients;
- do not use ACP serialization as the source of truth for Gemini semantics;
- keep notification helpers small and side-effect oriented;
- map semantic errors to actionable ACP-visible errors at one clear boundary.

The presentation edge is also the appropriate place to enforce final visibility checks.

## Error Architecture

Errors should be layered:

```text
input/configuration error
        |
semantic/protocol error
        |
runtime/tool error
        |
ACP presentation error
```

Do not erase useful context when crossing a layer.

Use typed errors for programmatic branching. Use display messages for users and logs.

When recovering from a semantic stream integrity violation, fail closed: unsafe protocol data must not be forwarded as assistant-visible text.

## State Machines

Any component that depends on chunk order should be considered a state machine, even if implemented as a struct with flags.

State must be explicit enough to answer:

- What state are we in?
- What input transitions the state?
- What output is emitted?
- What happens at EOF?
- What happens on cancellation?
- What happens on malformed input?

For streaming code, every state machine must define behavior for:

1. complete marker in one chunk;
2. marker split across arbitrary chunks;
3. nested or repeated marker-looking payload content;
4. UTF-8 multibyte boundaries;
5. EOF in the middle of a construct;
6. cancellation between chunks;
7. duplicate semantic IDs.

## Chunk-Boundary Contract

All stream processing must satisfy the following invariant:

> Repartitioning an identical byte/Unicode stream into different valid chunks must not change its semantic result.

The canonical test pattern is:

```text
reference = collect(full_stream)
for every valid split:
    actual = collect(left, right)
    assert actual == reference
```

For robust streaming components, extend this to multiple chunks and adversarial boundaries.

## UTF-8 Contract

Never split Rust strings by arbitrary byte index unless the index comes from a valid character boundary.

When testing chunk boundaries, use `char_indices()` or explicitly constructed valid UTF-8 slices.

A stream parser should operate on valid `str` data when its upstream contract already guarantees UTF-8.

If byte-oriented processing is introduced, its encoding boundary must be explicit and tested separately.

## Protocol Marker Safety

Internal markers are potentially ambiguous with normal user content.

Therefore marker detection should use the narrowest context possible:

- line-start semantics where appropriate;
- exact marker constants;
- explicit fence states;
- no global substring deletion.

Never implement protocol filtering as a broad replacement such as:

```text
input.replace("```", "")
```

because ordinary Markdown is valid user-visible content.

## Security / Fail-Closed Rules

When protocol interpretation becomes ambiguous, prefer dropping unsafe protocol output rather than exposing it as assistant content.

Examples:

- malformed tool-call identity -> reject the semantic call;
- protocol marker escapes the filter -> report a stream integrity failure;
- incomplete internal tool envelope at EOF -> keep it hidden;
- duplicate call ID -> deterministic re-key or reject according to the contract;
- impossible lifecycle transition -> fail the semantic contract.

Do not silently convert malformed protocol into ordinary assistant prose.

## Testing Strategy

Tests should be organized by invariant rather than implementation convenience.

### Unit tests

Test each state machine independently:

- protocol markers;
- opening and closing fences;
- tool-call parsing;
- stream normalization;
- event emission;
- configuration validation.

### Contract tests

Test interactions between modules:

- detector + filter;
- thought stream + response stream;
- semantic contract + runtime tool execution;
- semantic events + ACP presentation.

### Property-style tests

Where practical, test:

- arbitrary chunk boundaries;
- duplicate IDs;
- Unicode boundaries;
- marker-like payloads;
- empty chunks;
- repeated markers;
- EOF in every parser state.

### Regression tests

Every previously observed streaming failure should become a permanent test.

The test name should capture the violated invariant, not just the historical bug.

Prefer:

```text
arbitrary_chunk_boundaries_do_not_change_result
```

over:

```text
fix_bug_71
```

## Observability

Logs should explain semantic transitions without dumping sensitive tool payloads indiscriminately.

Recommended fields:

- session ID;
- message ID;
- tool name;
- original and replacement tool-call IDs when re-keying;
- lifecycle transition;
- contract violation category.

Avoid logging entire prompt or tool-result payloads by default.

## Performance Rules

Streaming code must remain incremental.

Avoid:

- repeatedly cloning the full accumulated stream;
- quadratic string concatenation in hot loops when buffers can be reused;
- parsing the same payload multiple times without justification;
- blocking mutexes across async work;
- unnecessary serialization/deserialization cycles.

Do not sacrifice semantic correctness for micro-optimizations in protocol handling.

Measure before introducing complex buffering strategies.

## API Design Rules

Public APIs should expose semantic types, not internal parser state.

Good:

```text
ParsedToolCall
StreamResult
StreamOutcome
```

Avoid exposing:

```text
ProtocolFilterState
ToolCallFenceState
internal marker buffers
```

unless another crate genuinely needs them as stable concepts.

Crate-private (`pub(crate)`) is preferred for implementation-level contracts.

## Refactoring Rules

Before changing a large stream or protocol module:

1. identify its current semantic responsibilities;
2. identify state machines and invariants;
3. locate existing tests that encode those invariants;
4. extract one responsibility at a time;
5. preserve the public behavior at each step;
6. run the complete relevant test surface;
7. only then remove obsolete compatibility code.

A refactor is not complete when a file becomes smaller. It is complete when the dependency graph and semantic ownership become clearer.

## Change Classification

Classify architectural changes before implementation:

### Type A — Local implementation

No semantic or dependency boundary changes.

Examples:

- helper extraction;
- naming cleanup;
- internal test improvements.

### Type B — Module boundary

Responsibility moves between modules within a crate.

Requires:

- ownership review;
- module API review;
- regression tests.

### Type C — Crate boundary

A capability moves across workspace crates.

Requires:

- dependency graph review;
- public API review;
- cycle analysis;
- integration tests.

### Type D — Semantic contract

Changes what a stream or ACP session means.

Requires:

- explicit invariant documentation;
- contract tests;
- lifecycle tests;
- regression coverage;
- review of failure and cancellation semantics.

### Type E — Protocol compatibility

Changes externally visible Gemini/ACP/MCP behavior.

Requires:

- protocol fixture tests;
- compatibility assessment;
- documentation update;
- explicit migration notes when behavior changes intentionally.

## Definition of Done

An architectural change is complete only when all applicable conditions are true:

- ownership is clear;
- module boundaries are coherent;
- no duplicated protocol constants exist;
- public APIs expose semantic concepts;
- stream behavior is chunk-boundary invariant;
- EOF behavior is tested;
- cancellation behavior is tested where applicable;
- malformed input fails closed;
- duplicate tool identities are handled deterministically;
- ACP-visible output cannot contain internal protocol syntax accidentally;
- runtime execution remains independent from text filtering;
- tests cover the invariant, not only the happy path;
- formatting and lint expectations are satisfied;
- the full relevant workspace test suite passes.

## Architectural Review Checklist

Before approving a change, ask:

### Ownership

- Which layer owns this behavior?
- Is the code living with its semantic owner?
- Does the change introduce an upward dependency?

### Streaming

- What happens when the input is split at every character boundary?
- What happens at EOF?
- What happens if the marker is present inside JSON/string payload content?
- Can ordinary Markdown be preserved?

### Lifecycle

- Is the event order valid?
- Can a state remain active after completion?
- What happens on cancellation?
- What happens on failure?

### Tools

- Can the same tool-call ID appear twice?
- Are malformed IDs/names rejected?
- Is parsing separated from execution?
- Is tool output kept out of assistant text unless explicitly intended?

### ACP

- Is the ACP adapter merely projecting semantics?
- Can protocol syntax leak through?
- Are error messages actionable but not misleading?

### MCP

- Is transport separated from client/discovery policy?
- Is synchronization async-safe?
- Is rendering separate from execution?

### Testing

- Is there a regression test?
- Is there a contract test?
- Is there an adversarial chunk-boundary case?
- Is there a Unicode boundary case where relevant?

## Implementation Workflow for Agents

When an agent receives an architecture-sensitive task:

1. **Map the repository** — inspect workspace members, owning crate, relevant modules, and recent commits.
2. **Identify invariants** — write down current stream, protocol, lifecycle, and identity guarantees before editing.
3. **Choose the smallest correct boundary** — do not refactor unrelated code.
4. **Add or strengthen tests first when behavior is ambiguous.**
5. **Implement one semantic responsibility per module.**
6. **Keep orchestration thin.**
7. **Run formatting, compile checks, unit tests, and integration/contract tests relevant to the changed layer.**
8. **Review the diff for accidental protocol behavior changes.**
9. **Document any new architectural invariant.**
10. **Commit with a message describing the architectural intent.**

## Preferred Evolution Path

The architecture should evolve toward a normalized semantic stream:

```text
Raw Gemini stream
      |
      v
Semantic decoder
      |
      +--> Thinking events
      +--> Assistant events
      +--> ToolCall events
      +--> ToolResult events
      +--> Lifecycle events
      |
      v
Runtime / policy
      |
      v
ACP projection
```

The existing `SemanticStreamContract` is the correct strategic insertion point for this evolution.

The end goal is to make `ProtocolFilter` a pure presentation safeguard, `ToolStreamDetector` a semantic decoder, and `ThoughtStream` one specialized semantic decoder rather than three competing interpretations of the same response.

## Anti-Patterns

Do not introduce:

- marker filtering in `stream.rs`;
- ACP serialization inside semantic parsers;
- runtime tool execution from text filtering code;
- duplicated tool-call parsing in multiple modules;
- global string replacement of protocol markers;
- hidden state transitions driven by logging side effects;
- blocking locks across `.await`;
- tests that assert only one favorable chunk layout;
- error recovery that forwards ambiguous protocol as assistant text.

## Documentation Rule

When a change introduces a new invariant, update this skill or a more specific architecture document in the same change unless the invariant is already fully represented here.

Architecture documentation is part of the implementation contract, not a post-hoc summary.
