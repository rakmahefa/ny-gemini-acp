# ADR 0001 — Sandbox shell execution: heuristic filter now, real confinement later

- **Status**: Accepted (partial — the confinement migration is deferred to SPEC-P2-05)
- **Date**: 2026-09-02
- **Context**: audit of ny-gemini-acp 0.2.2, chapter 3.1; SPEC-P0-01 and SPEC-P1-05

## Context

`shell_exec` runs commands with `sh -c <command>` under the caller's full
privileges. The only barrier is `ShellSandbox`: a hand-written lexer plus an
allowlist plus per-argument validation. The audit demonstrated four bypasses
that passed static validation while executing arbitrary commands:

1. `git -c alias.pwn='!cmd' status` — git config hooks execute through a shell
   (`core.pager`, `core.editor`, `core.fsmonitor`, `gpg.program`,
   `diff.external` work even without `!`);
2. `echo x | sed 's/x/x/e'` — GNU sed executes the pattern space via `/bin/sh`;
3. `tar --to-command=cmd` / `--checkpoint-action=exec=...` — post-archive hooks;
4. `find . -name '*.log' -delete` — a bare `rm` behind a read-only-looking tool.

The intended confinement path (ACP terminal, `executor/terminal.rs`) was dead
code, which made the permission labels dishonest: `git -c alias.pwn=...`
executed without any permission request in AcceptEdits mode.

## Decision

1. **Keep the heuristic filter, and make it honest.** Every refusal message and
   the module documentation now state explicitly that the sandbox is a
   *heuristic filter without OS confinement*. No message may claim an
   effective barrier.
2. **Refuse the known arbitrary-execution vectors at argument validation**:
   - any argument value containing `!` (generic hook vector: git aliases,
     fsmonitor) or starting with `ext::` (git transports);
   - `git -c` / `--config` / `--config-env` / `--exec-path` as a whole —
     filtering every shell-executing config key is not tractable;
   - `sed` as a whole — its `e` capability has delimiter variants that defeat
     fine-grained filtering (same fail-closed posture as `awk`);
   - `tar --to-command*` and `--checkpoint-action*`;
   - `find -delete` (and `-exec`/`-execdir`/`-ok`/`-okdir`/`-fls`/`-fprint*`):
     find passes in read-only usage only.
3. **Classify before prompting**: `ShellSandbox::classify` is the single risk
   entry point (validation failure → `Critical`). The High-risk program list
   gains `git`, `gh`, `sed`, `find`, `tar`, `zip`, `unzip`, with one documented
   exception: read-only git subcommands (status, log, diff, ...) keep their
   computed level so AcceptEdits does not prompt on every inspection command.
4. **Confinement remains a deferred decision** (SPEC-P2-05): either route
   `shell_exec` through the ACP terminal (Zed owns the process) or apply a real
   OS confinement (Landlock/seccomp via a dedicated crate, or a container).
   The dead `executor/terminal.rs` path is removed in SPEC-P1-05 rather than
   kept as silent dead code; the chosen migration will re-introduce an
   *executing* implementation under this ADR.

## Consequences

- The four audit vectors and their quoted variants fail with a clear error
  before any process spawn (`sandbox/attack_tests.rs`, kept green in CI).
- Legitimate read-only workflows (`ls`, `rg`, `git status`, `git log`,
  `find -name`, `tar -tf`, `unzip -l`, ...) keep working (positive controls in
  the same suite). `cargo build` stays refused: build scripts execute arbitrary
  code and the sandbox cannot contain them.
- `sed` removal is an explicit usability trade-off; the escape hatch is the
  permission prompt path (Explicit permission mode), never a silent bypass.
- `find -perm 4000` stays allowed: it is read-only enumeration, equivalent in
  kind to `ls`/`grep`; the intrusion suite records it as a positive control
  rather than a refused vector (deviation from spec §7.1, documented here).
