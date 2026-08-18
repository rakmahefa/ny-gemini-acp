# Documentation Roadmap

The repository documentation is organized by implementation phase. Each phase contains the evidence, contracts, decisions, and procedures needed to validate that phase independently.

## Phases

| Phase | Directory | Status | Purpose |
|---|---|---|---|
| 0 | [`phase-0/`](phase-0/) | In progress | Establish the real Zed/ACP baseline before further hardening. |
| 1 | `phase-1/` | Planned | Close lifecycle defects observed by the real Zed baseline. |
| 2 | `phase-2/` | Planned | Adversarial streaming and tool/content integrity validation. |

## Documentation rules

- Evidence belongs to the phase that produced it.
- A baseline result must distinguish `PASS`, `FAIL`, `BLOCKED`, `UNOBSERVED`, and `N/A`.
- Implementation facts are not baseline evidence until verified at the external boundary.
- Every reproducible failure should become either a regression test or an explicitly documented limitation.
- Phase documents should link forward to the next phase rather than mixing unrelated implementation notes.

## Current priority

Phase 0 remains active. The latest Zed capture demonstrates working ACP handshake, session/config negotiation, multi-chunk assistant streaming, real file write/read, and ordinary tool execution. It also exposes two concrete gaps: forwarded MCP servers are not wired, and the semantic event emitter reports invalid lifecycle transitions during a repeated tool round.
