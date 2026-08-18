# Documentation Roadmap

The repository documentation is organized by implementation phase. Each phase contains the evidence, contracts, decisions, procedures, prompts, and raw ACP evidence needed to validate that phase independently.

## Phase document convention

Every phase directory should provide, as applicable:

```text
phase-X/
├── README.md
├── PROMPT_TEST.md
├── ACP_LOG.md
└── phase-specific contracts / evidence
```

`PROMPT_TEST.md` is the executable manual test catalogue. `ACP_LOG.md` is the raw evidence journal captured from Zed ACP Logs. The README explains scope and interpretation; phase-specific documents contain the durable baseline/contract for that phase.

## Phases

| Phase | Directory | Status | Purpose |
|---|---|---|---|
| 0 | [`phase-0/`](phase-0/) | In progress | Establish the real Zed/ACP baseline before further hardening. |
| 1 | [`phase-1/`](phase-1/) | Planned | Close lifecycle defects observed by the real Zed baseline. |
| 2 | [`phase-2/`](phase-2/) | Planned | Adversarial streaming and tool/content integrity validation. |

## Documentation rules

- Evidence belongs to the phase that produced it.
- A baseline result must distinguish `PASS`, `FAIL`, `BLOCKED`, `UNOBSERVED`, and `N/A`.
- Implementation facts are not baseline evidence until verified at the external boundary.
- Every reproducible failure should become either a regression test or an explicitly documented limitation.
- Raw ACP traffic belongs in `ACP_LOG.md`; conclusions belong in the phase README/baseline documents.
- Prompts used to reproduce a scenario belong in `PROMPT_TEST.md` and should be stable enough to rerun after a fix.
- Phase documents should link forward to the next phase rather than mixing unrelated implementation notes.

## Current priority

Phase 0 remains active. The latest Zed captures demonstrate working ACP handshake, session/config negotiation, multi-chunk assistant streaming, real file write/read, ordinary tool execution, and Markdown/Unicode preservation. They also expose concrete gaps: forwarded MCP servers are not wired, and the semantic event emitter reports invalid lifecycle transitions during a repeated tool round.
