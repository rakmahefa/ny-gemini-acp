# Phase 2 — Adversarial Streaming and Tool/Content Integrity

Phase 2 hardens the raw streaming boundary after lifecycle defects are closed.

## Phase artifacts

- [`README.md`](README.md) — scope and exit criteria
- [`PROMPT_TEST.md`](PROMPT_TEST.md) — prompts and fixture-driven scenarios for adversarial content tests
- [`ACP_LOG.md`](ACP_LOG.md) — raw ACP evidence captured from real Zed

## Primary scope

- arbitrary valid chunk boundaries
- protocol markers split across chunks
- tool-result payloads containing protocol-like text
- Markdown fences and triple quotes as ordinary data
- Unicode and UTF-8 boundary coverage
- duplicate tool-call IDs
- fail-closed presentation behavior

## Exit criteria

The normalized semantic result must remain invariant under valid stream repartitioning, and tool-result data must never be reinterpreted as ACP/Gemini protocol.
