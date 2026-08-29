#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

printf '\n==> cargo fmt\n'
cargo fmt --all -- --check

printf '\n==> cargo test\n'
cargo test --workspace --all-targets

printf '\n==> cargo clippy\n'
cargo clippy --workspace --all-targets -- -D warnings

printf '\n==> provider-neutral architecture audit\n'
./scripts/audit-provider-neutral.sh

printf '\nValidation passed.\n'
