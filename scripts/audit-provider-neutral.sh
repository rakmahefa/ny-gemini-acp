#!/usr/bin/env bash
set -uo pipefail

ROOT="${1:-.}"
cd "$ROOT"

FAIL_COUNT=0
WARN_COUNT=0

section() {
  printf '\n=== %s ===\n' "$1"
}

status_line() {
  local level="$1"
  local message="$2"
  printf '%-4s %s\n' "$level" "$message"
}

# Assertions where a match means a violation.
assert_no_match() {
  local severity="$1"
  local title="$2"
  shift 2

  local out rc
  out=$("$@" 2>&1)
  rc=$?

  if [[ "$rc" -eq 0 ]]; then
    printf '%s\n' "$out"
    if [[ "$severity" == "FAIL" ]]; then
      FAIL_COUNT=$((FAIL_COUNT + 1))
      status_line "FAIL" "$title"
    else
      WARN_COUNT=$((WARN_COUNT + 1))
      status_line "WARN" "$title"
    fi
    return 1
  fi

  if [[ "$rc" -gt 1 ]]; then
    printf '%s\n' "$out"
    printf 'command failed with exit code %d\n' "$rc"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    status_line "FAIL" "$title (audit command failed)"
    return 1
  fi

  status_line "PASS" "$title"
  return 0
}

# Assertions where a match means the expected contract is present.
assert_match() {
  local severity="$1"
  local title="$2"
  shift 2

  local out rc
  out=$("$@" 2>&1)
  rc=$?

  if [[ "$rc" -eq 0 ]]; then
    printf '%s\n' "$out"
    status_line "PASS" "$title"
    return 0
  fi

  if [[ "$rc" -gt 1 ]]; then
    printf '%s\n' "$out"
    printf 'command failed with exit code %d\n' "$rc"
  fi

  if [[ "$severity" == "FAIL" ]]; then
    FAIL_COUNT=$((FAIL_COUNT + 1))
    status_line "FAIL" "$title"
  else
    WARN_COUNT=$((WARN_COUNT + 1))
    status_line "WARN" "$title"
  fi
  return 1
}

require_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'ERROR: %s is required\n' "$command_name" >&2
    exit 2
  fi
}

check_direct_dependency_absent() {
  local manifest="$1"

  if rg -n -S '^agent-client-protocol[[:space:]]*=' "$manifest" >/dev/null 2>&1; then
    return 0
  fi
  return 1
}

check_direct_dependency_used() {
  local source_dir="$1"
  rg -n -S --glob '*.rs' 'agent[_-]client[_-]protocol' "$source_dir" >/dev/null 2>&1
}

require_command rg

# The adapter is the only layer allowed to own ACP protocol conversion.
RUNTIME='crates/agent-runtime/src'
LLM='crates/llm-provider/src'
TOOLS='crates/tools-provider/src'
ADAPTER='crates/acp-adaptor/src'

# -----------------------------------------------------------------------------
# 1. HARD BOUNDARY: agent-runtime must know neither ACP nor Gemini.
# Production code is FAIL; test-only provider fixtures are WARN.
# -----------------------------------------------------------------------------
section "1. Runtime boundary"
assert_no_match "FAIL" "agent-runtime has no executable ACP references" \
  rg -n -S --glob '*.rs' --glob '!**/test/**' \
    'agent[_-]client[_-]protocol|schema::v1|PromptRequest|InitializeRequest|NewSessionRequest|\\bMcpServer\\b' \
    "$RUNTIME"

assert_no_match "FAIL" "agent-runtime production code has no Gemini/provider-specific references" \
  rg -n -S --glob '*.rs' --glob '!**/test/**' \
    '\\bGemini\\b|\\bgemini\\b|google|sapisid|web2api|cookie_file|auth_user' \
    "$RUNTIME"

assert_no_match "WARN" "agent-runtime tests are provider-neutral" \
  rg -n -S --glob '*.rs' \
    '\\bGemini\\b|\\bgemini\\b|gemini-acp-(runtime|config|agent|encaps|tools)|gemini_acp_(runtime|config|agent|encaps|tools)' \
    "$RUNTIME/test"

# -----------------------------------------------------------------------------
# 2. CONTRACT SURFACE: informational checks are WARN-only by design.
# They are signals for future typing work, not architectural failures.
# -----------------------------------------------------------------------------
section "2. Contract surface"
assert_match "WARN" "LLM contract contains fields worth reviewing for stronger typing" \
  rg -n -S \
    'serde_json::Value|Vec<Value>|Result<[^>]+,\\s*String>|pub .*String' \
    "$RUNTIME/providers.rs"

assert_match "WARN" "Tool contract contains fields worth reviewing for stronger typing" \
  rg -n -S \
    'ToolCallRequest|ToolCallResult|ToolProvider' \
    "$RUNTIME/providers.rs"

# -----------------------------------------------------------------------------
# 3. ACP MUST NOT cross provider entry points.
# Match actual ACP imports/types, not generic names such as McpServerConfig.
# -----------------------------------------------------------------------------
section "3. ACP provider boundary"
assert_no_match "FAIL" "ACP types do not cross llm-provider entry point" \
  rg -n -S --glob '*.rs' \
    'agent[_-]client[_-]protocol|schema::v1|PromptRequest|InitializeRequest|NewSessionRequest' \
    "$LLM/provider.rs"

assert_no_match "FAIL" "ACP types do not cross tools-provider entry point" \
  rg -n -S --glob '*.rs' \
    'agent[_-]client[_-]protocol|schema::v1|PromptRequest|InitializeRequest|NewSessionRequest' \
    "$TOOLS/provider.rs"

