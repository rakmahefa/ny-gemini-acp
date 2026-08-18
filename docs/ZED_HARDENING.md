# Zed Integration Hardening Roadmap

## Purpose

This document turns the real Zed evidence from `docs/phase-0/`, `docs/phase-1/`, and `docs/phase-2/` into a concrete hardening sequence for `ny-gemini-acp`.

The priority is to remove deterministic semantic/lifecycle defects before expanding protocol/content hardening, while preserving the existing ACP behavior already observed in Zed.

## Current state

The current Zed evidence demonstrates that the integration can:

- complete ACP initialization and session negotiation;
- stream assistant messages through Zed;
- negotiate `model`, `think`, and `tools_enabled`;
- execute shell, glob, read, and write tools;
- complete permission requests and terminal operations;
- preserve ordinary Markdown, code fences, quotes, Unicode, and tool-result content;
- handle adversarial file content that contains protocol-looking text in several scenarios.

The same evidence also exposes three hardening targets:

1. repeated tool rounds can reuse a semantic tool identity and produce invalid transitions from `Terminal`;
2. adversarial streamed content can trigger the stream contract fail-closed path, proving protection exists but also exposing an imperfect presentation/error path;
3. one Phase 2 FollowUp encapsulation path failed once under the thinking-model test and requires a dedicated causal trace.

Forwarded MCP servers are also received from Zed but are not wired into Gemini ACP yet.

## Priority order

### H1 — Semantic tool identity and lifecycle isolation

**Priority: P0**

Observed evidence includes:

```text
tool call gemini_call_0 was already requested
tool_execution_started ... invalid from state Terminal
tool_result_received ... invalid from state Terminal
```

Hardening actions:

- give every semantic tool invocation a unique identity scoped to the current turn/round;
- separate upstream Gemini tool identity from ACP presentation identity;
- model repeated rounds as new semantic tool invocations rather than reusing a terminalized identity;
- make `ToolCall -> Permission -> Execution -> Result -> Completed/Failed` explicit and monotonic;
- ensure a terminal outer turn cannot receive subsequent tool events;
- add regression tests for two and three consecutive tool rounds;
- test multiple tools in one round plus another tool in the next round;
- make illegal semantic transitions deterministic, observable, and non-corrupting.

**Exit criterion:** no invalid semantic transition appears in Zed ACP logs during repeated or multi-tool rounds.

### H2 — FollowUp encapsulation hardening

**Priority: P0**

Phase 2 produced one observed FollowUp encapsulation failure while using the thinking configuration. This must be isolated independently from ordinary tool-result filtering.

Hardening actions:

- trace `<FollowUp ...>` from raw stream parsing through normalization, permission request, selected outcome, `Role::User` injection, and the next internal Gemini round;
- verify FollowUp never reserves a second outer turn;
- verify FollowUp cannot race cancellation or terminalization of its containing turn;
- guarantee each FollowUp action receives a unique identity and cannot collide with a streamed tool identity;
- add tests for selected, rejected, malformed, split, repeated, and multiple FollowUp actions;
- add a regression test specifically for the observed thinking-model failure once its causal sequence is isolated.

**Exit criterion:** FollowUp succeeds deterministically or fails closed without corrupting the containing turn.

### H3 — Stream/content integrity hardening

**Priority: P0**

Phase 2 already demonstrates that adversarial content can trigger:

```text
semantic stream contract violation
protocol leaked to assistant output
...
dropping unsafe delta
```

The current behavior is useful because the unsafe delta is rejected. The next goal is to make the protection precise and user-safe rather than merely fail-closed.

Hardening actions:

- separate protocol detection from ordinary assistant text parsing;
- guarantee tool-result data is treated as data even when it contains `[Assistant]`, `[User]`, `[Tool result]`, fences, quotes, XML-like tags, or JSON-looking payloads;
- preserve safe surrounding assistant text when one delta is rejected;
- avoid converting a single unsafe delta into a noisy internal-error assistant message unless required by the ACP contract;
- test marker/fence boundaries across streaming chunks;
- test UTF-8 boundaries and embedded control characters;
- verify repeated and nested protocol-like strings inside tool results;
- add differential tests for normalized content versus raw model chunks.

**Exit criterion:** no protocol-looking tool data changes semantic role, and unsafe assistant deltas are rejected without damaging neighboring safe content.

### H4 — MCP forwarding

**Priority: P1**

Zed visibly forwards `mcpServers` during `session/new` and `session/load`, while the agent currently reports that Gemini ACP does not wire them yet.

Hardening actions:

- define an explicit MCP ownership boundary between Zed ACP and Gemini ACP;
- validate and normalize forwarded server definitions;
- wire supported servers into the Gemini tool/session layer;
- define unsupported transports and malformed configurations explicitly;
- add MCP startup, error, tool-call, tool-result, timeout, and shutdown tests;
- add adversarial MCP result-content tests after the generic content-integrity layer is hardened.

**Exit criterion:** every forwarded MCP configuration is either supported end-to-end or rejected with an explicit stable ACP error.

### H5 — Cancellation and turn shutdown

**Priority: P1**

Cancellation is still under-observed in real Zed captures.

Hardening actions:

- test cancellation during assistant streaming;
- test cancellation during permission waiting;
- test cancellation during terminal execution;
- test cancellation during FollowUp permission waiting;
- release turn ownership exactly once;
- reject post-cancel semantic events cleanly;
- ensure cancellation cannot leak a stale tool identity into the next round.

**Exit criterion:** cancellation leaves the session in a consistent idle/terminal state with no stale active turn.

### H6 — Session lifecycle completeness

**Priority: P1**

Observed `session/load` support is not enough to certify the complete lifecycle.

Hardening actions:

- exercise list, load, resume, fork, close, and delete from Zed;
- test forked sessions with independent tool identities;
- test resume after tool completion and after tool failure;
- test process restart followed by session load/resume;
- verify turn identity and semantic state are never copied incorrectly across sessions.

**Exit criterion:** all advertised session capabilities have real Zed evidence or are explicitly removed from the advertised capability set.

## Required regression matrix

| Area | Required regression |
|---|---|
| Tool identity | same tool name, different invocations across rounds |
| Tool lifecycle | permission → execution → result → terminal ordering |
| Multi-tool | 2+ tools in one round |
| Multi-round | tool round → assistant → next tool round |
| FollowUp | selected/rejected/multiple/split marker |
| Streaming | marker/fence split across chunks |
| Content integrity | protocol-like strings inside file/tool results |
| Unicode | multibyte characters at stream boundaries |
| Cancellation | cancel at every waiting/executing boundary |
| MCP | forwarded config, tool call/result, failure |
| Sessions | load/resume/fork/close/delete |

## Hardening order

```text
P0.1 Tool identity / lifecycle isolation
        ↓
P0.2 FollowUp causal fix
        ↓
P0.3 Stream/content integrity refinement
        ↓
P1.1 Cancellation hardening
        ↓
P1.2 Session lifecycle hardening
        ↓
P1.3 MCP forwarding
        ↓
Full Zed regression campaign
```

The sequence deliberately keeps semantic lifecycle isolation ahead of broad MCP work. The Phase 2 evidence shows that richer model behavior can exercise multiple layers simultaneously; the runtime must therefore have stable turn/tool invariants before final compatibility claims are made.

## Evidence policy

- Raw ACP captures remain immutable evidence under the relevant phase.
- `ACP_LOG/parts/` is the analysis representation and must preserve event order.
- A fix is not considered validated until the same scenario is reproduced successfully in real Zed.
- Every historical `FAIL` becomes either a regression target or an explicit documented limitation.
