# Phase 0 — Baseline réelle Zed

## Purpose

This document defines the **real Zed baseline** for `ny-gemini-acp` before any further streaming, semantic, or lifecycle hardening.

The goal is not to prove theoretical ACP compliance. The goal is to record what the current release of `ny-gemini-acp` actually does when launched by Zed as an External Agent.

## Baseline rules

1. Run the baseline against the exact binary intended for daily Zed use.
2. Record the exact Zed version, agent commit/version, OS, Rust toolchain, and launch command.
3. Do not change production semantics to make the baseline pass.
4. A behavior that is not observed must be recorded as `UNOBSERVED`, never as `PASS`.
5. Every failure becomes a regression test or an explicitly accepted limitation before the next hardening phase.

## Status vocabulary

| Status | Meaning |
|---|---|
| `PASS` | Reproduced successfully in real Zed and observed in the ACP log/UI. |
| `FAIL` | Reproduced in real Zed and the current implementation violates the expected contract. |
| `BLOCKED` | The test could not be run because of an environment prerequisite. |
| `UNOBSERVED` | Not yet executed. This is the default state for a new baseline. |
| `N/A` | Not applicable to the current agent build/configuration. |

## Real baseline run #1

The following results are based on a real Zed ACP log captured from a fresh External Agent thread using the current `agent/zed-baseline` code path.

### Environment evidence

| Field | Observed value |
|---|---|
| Observation timestamp | `2026-08-17T21:06:17Z` onward in ACP stderr/log entries |
| Zed client | `zed` |
| Zed version | `1.13.2+stable` |
| ACP protocol version | `1` |
| Agent name | `gemini-acp` |
| Agent title | `Gemini (Web)` |
| Agent version | `0.2.2` |
| Workspace | `/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/ny-gemini-acp` |
| Prompt | `Bonjour` |
| Observed session ID | `sess_369b48a56e2749c6b8f5bdc05857086c` |
| Observed message ID | `msg_dae0814536c14097b70997e4204687b3` |
| MCP servers forwarded by Zed | `2` |
| MCP handling in agent | Warning: forwarded servers received but not wired yet |

### Evidence summary

The trace demonstrates the following real sequence:

```text
initialize
  -> initialize response
session/new
  -> session/new response
session/set_config_option (model)
session/set_config_option (think)
  -> config_option_update notifications/responses
session/prompt("Bonjour")
  -> session_info_update(title="Bonjour")
  -> agent_message_chunk*
  -> tool_call
  -> tool_call_update(in_progress)
  -> tool_call_update(completed)
  -> agent_message_chunk*
  -> usage_update
  -> session/prompt response(stopReason=end_turn)
```

The tool interaction was visibly represented through ACP `tool_call` and `tool_call_update` notifications rather than being exposed as ordinary assistant text. The streamed assistant message was delivered in multiple `agent_message_chunk` notifications using one stable `messageId`.

### Observed baseline matrix

