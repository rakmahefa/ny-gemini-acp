# Phase 2 — MCP result integrity

P2-010 is explicitly gated on forwarded MCP support. Phase 0 currently records forwarded MCP as a real `FAIL`, because Zed sends `mcpServers` but the agent does not wire them yet.

Do not classify P2-010 as `PASS` or `FAIL` from prompt behavior until a real forwarded MCP server is active. Keep the evidence separate from ordinary tool-result tests because MCP introduces another transport/content boundary.

When MCP is wired, isolate:

1. MCP tool invocation in real Zed.
2. MCP result containing `[Assistant]:`, `[Tool result]:`, ```tool_call and `'''tool_call`.
3. ACP session/update sequence.
4. Any encapsulation or normalization errors.
5. Final assistant presentation.

Primary acceptance rule: MCP output remains data and never becomes protocol semantics.
