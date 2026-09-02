# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/), and this project
adheres to [Semantic Versioning](https://semver.org/).

## [0.3.0] — 2026-09-02

Implementation of the SPEC plan (audit of 0.2.2, commit `d16e6eb`) — Phase P0
(Security & visible bugs) and Phase P1 (Semantic consolidation) delivered in
full, with their acceptance tests. Gates: `cargo fmt --check`, `cargo clippy
--workspace --all-targets -- -D warnings`, `cargo check --workspace
--all-targets`, 393 tests green (370 pre-existing + 23 new).

### Security — Phase P0

- **[SPEC-P0-01] Secured the shell sandbox** (`fix(tools-provider)`):
  - refuses any argument containing `!` (git alias/fsmonitor hooks) and the
    `ext::` transport prefix;
  - refuses `git -c`/`--config*`/`--exec-path` overrides (pager, editor,
    fsmonitor, gpg.program, diff.external execute through a shell);
  - refuses `sed` as a whole — its `e` capability is not finely filterable
    (same fail-closed posture as `awk`);
  - refuses `tar --to-command`/`--checkpoint-action` and `find -delete`,
    `-exec*`, `-ok*`, `-fls`, `-fprint*`;
  - new single classification entry point `ShellSandbox::classify`
    (validation failure => Critical); High list gains
    git/gh/sed/find/tar/zip/unzip with a documented read-only git
    subcommand exception;
  - refusal messages and module docs state the heuristic, no-OS-confinement
    nature of the sandbox; decision recorded in
    `docs/adr/0001-sandbox-execution.md`;
  - intrusion suite `sandbox/attack_tests.rs`: the 4 audit vectors and
    variants, pipe combinations, positive controls.

- **[SPEC-P0-02] Session titles now survive restart and fork**
  (`fix(acp-adaptor)`): the derived title is written through
  `Store::update_session` (the live entry receives it, so `end_turn`'s merge
  is correct); UI notification kept for display; store-level acceptance
  tests (reload, fork, untitled).

- **[SPEC-P0-03] Typed streaming channel** (`fix(llm-provider)`):
  `StreamResult = Result<StreamItem, LlmError>`; the producer no longer
  detypes errors into strings; `map_gemini_error` moved to `core::errors` as
  the single classification point; `CookiesExpired` is classified
  `authentication`.

### Semantic consolidation — Phase P1

- **[SPEC-P1-01] BusyIo vs AlreadyRunning** (`fix(agent-runtime)`): a storage
  I/O failure on the busy sentinel surfaces as `TurnError::BusyIo` (ACP
  `internal_error`), never as the lying "a turn is already active".
- **[SPEC-P1-02] load/resume ordering** (`fix(acp-adaptor)`): `cancel_and_wait`
  now runs before the snapshot read in `session/load` and `session/resume`
  (same order as fork/delete/close), so the replay includes the committed
  cancelled turn; misleading D-13 comments removed.
- **[SPEC-P1-03] Honest config validation, session-id guard, typed image
  upload** (`fix(acp-adaptor)`): `session/set_config_option` answers
  `invalid_params` (English, listing accepted values) instead of silently
  succeeding on unknown model / non-numeric think / invalid tools_enabled /
  unknown config_id; the handler validates session ids through the shared
  guard; image-upload failures are typed (`ImageUploadError`) and projected
  as `internal_error` instead of `StopReason::Refusal`, with the user message
  pushed before finalization so it survives the replay.
- **[SPEC-P1-04] Effective tool cancellation + session-scoped always
  allow/reject** (`feat(tools-provider)`): cancellation flows from
  `ToolCallRequest` into `registry.call_async` and every tool; `shell_exec`
  kills its process group on `session/cancel` (< 2 s); MCP calls are aborted
  in-flight and the stdio transport correlates responses by request id;
  "always allow/reject" decisions are remembered per (tool, kind) in the
  session state and enforced without re-prompting (never cross-session,
  forks do not inherit).
- **[SPEC-P1-05] Dead-path removal + unified risk + honest budget**
  (`refactor(tools-provider)`): `executor/terminal.rs` and the executor
  lifecycle state machine deleted; single permission policy and single risk
  classifier remain; the fake `checked_add` overflow guard is replaced by
  `max_tool_calls_per_turn` (default 128) with a truthful
  `ToolCallBudgetExhausted` error.
- **[SPEC-P1-06] Decoded-frame detections + honest web2api** (`fix
  (llm-provider)`): `bard_error` fires on decoded metadata only (never on
  text candidates), raw-accumulator refusal-phrase scan deleted; failed
  image uploads and SSE upstream errors surface as errors (502 / error
  chunks), never as successful streams; tool-call blocks are removed only
  when parsed, and multi-line bare `function_call` payloads are extracted via
  brace balancing.

### Notes

- Version bumped 0.2.2 -> 0.3.0 following the plan's milestone table
  (jalon A "Sécurité" + jalon B "Consolidation").
- Remaining plan work (Phase P2 deep refactors and SPEC-CI) is tracked in
  `SPEC.md` section 10.
