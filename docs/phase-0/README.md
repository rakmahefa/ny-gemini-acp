# Phase 0 — Baseline réelle Zed

Phase 0 establishes the empirical behavior of `ny-gemini-acp` when used as a Zed External Agent.

## Scope

- ACP handshake and stdio transport
- session creation/load/configuration
- assistant streaming at the ACP boundary
- tool-call rendering and permissions
- file read/write behavior
- lifecycle and cancellation observations
- MCP forwarding observations

## Acceptance gate

Phase 0 is complete only when all applicable baseline scenarios are classified as `PASS`, `FAIL`, or `N/A`, every `FAIL` has a minimal reproduction, and no conclusion is inferred from unit tests alone.

## Evidence

- [`ZED_BASELINE.md`](ZED_BASELINE.md) — baseline procedure, test matrix, and real Zed observations

## Current next actions

1. Reproduce and isolate the semantic lifecycle transition errors observed during repeated tool rounds.
2. Run adversarial tool-result content tests using quotes, ellipsis, Unicode, triple fences, single-quote fences, and literal protocol markers.
3. Exercise load/resume/fork/cancellation from Zed.
4. Keep forwarded MCP as an explicit `FAIL` until the implementation wires Zed-provided MCP servers.
