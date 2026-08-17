# Phase 0 — Baseline réelle Zed

## Purpose

This document defines the **real Zed baseline** for `ny-gemini-acp` before any further streaming, semantic, or lifecycle hardening.

The goal is not to prove theoretical ACP compliance. The goal is to record what the current release of `ny-gemini-acp` actually does when launched by Zed as an External Agent.

Zed currently integrates external agents through ACP. Custom agents are configured through `agent_servers` in Zed settings, and Zed exposes ACP traffic through `dev: open acp logs`. citeturn101497search0turn101497search1

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

## Test environment

Record these values for every baseline run:

```text
Date:
OS:
Zed version:
Zed build/channel:
ny-gemini-acp commit:
ny-gemini-acp version:
Rust version:
Launch command:
Working directory:
Authentication/configuration source:
GEMINI_ACP_DATA_DIR:
Other relevant environment:
```

On Linux, Zed's user settings are normally stored at `~/.config/zed/settings.json`. citeturn177410search3turn177410search6

## Recommended custom-agent configuration

Zed's custom External Agent configuration uses an `agent_servers` entry with a command, optional arguments, and environment variables. citeturn177410search0

For a locally built release, use the equivalent of:

```json
{
  "agent_servers": {
    "ny-gemini-acp-baseline": {
      "type": "custom",
      "command": "/absolute/path/to/ny-gemini-acp/target/release/gemini-acp",
      "args": [],
      "env": {}
    }
  }
}
```

Do not commit a machine-specific absolute path to the repository.

## Baseline procedure

### 1. Build the exact release binary

```sh
cargo build -p gemini-acp-agent --release
```

The binary used by Zed must be the resulting `target/release/gemini-acp` or an equivalent release artifact.

### 2. Configure Zed

Open Agent Settings and add a custom External Agent, or add the equivalent `agent_servers` entry to the settings file. Zed exposes Custom Agents from the External Agents page. citeturn101497search0turn101497search1

### 3. Open a new external-agent thread

Start a fresh thread from the Agent Panel/Threads Sidebar using the `ny-gemini-acp-baseline` agent.

### 4. Enable ACP logging

Open the Command Palette and run:

```text
Dev: Open ACP Logs
```

Zed documents the ACP log surface as the primary debugging mechanism for External Agents. citeturn101497search0

### 5. Execute the baseline matrix below

Run the tests in order. For every test, record:

- prompt/input;
- visible UI result;
- ACP log observation;
- expected result;
- actual result;
- status;
- notes/reference to a regression test if it fails.

## Baseline matrix

### A. Process and handshake

| ID | Scenario | Expected | Status |
|---|---|---|---|
| ZED-001 | Zed launches `gemini-acp` | Process stays alive and accepts ACP traffic over stdio | `UNOBSERVED` |
| ZED-002 | ACP initialization | Agent identifies itself as `gemini-acp` with the current package version | `UNOBSERVED` |
| ZED-003 | Initialization capabilities | Zed accepts the advertised capabilities without a protocol error | `UNOBSERVED` |
| ZED-004 | First session creation | New thread/session is created and rendered by Zed | `UNOBSERVED` |
| ZED-005 | Second session creation | Independent thread/session can be created | `UNOBSERVED` |

### B. Basic assistant streaming

| ID | Scenario | Expected | Status |
|---|---|---|---|
| ZED-010 | Plain text response | Visible response contains no internal protocol markers | `UNOBSERVED` |
| ZED-011 | Multi-chunk response | Text is complete and ordering is preserved | `UNOBSERVED` |
| ZED-012 | Assistant marker in Gemini output | Internal marker is hidden from Zed UI | `UNOBSERVED` |
| ZED-013 | User marker in Gemini output | Internal marker is hidden from Zed UI | `UNOBSERVED` |
| ZED-014 | Markdown code fence | Normal Markdown fence remains visible | `UNOBSERVED` |

### C. Tool execution

| ID | Scenario | Expected | Status |
|---|---|---|---|
| ZED-020 | One tool call | Zed renders a coherent tool interaction and the assistant continues | `UNOBSERVED` |
| ZED-021 | Tool result contains quotes | Tool result does not corrupt following assistant output | `UNOBSERVED` |
| ZED-022 | Tool result contains `...` or Unicode punctuation | Tool result remains data; assistant output remains intact | `UNOBSERVED` |
| ZED-023 | Tool result contains ``` | Embedded fence is not reinterpreted as a new tool call | `UNOBSERVED` |
| ZED-024 | Tool result contains `'''` | Embedded single-quote fence is not reinterpreted | `UNOBSERVED` |
| ZED-025 | Tool result contains `[Assistant]:` | Embedded marker is not surfaced or treated as a lifecycle transition | `UNOBSERVED` |
| ZED-026 | Multiple consecutive tools | Tool identity/order remains coherent | `UNOBSERVED` |

### D. Streaming boundaries

| ID | Scenario | Expected | Status |
|---|---|---|---|
| ZED-030 | Tool-call opening split across chunks | No protocol leakage | `UNOBSERVED` |
| ZED-031 | Tool-call closing fence split across chunks | No protocol leakage and no dropped assistant text | `UNOBSERVED` |
| ZED-032 | Tool result prefix split across chunks | Result envelope stays hidden | `UNOBSERVED` |
| ZED-033 | Assistant marker split across chunks | Marker is removed without dropping visible text | `UNOBSERVED` |
| ZED-034 | UTF-8 text around chunk boundaries | No corruption of Unicode output | `UNOBSERVED` |

### E. Lifecycle and cancellation

| ID | Scenario | Expected | Status |
|---|---|---|---|
| ZED-040 | Normal completion | Thread reaches a coherent terminal state | `UNOBSERVED` |
| ZED-041 | User cancellation during active turn | Turn stops without later successful completion | `UNOBSERVED` |
| ZED-042 | Cancellation with no active turn | No spurious failure or state corruption | `UNOBSERVED` |
| ZED-043 | Agent/runtime error | Zed receives an actionable failure and thread is terminal | `UNOBSERVED` |
| ZED-044 | Process restart | New Zed thread can reconnect cleanly | `UNOBSERVED` |

### F. Sessions and persistence

| ID | Scenario | Expected | Status |
|---|---|---|---|
| ZED-050 | List sessions | Existing sessions are discoverable | `UNOBSERVED` |
| ZED-051 | Load session | History is rendered coherently | `UNOBSERVED` |
| ZED-052 | Resume session | New prompt continues the expected session | `UNOBSERVED` |
| ZED-053 | Fork session | Forked session is independent when capability is advertised | `UNOBSERVED` |
| ZED-054 | Close/delete session | Session lifecycle remains coherent in Zed | `UNOBSERVED` |

### G. MCP and tool forwarding

Zed can forward configured MCP servers to External Agents over ACP. The agent may also have native MCP configuration. citeturn101497search3

| ID | Scenario | Expected | Status |
|---|---|---|---|
| ZED-060 | No MCP configured | Agent still starts and works normally | `UNOBSERVED` |
| ZED-061 | One forwarded MCP server | Agent discovers/uses it without corrupting ACP output | `UNOBSERVED` |
| ZED-062 | MCP tool result contains protocol-like text | Result is treated as data, not protocol | `UNOBSERVED` |
| ZED-063 | MCP error | Failure remains attributable and does not corrupt lifecycle state | `UNOBSERVED` |

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
