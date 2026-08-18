# Phase 0 — Baseline réelle Zed

## Purpose

This document records the observed behavior of `ny-gemini-acp` when used as a Zed External Agent. It is an empirical baseline, not a claim of theoretical ACP compliance.

## Rules

- Test the exact binary used by Zed.
- Record the Zed version, agent version/commit, workspace, and relevant configuration.
- `PASS` requires real Zed evidence.
- `FAIL` requires a reproducible deviation from the expected contract.
- `UNOBSERVED` is never treated as `PASS`.
- Every carried-forward `FAIL` becomes a regression target or an explicit limitation.

## Status vocabulary

| Status | Meaning |
|---|---|
| `PASS` | Reproduced successfully in real Zed and observed in ACP logs/UI. |
| `FAIL` | Reproduced and violates the expected contract. |
| `BLOCKED` | Environment prevented execution. |
| `UNOBSERVED` | Not yet exercised. |
| `N/A` | Not applicable. |

## Observed environment

Two real ACP captures were supplied from Zed `1.13.2+stable` against agent version `0.2.2` and ACP protocol version `1`.

The captures used fresh and loaded sessions in the following workspaces:

- `.../ny-gemini-acp`
- `.../test/test-workspace`

Zed forwarded MCP server definitions including `mcp-libre` and `mcp-server-playwright` in `session/new`.

## Real observations

### ACP handshake and configuration

```text
initialize
  -> initialize response
session/new or session/load
  -> configuration options
session/set_config_option(model)
session/set_config_option(think)
  -> config_option_update
session/prompt
```

Observed as working:

- Zed launches and communicates with `gemini-acp` over ACP stdio.
- Agent identity is `gemini-acp`, title `Gemini (Web)`, version `0.2.2`.
- Zed accepts the advertised capabilities.
- Session creation and session loading are accepted.
- `model`, `think`, and `tools_enabled` configuration negotiation succeeds.

### Assistant streaming

Real prompts demonstrated:

- exact one-line output (`Bonjour`);
- multi-line output delivered in multiple `agent_message_chunk` notifications with a stable `messageId`;
- normal Markdown with Rust fences;
- quotes, ellipsis, accented characters, and Japanese text;
- normal completion with `stopReason=end_turn`.

These are `PASS` for the ACP presentation boundary. They do not prove arbitrary raw Gemini chunk-boundary invariance because Zed ACP logs expose normalized agent notifications, not every upstream Gemini chunk.

### Tool execution

A real glob tool completed through:

```text
tool_call
  -> tool_call_update(in_progress)
  -> tool_call_update(completed)
  -> assistant continuation
```

A real shell execution also exercised:

```text
session/request_permission
  -> permission selected
terminal/create
  -> terminal/wait_for_exit
  -> terminal/output
  -> terminal/release
  -> tool_call_update(completed)
```

A real file write created `example.md` containing Markdown, quotes, triple backticks, Python code, and Rust code. A subsequent file read returned that content intact, and the assistant re-presented the Markdown/code content through multiple ACP chunks. This is the strongest current real-Zed evidence for ordinary tool-result content integrity.

### Real failures observed

#### ZED-061 — forwarded MCP not wired

Zed supplied MCP servers in `session/new`, and the agent logged:

```text
session/new received mcp_servers, but Gemini ACP does not wire them yet
```

This remains a real `FAIL` until forwarded MCP servers are wired or explicitly rejected/documented by contract.

#### ZED-026 / lifecycle defect — repeated tool round

During a `glob` followed by a second tool round, the runtime emitted:

```text
rejected invalid semantic event transition
error=tool call gemini_call_0 was already requested
```

followed by:

```text
permission_requested for tool gemini_call_0 is invalid from state Terminal
tool_execution_started for tool gemini_call_0 is invalid from state Terminal
tool_result_received for tool gemini_call_0 is invalid from state Terminal
```

The ACP UI nevertheless rendered the shell tool and the final assistant response completed. Therefore this is an internal semantic lifecycle failure hidden behind functionally successful Zed output.

The expected ordering is:

```text
ToolCall
  -> Permission
  -> Execution
  -> Result
  -> Completed
```

not:

```text
ToolCall
  -> Terminal
  -> Permission / Execution / Result
```

This failure is the primary Phase 1 entry defect.

