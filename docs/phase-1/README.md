# Phase 1 — Semantic Lifecycle Hardening

Phase 1 turns the concrete lifecycle defects observed during the real Zed baseline into deterministic contracts and fixes.

## Entry conditions

Phase 0 must provide reproducible evidence for every lifecycle failure carried forward.

## Phase artifacts

- [`README.md`](README.md) — scope, entry and exit criteria
- [`PROMPT_TEST.md`](PROMPT_TEST.md) — real-Zed prompts for lifecycle reproduction
- [`ACP_LOG.md`](ACP_LOG.md) — raw ACP evidence for lifecycle transitions

## Primary scope

- repeated tool rounds in one turn
- tool identity ownership across rounds
- permission → execution → result ordering
- terminal-state protection
- cancellation terminality
- duplicate or replayed tool events
- semantic event emitter invariants

## Exit criteria

- the observed repeated-tool lifecycle failure has a deterministic regression test;
- no semantic event is accepted from an invalid predecessor state;
- normal completion, failure, and cancellation are mutually exclusive terminal outcomes;
- real Zed reproduces the fixed lifecycle without runtime emitter errors.