| ID | Scenario | Status | Evidence |
|---|---|---|---|
| ZED-001 | Zed launches `gemini-acp` and accepts ACP traffic over stdio | `PASS` | `initialize` request received and answered; subsequent session traffic succeeds |
| ZED-002 | ACP initialization identifies agent | `PASS` | response reports `name=gemini-acp`, `title=Gemini (Web)`, `version=0.2.2` |
| ZED-003 | Initialization capabilities accepted by Zed | `PASS` | Zed continues directly to `session/new`; no protocol error observed |
| ZED-004 | First session creation | `PASS` | `session/new` returns `sess_369b48a56e2749c6b8f5bdc05857086c` with modes/config options |
| ZED-005 | Second independent session creation | `UNOBSERVED` | not exercised in this capture |
| ZED-010 | Plain text assistant response | `PASS` | `agent_message_chunk` notifications deliver coherent visible French text |
| ZED-011 | Multi-chunk response | `PASS` | same `messageId` is used across multiple text chunks and final `stopReason=end_turn` is received |
| ZED-012 | Assistant marker hidden | `UNOBSERVED` | no explicit marker injection test |
| ZED-013 | User marker hidden | `UNOBSERVED` | no explicit marker injection test |
| ZED-014 | Normal Markdown fence preserved | `UNOBSERVED` | not isolated as a baseline test |
| ZED-020 | One tool call | `PASS` | `tool_call` → `in_progress` → `completed`, followed by assistant continuation |
| ZED-021 | Tool result containing quotes | `UNOBSERVED` | not isolated |
| ZED-022 | Tool result containing ellipsis/Unicode punctuation | `UNOBSERVED` | not isolated |
| ZED-023 | Tool result containing ``` | `UNOBSERVED` | not isolated in real Zed |
| ZED-024 | Tool result containing `'''` | `UNOBSERVED` | not isolated in real Zed |
| ZED-025 | Tool result containing `[Assistant]:` | `UNOBSERVED` | not isolated in real Zed |
| ZED-026 | Multiple consecutive tools | `UNOBSERVED` | not exercised |
| ZED-030 | Tool-call opening split across chunks | `UNOBSERVED` | ACP log does not expose Gemini raw chunk partitioning |
| ZED-031 | Tool-call closing fence split across chunks | `UNOBSERVED` | raw Gemini boundary not directly observable from this ACP capture |
| ZED-032 | Tool-result prefix split across chunks | `UNOBSERVED` | raw Gemini boundary not directly observable |
| ZED-033 | Assistant marker split across chunks | `UNOBSERVED` | raw Gemini boundary not directly observable |
| ZED-034 | UTF-8 around chunk boundaries | `UNOBSERVED` | no targeted adversarial stream test |
| ZED-040 | Normal completion | `PASS` | terminal `session/prompt` response contains `stopReason=end_turn` |
| ZED-041 | User cancellation during active turn | `UNOBSERVED` | not exercised |
| ZED-042 | Cancellation with no active turn | `UNOBSERVED` | not exercised |
| ZED-043 | Agent/runtime error | `UNOBSERVED` | no failure scenario in this capture |
| ZED-044 | Process restart | `UNOBSERVED` | not exercised |
| ZED-050 | List sessions | `UNOBSERVED` | not exercised from Zed |
| ZED-051 | Load session | `UNOBSERVED` | not exercised from Zed |
| ZED-052 | Resume session | `UNOBSERVED` | not exercised |
| ZED-053 | Fork session | `UNOBSERVED` | capability is advertised but not exercised |
| ZED-054 | Close/delete session | `UNOBSERVED` | not exercised |
| ZED-060 | No MCP configured | `UNOBSERVED` | this run had two forwarded MCP servers |
| ZED-061 | Forwarded MCP server is discovered/used | `FAIL` | Zed forwarded two servers, but agent stderr explicitly reports they are received and **not wired yet** |
| ZED-062 | MCP result contains protocol-like text | `UNOBSERVED` | no real MCP tool result with adversarial content observed |
| ZED-063 | MCP error | `UNOBSERVED` | no MCP execution/error path exercised |

### Initial findings

#### 1. ACP handshake is operational

The real Zed client successfully negotiated protocol version `1`, received the agent capabilities, accepted the `gemini-acp` identity, created a session, changed configuration, submitted a prompt, received streaming updates, and received a terminal `end_turn` response.

This establishes a real-world PASS for the core Zed → ACP transport path.

#### 2. Streaming presentation is operational at the ACP boundary

The capture contains multiple `agent_message_chunk` notifications for a single logical message, followed by `usage_update` and `stopReason=end_turn`. No protocol marker leaked into the observed assistant text.

This is evidence for ZED-010/ZED-011 only. It is **not** proof of arbitrary raw Gemini chunk-boundary invariance, because ACP logs expose the already-normalized agent notifications rather than every upstream Gemini chunk.

#### 3. Tool lifecycle is operational

A real `glob` tool call was represented as:

```text
tool_call(state=pending)
  -> tool_call_update(status=in_progress)
  -> tool_call_update(status=completed)
  -> assistant_message_chunk continuation
```

The observed tool call used ID `gemini_call_0` and completed before the assistant resumed its response.

#### 4. Forwarded MCP is an explicit current limitation

Zed forwarded two MCP server definitions in `session/new`:

- `mcp-libre`
- `mcp-server-playwright`

The agent emitted this stderr warning:

```text
session/new received mcp_servers, but Gemini ACP does not wire them yet
```

Therefore this is a **real observed limitation**, not a hypothetical concern. ZED-061 is marked `FAIL` until forwarded MCP servers are either wired or the expected behavior is intentionally changed and documented.

#### 5. Configuration negotiation is working

Zed successfully negotiated the current configuration options for:

- `model`
- `think`
- `tools_enabled`

The trace shows both `session/set_config_option` responses and matching `config_option_update` notifications.

## Remaining Phase 0 work

The first run does not complete Phase 0 because many adversarial and lifecycle scenarios remain `UNOBSERVED`.

The next real-Zed captures should focus on:

1. a prompt forcing a `file_read`/tool result containing quotes, `…`, triple backticks, triple single quotes, and literal `[Assistant]:` text;
2. multiple consecutive tool calls;
3. explicit cancellation while a tool or model response is active;
4. session reload/resume/fork from Zed;
5. an MCP-only test that distinguishes forwarded-server discovery from the current built-in tool registry.

These tests should be captured in ACP Logs and added here without changing production code first.

## Evidence capture

For every `FAIL`, preserve:

1. the exact prompt;
2. the relevant ACP log excerpt;
3. the visible Zed output;
4. the agent commit SHA;
5. the Zed version;
6. whether the failure reproduces with one chunk or only under streaming boundaries.

Do not paste API keys, cookies, credentials, or complete private tool payloads into repository artifacts.

## Baseline acceptance gate

Phase 0 is complete only when:

- the environment is recorded;
- all applicable matrix entries are `PASS`, `FAIL`, or `N/A` (no unexplained `UNOBSERVED` entries);
- every `FAIL` has a minimal reproduction;
- every accepted failure is explicitly tracked;
- no baseline conclusion is inferred from unit tests alone.

The result of Phase 0 is an empirical contract for subsequent hardening work.

## Current repository-side observations

The current implementation already has several properties that should be verified rather than assumed in real Zed:

- `gemini-acp` is a dedicated stdio ACP binary;
- initialization advertises `gemini-acp` and the package version;
- session `fork` is advertised;
- the prompt path uses `TurnManager` and an interactive per-turn context;
- the stream presentation edge uses an incremental `ProtocolFilter`;
- the semantic stream contract validates tool-call identity and prevents protocol markers from escaping to visible assistant output.

These are **implementation facts**, not Zed baseline results. They remain `UNOBSERVED` until verified through a real Zed thread and ACP logs.