## Baseline matrix

| ID | Scenario | Current status | Evidence |
|---|---|---|---|
| ZED-001 | Agent launch and ACP stdio | `PASS` | initialize + subsequent ACP traffic |
| ZED-002 | Agent identity/version | `PASS` | `gemini-acp` / `Gemini (Web)` / `0.2.2` |
| ZED-003 | Capabilities accepted | `PASS` | Zed proceeds to session operations |
| ZED-004 | Session creation | `PASS` | session/new response observed |
| ZED-005 | Second independent session | `UNOBSERVED` | not isolated |
| ZED-010 | Plain assistant text | `PASS` | coherent agent_message_chunk |
| ZED-011 | Multi-chunk assistant stream | `PASS` | multiple chunks, stable messageId, end_turn |
| ZED-012 | Assistant marker injection | `UNOBSERVED` | not targeted |
| ZED-013 | User marker injection | `UNOBSERVED` | not targeted |
| ZED-014 | Normal Markdown fence preserved | `PASS` | real Rust/Python fences survived |
| ZED-020 | Single tool call | `PASS` | glob/tool lifecycle rendered |
| ZED-021 | Tool result with quotes | `PASS` | file_write/file_read fixture contains quotes |
| ZED-022 | Tool result with Unicode/ellipsis | `PASS` for observed assistant path | explicit Unicode response and Unicode file content observed; adversarial tool-result ellipsis still needs isolation |
| ZED-023 | Tool result with ``` | `PASS` for file_read fixture | triple backticks survived file write/read and assistant presentation |
| ZED-024 | Tool result with `'''` | `UNOBSERVED` | not isolated |
| ZED-025 | Tool result with `[Assistant]:` | `UNOBSERVED` | not isolated |
| ZED-026 | Multiple/consecutive tools | `FAIL` | invalid semantic event transitions during second tool round |
| ZED-030 | Tool opening fence split | `UNOBSERVED` | raw Gemini chunking unavailable in Zed ACP log |
| ZED-031 | Closing fence split | `UNOBSERVED` | raw Gemini chunking unavailable |
| ZED-032 | Tool-result prefix split | `UNOBSERVED` | raw Gemini chunking unavailable |
| ZED-033 | Assistant marker split | `UNOBSERVED` | raw Gemini chunking unavailable |
| ZED-034 | UTF-8 boundary adversarial test | `UNOBSERVED` | only valid normalized chunks observed |
| ZED-040 | Normal completion | `PASS` | end_turn observed |
| ZED-041 | Cancellation during active turn | `UNOBSERVED` | not exercised |
| ZED-042 | Idle cancellation | `UNOBSERVED` | not exercised |
| ZED-043 | Runtime failure | `UNOBSERVED` | no isolated failure scenario |
| ZED-044 | Process restart | `UNOBSERVED` | not exercised |
| ZED-050 | Session list | `UNOBSERVED` | not exercised from Zed |
| ZED-051 | Session load | `PASS` | session/load observed |
| ZED-052 | Session resume | `UNOBSERVED` | not isolated |
| ZED-053 | Session fork | `UNOBSERVED` | capability advertised, not exercised |
| ZED-054 | Close/delete | `UNOBSERVED` | not exercised |
| ZED-060 | No MCP configured | `UNOBSERVED` | captures forwarded MCP |
| ZED-061 | Forwarded MCP wired | `FAIL` | servers received, explicitly not wired |
| ZED-062 | Adversarial MCP result | `UNOBSERVED` | not exercised |
| ZED-063 | MCP error lifecycle | `UNOBSERVED` | not exercised |

## Phase 0 conclusion

The real Zed baseline has already proved the core ACP path, configuration negotiation, assistant streaming, permission flow, shell/terminal integration, file write/read, and ordinary Markdown/code preservation.

Phase 0 is **not complete** because:

1. the repeated-tool semantic lifecycle defect is reproducible;
2. forwarded MCP remains unwired;
3. adversarial protocol-like tool-result content is not fully exercised;
4. cancellation and session lifecycle scenarios remain incomplete.

## Entry to Phase 1

Phase 1 should first fix and regression-test the repeated-tool lifecycle defect before further protocol/filter hardening. The Zed baseline should then be rerun to verify that the runtime no longer emits invalid semantic transition errors during multi-round tool use.
