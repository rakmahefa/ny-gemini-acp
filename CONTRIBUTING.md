# Contributing

## Development

This repository is a Rust workspace with four crates. Keep changes within the existing crate boundaries unless there is a concrete architectural reason to do otherwise.

Before opening a pull request, run:

```text
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace --all-targets
```

## Tests

Use inline `#[cfg(test)]` modules for unit tests and the crate `tests/` directory for integration tests. New `#[path]`-mounted test modules should not be introduced.

## Commits

Use the project convention:

```text
fix|refactor|chore(scope): [SPEC-Px-yy] description
```

Do not add audit-report identifiers to Rust comments. Put rationale in the commit message, pull request description, ADR, or changelog instead.
