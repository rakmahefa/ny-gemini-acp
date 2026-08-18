# Phase 0 — ACP log split index

The canonical raw evidence remains `../ACP_LOG.md`. This directory provides analysis-oriented slices and does not replace the raw capture.

## Focus

- transport and handshake
- session/config negotiation
- assistant streaming
- ordinary tool execution
- file read/write preservation
- baseline failures and unobserved cases

Use the canonical log for exact ordering. Use the split index files for targeted analysis.

For deterministic event-level splitting, run:

```bash
python3 scripts/split-acp-logs.py docs/phase-0/ACP_LOG.md docs/phase-0/ACP_LOG/parts --events-per-part 100
```
