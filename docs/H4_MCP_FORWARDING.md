# H4 — MCP Forwarding

## Implementation status

H4 is implemented on `agent/zed-baseline`.

The integration now treats MCP configuration received through ACP session setup as **session-scoped runtime state** instead of global process configuration.

### Ownership boundary

```text
Zed ACP
  │
  ├─ session/new.mcpServers
  ├─ session/load.mcpServers
  ├─ session/resume.mcpServers
  └─ session/fork.mcpServers
        │
        ▼
SessionManager::configure_mcp
        │
        ├─ ACP → internal MCP config normalization
        ├─ transport validation
        ├─ server discovery (`tools/list`)
        ├─ descriptor validation
        └─ session-scoped ToolRegistry
                │
                ▼
        Gemini prompt/tool execution
```

The global `GEMINI_ACP_MCP_SERVERS` configuration remains the fallback for sessions that do not provide forwarded MCP servers. A forwarded configuration replaces that session's fallback registry without changing other sessions.

## Supported transports

- `stdio`: supported end-to-end. ACP requires an absolute command path; environment variables are validated; the process is launched with the persisted ACP session workspace as its working directory.
- `http`: supported end-to-end through the existing MCP HTTP transport, including configured HTTP headers and JSON/SSE responses from that transport.
- legacy `sse`: explicitly rejected. The current runtime does not implement the separate SSE endpoint handshake, so the agent does **not** advertise `mcp_capabilities.sse` and never claims partial support.
- unstable MCP-over-ACP transport: explicitly rejected because the repository does not enable that ACP feature.

## Lifecycle guarantees

MCP configuration is fully built and tool-discovered before it is published into the session registry. A failed reconfiguration therefore cannot publish a partially initialized catalog.

Closing or deleting a session clears its MCP registry. Dropping the registry releases MCP clients and terminates stdio child processes through the existing transport `Drop` implementation.

Forked sessions receive their own MCP registry and therefore cannot reuse another session's remote-tool state.

## Error contract

Malformed or unsupported forwarded MCP configuration is returned as ACP `invalid_params` with stable machine-readable data:

```json
{
  "session_id": "sess_…",
  "error": "MCP configuration rejected",
  "mcp_error": "..."
}
```

Server startup, discovery, protocol, timeout, and remote failures remain represented by the existing `McpError` categories and are surfaced through the same ACP error boundary.

## Validation coverage

The implementation adds regression coverage for:

- stdio command and environment forwarding;
- session `cwd` propagation for stdio;
- invalid relative commands;
- invalid environment variable names and duplicate variables;
- HTTP transport and custom headers;
- case-insensitive duplicate HTTP headers;
- explicit legacy SSE rejection;
- stable ACP MCP configuration errors.

Existing MCP catalog/transport tests continue to cover JSON-RPC validation, response-ID matching, size/time limits, pagination, result rendering, and transport failures.

## Real-Zed exit criterion

The remaining validation step is empirical rather than architectural: reproduce the Zed scenarios that populate `mcpServers` in `session/new` and `session/load`, then confirm that the advertised MCP tools are available to Gemini and execute through the normal ACP tool lifecycle without cross-session leakage.
