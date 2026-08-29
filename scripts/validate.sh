#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

printf '\n==> cargo fmt --check\n'
cargo fmt --check

printf '\n==> cargo check --workspace\n'
cargo check --workspace

printf '\n==> cargo test --workspace --all-targets\n'
cargo test --workspace --all-targets

printf '\n==> cargo clippy --workspace --all-targets -- -D warnings\n'
cargo clippy --workspace --all-targets -- -D warnings

printf '\nValidation passed.\n'
