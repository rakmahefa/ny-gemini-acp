#!/usr/bin/env bash
set -euo pipefail

# Phase 0 helper: collect the environment needed for a real Zed baseline.
# This script deliberately does not mutate Zed settings or claim ACP compatibility.

usage() {
  cat <<'EOF'
Usage: scripts/zed-baseline.sh [--output PATH]

Collects a reproducible environment report for Phase 0 — Baseline réelle Zed.
It does not edit Zed settings, launch a GUI session, or alter the agent.

Environment variables:
  GEMINI_ACP_DATA_DIR  Optional session data directory to record.
EOF
}

output=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output)
      [[ $# -ge 2 ]] || { echo "error: --output requires a path" >&2; exit 2; }
      output="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$repo_root" ]]; then
  echo "error: run this script from inside the ny-gemini-acp git repository" >&2
  exit 1
fi
cd "$repo_root"

if ! command -v zed >/dev/null 2>&1; then
  zed_status="BLOCKED: zed executable not found"
else
  zed_status="$(zed --version 2>&1 | head -n 1)"
fi

if [[ -x "target/release/gemini-acp" ]]; then
  agent_path="$repo_root/target/release/gemini-acp"
  agent_status="release binary present"
else
  agent_path="$repo_root/target/release/gemini-acp"
  agent_status="BLOCKED: target/release/gemini-acp not found (run cargo build -p gemini-acp-agent --release)"
fi

commit="$(git rev-parse HEAD)"
branch="$(git branch --show-current)"
remote="$(git remote get-url origin 2>/dev/null || true)"

rust_version="$(rustc --version 2>/dev/null || echo 'BLOCKED: rustc not found')"
cargo_version="$(cargo --version 2>/dev/null || echo 'BLOCKED: cargo not found')"

os_name="$(uname -s 2>/dev/null || echo unknown)"
os_release="$(uname -r 2>/dev/null || echo unknown)"
arch="$(uname -m 2>/dev/null || echo unknown)"

report="$(cat <<EOF
# ny-gemini-acp — Phase 0 Zed Baseline Environment

Generated: $(date -u '+%Y-%m-%dT%H:%M:%SZ')

## Repository

- Branch: $branch
- Commit: $commit
- Origin: ${remote:-UNSET}

## Host

- OS: $os_name
- Kernel: $os_release
- Architecture: $arch

## Toolchain

- Rust: $rust_version
- Cargo: $cargo_version
- Zed: $zed_status

## Agent

- Binary: $agent_path
- Status: $agent_status
- Package version: $(awk -F'"' '/^version\.workspace|^version[[:space:]]*=/{print $2; exit}' Cargo.toml 2>/dev/null || echo 'workspace-defined')
- GEMINI_ACP_DATA_DIR: ${GEMINI_ACP_DATA_DIR:-UNSET}

## Real-Zed execution state

This script only collects prerequisites. A real baseline still requires a Zed External Agent thread and ACP log observation.

- Zed launch: $(if command -v zed >/dev/null 2>&1; then echo READY; else echo BLOCKED; fi)
- Release agent binary: $(if [[ -x "$agent_path" ]]; then echo READY; else echo BLOCKED; fi)
- ACP evidence: UNOBSERVED
EOF
)"

if [[ -n "$output" ]]; then
  mkdir -p "$(dirname "$output")"
  printf '%s\n' "$report" > "$output"
  echo "baseline environment report: $output"
else
  printf '%s\n' "$report"
fi

cat <<'EOF'

Next real-Zed step:
  1. Configure the release binary as a Zed Custom External Agent under `agent_servers`.
  2. Start a fresh External Agent thread.
  3. Open `dev: open acp logs`.
  4. Execute the matrix in docs/ZED_BASELINE.md and record PASS/FAIL/BLOCKED/N/A.
EOF
