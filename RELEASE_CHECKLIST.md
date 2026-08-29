# Release readiness checklist

## Verification

- [ ] `cargo fmt --check`
- [ ] `cargo check --workspace`
- [ ] `cargo test --workspace --all-targets`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] semantic event invariant matrix passes
- [ ] replay journal audit and JSONL round-trip pass
- [ ] concurrency stress tests pass
- [ ] dependency audit reviewed

## Contracts

- [ ] ACP compatibility checked against supported protocol version
- [ ] tool-result semantics documented and verified
- [ ] cancellation and failure semantics verified
- [ ] persistence guarantees and limitations documented
- [ ] no unsupported OS sandbox/confinement claim is made

## Release contents

- [ ] `CHANGELOG.md` updated
- [ ] version numbers synchronized
- [ ] release notes include behavior and compatibility changes
- [ ] migration/persistence impact reviewed
- [ ] user-facing diagnostics reviewed for actionable errors

## Rollback

A release rollback means returning the deployment to the previous known-good tag and preserving the failed release artifacts and semantic journal needed for diagnosis. Do not rewrite or silently discard incident records.
