# Phase 2 — FollowUp / encapsulation incident

## Incident

During the Phase 2 adversarial tests, using a thinking model, one FollowUp encapsulation path was observed to break once. This is treated as a separate investigation target from ordinary tool-result filtering.

## What the code says

`prompt/turn/rounds.rs` handles `FollowUp` specially:

1. it detects a parsed `FollowUp` tool call;
2. it calls `follow_up::request_action(...)`;
3. the ACP host is asked for an explicit permission-style choice;
4. when selected, the returned query is appended as `Role::User`;
5. the same `run_turn` continues with the next Gemini round.

Therefore this is **not** a second ACP `session/prompt` and should not require a second `TurnManager` reservation.

`gemini-acp-encaps::TurnManager` deliberately rejects a second active turn for the same session with `EncapsError::TurnAlreadyActive`.

## First hypotheses to discriminate

### H1 — ACP FollowUp request race/deadlock

`request_action` performs `cx.send_request(request).block_task().await` while the enclosing ACP turn is still active. A failure here would be in the interactive ACP request path, not in content filtering.

### H2 — follow-up cancellation/terminal race

The outer turn may transition to cancellation or terminal state while the FollowUp permission interaction is pending. The current `AcpTurn`/`TurnManager` lifecycle owns cancellation independently from the FollowUp request.

### H3 — parser / normalizer interaction

`StreamNormalizer` removes `<FollowUp ...>` markers incrementally. A malformed or split marker must not cause the normalizer to lose the surrounding assistant content or manufacture a second action.

### H4 — repeated round / semantic lifecycle interaction

A selected FollowUp injects a new `Role::User` message and loops inside the same turn. The semantic event emitter must therefore remain in the correct non-terminal state across the internal round transition.

## Evidence needed from ACP_LOG

For the failing occurrence, isolate the contiguous event sequence containing:

- the assistant stream carrying `<FollowUp ...>`;
- the normalized ACP presentation before the action;
- `session/request_permission` and its response/outcome;
- any `stderr`/runtime error from `gemini_acp_encaps`;
- the next assistant/tool round, if any;
- final `PromptResponse` / stop reason.

Do not classify this as a filtering failure unless the ACP evidence shows that the FollowUp marker/content was corrupted before `request_action`.

## Current code evidence

- FollowUp action IDs are generated per action (`followup_<uuid>`), so the interaction itself is not intentionally reusing a static tool call ID.
- FollowUp is represented with a `ToolCall`-shaped ACP permission request but is explicitly marked with metadata `nonExecutionKind: follow_up_action`.
- A selected FollowUp remains inside the existing `run_turn` loop.
- `TurnManager` rejects competing **outer turns**, not internal FollowUp rounds.

This distinction should be preserved when interpreting the raw Phase 2 capture.