# -----------------------------------------------------------------------------
# 4. Dependency hygiene.
# - llm-provider MUST NOT declare ACP and must not pull it as a direct dep.
# - tools-provider MAY declare ACP only because tools/elicitation.rs currently
#   projects interactive tool requests onto ACP client APIs.
# -----------------------------------------------------------------------------
section "4. Cargo dependency hygiene"

if [[ ! -f crates/llm-provider/Cargo.toml || ! -f crates/tools-provider/Cargo.toml ]]; then
  status_line "FAIL" "provider Cargo.toml files are present"
  FAIL_COUNT=$((FAIL_COUNT + 1))
else
  if check_direct_dependency_absent "crates/llm-provider/Cargo.toml"; then
    status_line "FAIL" "llm-provider has no direct agent-client-protocol dependency"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  else
    status_line "PASS" "llm-provider has no direct agent-client-protocol dependency"
  fi

  if check_direct_dependency_used "$TOOLS"; then
    status_line "PASS" "tools-provider ACP dependency is justified by source usage (elicitation bridge)"
  else
    if rg -n -S '^agent-client-protocol[[:space:]]*=' crates/tools-provider/Cargo.toml >/dev/null 2>&1; then
      status_line "FAIL" "tools-provider declares ACP without source usage"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    else
      status_line "PASS" "tools-provider does not declare an unnecessary ACP dependency"
    fi
  fi
fi

if command -v cargo >/dev/null 2>&1; then
  llm_tree=$(cargo tree -p llm-provider --depth 1 2>/dev/null || true)
  if printf '%s\n' "$llm_tree" | rg -n 'agent-client-protocol' >/dev/null 2>&1; then
    printf '%s\n' "$llm_tree" | rg -n 'agent-client-protocol'
    status_line "FAIL" "llm-provider dependency graph contains agent-client-protocol"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  else
    status_line "PASS" "llm-provider dependency graph excludes agent-client-protocol"
  fi

  tools_tree=$(cargo tree -p tools-provider --depth 1 2>/dev/null || true)
  if printf '%s\n' "$tools_tree" | rg -n 'agent-client-protocol' >/dev/null 2>&1; then
    status_line "PASS" "tools-provider dependency graph contains only its justified ACP dependency"
  else
    status_line "WARN" "tools-provider dependency graph does not expose ACP; verify the elicitation boundary manually"
    WARN_COUNT=$((WARN_COUNT + 1))
  fi
else
  status_line "WARN" "cargo unavailable; dependency graph checks skipped"
  WARN_COUNT=$((WARN_COUNT + 1))
fi

# -----------------------------------------------------------------------------
# 5. MCP normalization: ACP-specific server shapes must be normalized by the
# adapter; MCP implementation is allowed inside tools-provider.
# -----------------------------------------------------------------------------
section "5. MCP normalization"
assert_match "FAIL" "ACP MCP servers are normalized at the adapter boundary" \
  rg -n -S 'agent_client_protocol::schema::v1|McpServer|from_acp_servers|normalize_server' \
  "$ADAPTER/config/mcp.rs"

assert_match "WARN" "tools-provider owns MCP implementation details rather than runtime" \
  rg -n -S 'McpServerConfig|McpCatalog|McpTransportKind' "$TOOLS"

# -----------------------------------------------------------------------------
# 6. Provider-owned session state.
# -----------------------------------------------------------------------------
section "6. Provider-owned session state"
assert_match "WARN" "tool session ownership is implemented by ToolProvider" \
  rg -n -S \
    'for_session|configure_session|clear_session|session_id|HashMap' \
    "$RUNTIME/providers.rs" "$TOOLS/provider.rs" "$RUNTIME/session.rs"

# -----------------------------------------------------------------------------
# 7. Legacy architecture identities.
# Production references are FAIL; fixtures/docs are WARN.
# -----------------------------------------------------------------------------
section "7. Legacy architecture names"
assert_no_match "FAIL" "production code has no legacy gemini-acp crate identities" \
  rg -n -S --glob '*.rs' --glob '!**/test/**' \
    'gemini_acp_(runtime|config|agent|encaps|tools)|gemini-acp-(runtime|config|agent|encaps|tools)' \
    crates

assert_no_match "WARN" "tests and fixtures have no legacy gemini-acp crate identities" \
  rg -n -S --glob '*.rs' --glob '**/test/**' \
    'gemini_acp_(runtime|config|agent|encaps|tools)|gemini-acp-(runtime|config|agent|encaps|tools)' \
    crates

# -----------------------------------------------------------------------------
# 8. Public provider contracts remain centralized in agent-runtime.
# -----------------------------------------------------------------------------
section "8. Provider trait declarations"
assert_match "FAIL" "provider traits are centralized in agent-runtime" \
  rg -n -S \
    '^pub trait (LlmProvider|ToolProvider)' \
    "$RUNTIME/providers.rs"

assert_no_match "FAIL" "provider implementations do not redeclare runtime provider traits" \
  rg -n -S \
    '^pub trait (LlmProvider|ToolProvider)' \
    "$LLM" "$TOOLS"

printf '\n=== RESULT ===\n'
printf 'WARN: %d\n' "$WARN_COUNT"
printf 'FAIL: %d\n' "$FAIL_COUNT"

if [[ "$FAIL_COUNT" -eq 0 ]]; then
  if [[ "$WARN_COUNT" -eq 0 ]]; then
    echo "PASS: provider-neutral architecture audit is clean."
  else
    echo "PASS: no hard-boundary failures; warnings are informational and non-blocking."
  fi
  exit 0
fi

echo "FAIL: hard-boundary findings detected."
exit 1
