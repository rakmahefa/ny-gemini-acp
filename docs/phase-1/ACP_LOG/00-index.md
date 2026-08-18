# Phase 1 — ACP log split index

The canonical raw evidence remains `../ACP_LOG.md`. This directory is an analysis-oriented view for the lifecycle hardening evidence.

## Focus

- repeated tool rounds
- duplicate/replayed tool identity
- permission → execution → result ordering
- semantic terminality
- cancellation and failure transitions

For deterministic event-level splitting, run:

```bash
python3 scripts/split-acp-logs.py docs/phase-1/ACP_LOG.md docs/phase-1/ACP_LOG/parts --events-per-part 100
```

The most important Phase 1 evidence should be grouped around the first repeated-tool incident and its follow-up tool round, not interpreted from isolated log lines.
