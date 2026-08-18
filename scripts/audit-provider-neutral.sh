#!/usr/bin/env bash
set -uo pipefail

ROOT="${1:-.}"
cd "$ROOT"

fail=0

section() {
  printf '\n=== %s ===\n' "$1"
}

findings() {
  local title="$1"
  shift
  section "$title"
  local out
  out=$("$@" 2>/dev/null || true)
  if [[ -n "$out" ]]; then
    printf '%s\n' "$out"
    fail=1
  else
    printf 'OK\n'
  fi
}

require_rg() {
  if ! command -v rg >/dev/null 2>&1; then
    echo "ERROR: ripgrep (rg) is required" >&2
    exit 2
  fi
}

require_rg

# The adapter is the only layer allowed to know ACP details.
RUNTIME='crates/agent-runtime/src'
LLM='crates/llm-provider/src'
TOOLS='crates/tools-provider/src'
ADAPTER='crates/acp-adaptor/src'

# -----------------------------------------------------------------------------
# 1. HARD BOUNDARY: agent-runtime must know neither ACP nor Gemini.
# -----------------------------------------------------------------------------
findings "1. ACP leakage into agent-runtime" \
  rg -n -S --glob '*.rs' \
    'agent[_-]client[_-]protocol|schema::v1|PromptRequest|InitializeRequest|NewSessionRequest|McpServer|Acp|ACP' \
    "$RUNTIME"

findings "2. Gemini leakage into agent-runtime" \
  rg -n -S --glob '*.rs' \
    '\bGemini\b|\bgemini\b|google|sapisid|web2api|cookie_file|auth_user' \
    "$RUNTIME"

# -----------------------------------------------------------------------------
# 2. CONTRACT SURFACE: only inspect the actual provider boundary.
# -----------------------------------------------------------------------------
section "3. Generic LLM contract: suspicious Gemini-shaped fields"
rg -n -S \
  'think\b|refs\b|prompt:\s*String|model:\s*String' \
  "$RUNTIME/providers.rs" 2>/dev/null || true

section "4. Generic LLM contract: weakly typed boundaries"
rg -n -S \
  'serde_json::Value|Vec<Value>|Result<[^>]+,\s*String>|pub .*String' \
  "$RUNTIME/providers.rs" 2>/dev/null || true

section "5. Generic Tool contract: ACP-shaped or untyped MCP configuration"
rg -n -S \
  'serde_json::Value|Vec<Value>|agent[_-]client[_-]protocol|McpServer|Result<[^>]+,\s*String>' \
  "$RUNTIME/providers.rs" 2>/dev/null || true

# -----------------------------------------------------------------------------
# 3. PROVIDER IMPLEMENTATIONS: detect protocol leakage at the adapter edge.
# Internal provider implementation code may use protocol details, but the
# provider entry points should not expose them in public contracts.
# -----------------------------------------------------------------------------
findings "6. ACP types crossing llm-provider entry points" \
  rg -n -S --glob '*.rs' \
    'agent[_-]client[_-]protocol|schema::v1|PromptRequest|InitializeRequest|McpServer' \
    "$LLM/provider.rs"

findings "7. ACP types crossing tools-provider entry point" \
  rg -n -S --glob '*.rs' \
    'agent[_-]client[_-]protocol|schema::v1|PromptRequest|InitializeRequest' \
    "$TOOLS/provider.rs"

# -----------------------------------------------------------------------------
# 4. TOOL CONFIGURATION: MCP should be converted at the ACP boundary.
# These are the concrete places to inspect if ACP types leaked inward.
# -----------------------------------------------------------------------------
section "8. MCP configuration conversion sites"
rg -n -S \
  'McpServer|from_acp_servers|configure_session|servers:\s*Vec<Value>|serde_json::from_value' \
  "$TOOLS" "$ADAPTER" 2>/dev/null || true

# -----------------------------------------------------------------------------
# 5. SESSION OWNERSHIP: focus only on provider/session coupling.
# -----------------------------------------------------------------------------
section "9. Provider-owned session state"
rg -n -S \
  'for_session|configure_session|clear_session|session_id|HashMap<.*String.*Arc<dyn ToolProvider|HashMap<String, Arc<ToolRegistry>>' \
  "$RUNTIME/providers.rs" "$TOOLS/provider.rs" "$RUNTIME/session.rs" 2>/dev/null || true

# -----------------------------------------------------------------------------
# 6. LEGACY MIGRATION: only old architectural identities, not generic TODOs.
# -----------------------------------------------------------------------------
section "10. Legacy architecture names still referenced"
rg -n -S \
  'gemini_acp_(runtime|config|agent|encaps|tools)|gemini-acp-(runtime|config|agent|encaps|tools)' \
  Cargo.toml crates README.md scripts 2>/dev/null || true

# -----------------------------------------------------------------------------
# 7. DEPENDENCY DIRECTION: only inspect the runtime package.
# -----------------------------------------------------------------------------
section "11. agent-runtime direct provider/protocol dependencies"
if command -v cargo >/dev/null 2>&1; then
  cargo tree -p agent-runtime --depth 1 2>/dev/null \
    | rg -n 'llm-provider|tools-provider|agent-client-protocol|gemini' \
    || true
else
  echo "cargo not available; skipped"
fi

# -----------------------------------------------------------------------------
# 8. FINAL CHECK: public provider contracts should remain centralized.
# -----------------------------------------------------------------------------
section "12. Provider trait declarations"
rg -n -S \
  '^pub trait (LlmProvider|ToolProvider)|^pub struct (LlmRequest|ToolCallRequest|ToolCallResult|LlmModelInfo)' \
  "$RUNTIME" "$LLM" "$TOOLS" 2>/dev/null || true

printf '\n=== RESULT ===\n'
if [[ "$fail" -eq 0 ]]; then
  echo "No hard-boundary findings. Review informational sections 3-5, 8-12 manually."
  exit 0
fi

echo "Focused findings detected. Prioritize sections 1-2, then 3-9."
exit 1
