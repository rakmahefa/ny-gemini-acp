#!/usr/bin/env bash
set -euo pipefail

printf '\n==> duplicate dependency versions\n'
cargo tree --workspace --duplicates

printf '\n==> enabled feature graph\n'
cargo tree --workspace --features all
