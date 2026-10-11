#!/usr/bin/env bash
set -Eeuo pipefail
set +x

if [[ $# -ne 3 ]]; then
  echo "Usage: bash scripts/test-release.sh <archive> <checksum-file> <diagnostics-directory>" >&2
  exit 2
fi

ARCHIVE="$1"
CHECKSUM_FILE="$2"
LOG_DIR="$3"
COOKIE_FILE="$(printenv COOKIE_FILE || true)"
RUN_LIVE_INTEGRATION="$(printenv RUN_LIVE_INTEGRATION || echo false)"
RELEASE_SOURCE="$(printenv RELEASE_SOURCE || echo dev-candidate)"
PORT="$(printenv DIAGNOSTICS_PORT || echo 18281)"
BASE_URL="http://127.0.0.1:$PORT"

mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/diagnostics.log"
SERVER_SAFE="$LOG_DIR/server-runtime.log"
TEMP_BASE="$(printenv RUNNER_TEMP || printenv TMPDIR || echo /tmp)"
WORK_DIR="$(mktemp -d "$TEMP_BASE/ny-gemini-acp-diagnostics.XXXXXX")"
SERVER_RAW="$WORK_DIR/server-runtime.raw.log"
SERVER_PID=""

exec > >(tee -a "$LOG_FILE") 2>&1

log() {
  printf '[%s] %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$*"
}

sanitize_server_log() {
  python3 - "$SERVER_RAW" "$SERVER_SAFE" "$COOKIE_FILE" <<'PY'
import json
import re
import sys
from urllib.parse import quote

raw_path, safe_path, cookie_path = sys.argv[1:4]
try:
    with open(raw_path, "r", encoding="utf-8", errors="replace") as handle:
        content = handle.read()
except OSError:
    content = ""

secrets = []
try:
    with open(cookie_path, "r", encoding="utf-8") as handle:
        parsed = json.load(handle)
    if isinstance(parsed, list):
        for item in parsed:
            if isinstance(item, dict) and item.get("name") == "SAPISID":
                value = item.get("value")
                if isinstance(value, str) and value:
                    secrets.append(value)
    elif isinstance(parsed, dict):
        value = parsed.get("sapisid")
        if isinstance(value, str) and value:
            secrets.append(value)
        raw_cookie = parsed.get("cookie")
        if isinstance(raw_cookie, str):
            for piece in raw_cookie.split(";"):
                name, separator, value = piece.strip().partition("=")
                if separator and name.strip() == "SAPISID" and value.strip():
                    secrets.append(value.strip())
except (OSError, ValueError, TypeError):
    pass

for secret in sorted(set(secrets), key=len, reverse=True):
    content = content.replace(secret, "[REDACTED]")
    encoded = quote(secret, safe="")
    if encoded:
        content = content.replace(encoded, "[REDACTED]")

content = re.sub(r"(?i)(SAPISID\s*=\s*)[^;\s,'\"]+", r"\1[REDACTED]", content)
with open(safe_path, "w", encoding="utf-8") as handle:
    handle.write(content)
PY
}

finish() {
  status=$?
  trap - EXIT
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  sanitize_server_log || true
  if [[ "$status" -eq 0 ]]; then
    log "RESULT: PASS"
  else
    log "RESULT: FAIL (exit code $status)"
    if [[ -s "$SERVER_SAFE" ]]; then
      log "Last sanitized server log lines:"
      tail -n 60 "$SERVER_SAFE"
    fi
  fi
  rm -f "$SERVER_RAW"
  rm -rf "$WORK_DIR"
  exit "$status"
}
trap finish EXIT

log "Release source: $RELEASE_SOURCE"
log "Runner OS/architecture: $(uname -s)/$(uname -m)"
log "Archive: $(basename "$ARCHIVE")"
log "Diagnostics are configured to omit credentials and API response bodies."

if [[ -z "$COOKIE_FILE" || ! -f "$COOKIE_FILE" ]]; then
  log "FAIL: temporary cookie file is missing."
  exit 1
fi
if [[ ! -f "$ARCHIVE" || ! -f "$CHECKSUM_FILE" ]]; then
  log "FAIL: archive or SHA-256 checksum file is missing."
  exit 1
fi
if [[ "$(dirname "$ARCHIVE")" != "$(dirname "$CHECKSUM_FILE")" ]]; then
  log "FAIL: archive and checksum must be in the same directory."
  exit 1
fi

ARCHIVE="$(cd "$(dirname "$ARCHIVE")" && pwd)/$(basename "$ARCHIVE")"
CHECKSUM_FILE="$(cd "$(dirname "$CHECKSUM_FILE")" && pwd)/$(basename "$CHECKSUM_FILE")"
(
  cd "$(dirname "$ARCHIVE")"
  sha256sum --check "$(basename "$CHECKSUM_FILE")"
)
log "SHA-256 verification passed."

if tar -tzf "$ARCHIVE" | grep -Eq '(^/|(^|/)\.\.(/|$))'; then
  log "FAIL: archive contains an unsafe path."
  exit 1
fi

UNPACKED="$WORK_DIR/unpacked"
mkdir -p "$UNPACKED"
tar -xzf "$ARCHIVE" -C "$UNPACKED" --no-same-owner
ACP_BIN="$UNPACKED/gemini-acp"
WEB_BIN="$UNPACKED/gemini-web2api"

for binary in "$ACP_BIN" "$WEB_BIN"; do
  if [[ ! -x "$binary" ]]; then
    log "FAIL: required executable is missing: $(basename "$binary")"
    exit 1
  fi

  description="$(file -b "$binary")"
  log "$(basename "$binary"): $description"
  if ! grep -Eq 'ELF 64-bit.*(x86-64|x86_64)' <<< "$description"; then
    log "FAIL: $(basename "$binary") is not a Linux x86-64 ELF executable."
    exit 1
  fi

  libraries="$(ldd "$binary" 2>&1 || true)"
  if grep -q 'not found' <<< "$libraries"; then
    log "FAIL: missing shared library dependency for $(basename "$binary")."
    printf '%s\n' "$libraries"
    exit 1
  fi
  log "Shared library inspection passed for $(basename "$binary")."
done

if [[ ! -f "$UNPACKED/README.md" || ! -f "$UNPACKED/PROJECT.md" ]]; then
  log "FAIL: release documentation is missing."
  exit 1
fi
log "Release archive structure passed."

log "Starting gemini-web2api on loopback port $PORT."
GEMINI_WEB2API_HOST=127.0.0.1 \
GEMINI_WEB2API_PORT="$PORT" \
GEMINI_WEB2API_COOKIE_FILE="$COOKIE_FILE" \
GEMINI_WEB2API_LOG_REQUESTS=false \
RUST_LOG=llm_provider::client=debug \
"$WEB_BIN" > "$SERVER_RAW" 2>&1 &
SERVER_PID=$!

if ! grep -Eq 'cookies loaded: [0-9]+ pairs, SAPISID present' "$SERVER_RAW" 2>/dev/null; then
  # Allow a short startup window for the cookie-loader diagnostic.
  for attempt in $(seq 1 10); do
    sleep 1
    if grep -Eq 'cookies loaded: [0-9]+ pairs, SAPISID present' "$SERVER_RAW" 2>/dev/null; then
      break
    fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
      break
    fi
  done
fi
if ! grep -Eq 'cookies loaded: [0-9]+ pairs, SAPISID present' "$SERVER_RAW" 2>/dev/null; then
  log "FAIL: server did not confirm loading a SAPISID cookie (cookie value is never printed)."
  exit 1
fi
log "Cookie parser smoke test passed (SAPISID present; value omitted)."

ROOT_JSON="$WORK_DIR/root.json"
ROOT_OK=false
for attempt in $(seq 1 60); do
  if curl --silent --fail --max-time 2 "$BASE_URL/" -o "$ROOT_JSON" 2>/dev/null; then
    ROOT_OK=true
    break
  fi
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    break
  fi
  sleep 1
done

if [[ "$ROOT_OK" != true ]]; then
  log "FAIL: HTTP server did not become ready at $BASE_URL/."
  exit 1
fi
if ! jq -e '.status == "ok" and (.models | type == "array" and length > 0) and (.defaultModel | type == "string" and length > 0)' "$ROOT_JSON" >/dev/null; then
  log "FAIL: root endpoint returned an unexpected JSON shape."
  exit 1
fi
DEFAULT_MODEL="$(jq -r '.defaultModel' "$ROOT_JSON")"
log "Root endpoint passed; configured model: $DEFAULT_MODEL."

MODELS_JSON="$WORK_DIR/models.json"
if ! curl --silent --fail --max-time 10 "$BASE_URL/v1/models" -o "$MODELS_JSON" 2>/dev/null; then
  log "FAIL: /v1/models returned a failed HTTP response."
  exit 1
fi
if ! jq -e '.object == "list" and (.data | type == "array" and length > 0)' "$MODELS_JSON" >/dev/null; then
  log "FAIL: /v1/models returned an unexpected JSON shape."
  exit 1
fi
log "OpenAI-compatible model-list endpoint passed."

if [[ "$(printf '%s' "$RUN_LIVE_INTEGRATION" | tr '[:upper:]' '[:lower:]')" == "true" ]]; then
  log "Running opt-in live Gemini request; response content will not be logged."
  REQUEST_JSON="$WORK_DIR/live-request.json"
  RESPONSE_JSON="$WORK_DIR/live-response.json"
  jq -n --arg model "$DEFAULT_MODEL" '{model:$model, messages:[{role:"user",content:"Reply with the exact text: RELEASE_SMOKE_TEST_OK"}], stream:false}' > "$REQUEST_JSON"
  HTTP_STATUS="$(curl --silent --max-time 120 --connect-timeout 5 -H 'Content-Type: application/json' -d @"$REQUEST_JSON" -o "$RESPONSE_JSON" -w '%{http_code}' "$BASE_URL/v1/chat/completions" 2>/dev/null || true)"
  if [[ "$HTTP_STATUS" != "200" ]]; then
    log "FAIL: live Gemini request returned HTTP status $HTTP_STATUS; response body suppressed."
    exit 1
  fi
  if ! jq -e '.choices | type == "array" and length > 0 and (.[0].message.content | type == "string" and length > 0)' "$RESPONSE_JSON" >/dev/null; then
    log "FAIL: live Gemini response did not match the expected API schema; response body suppressed."
    exit 1
  fi
  log "Live Gemini request passed (response text suppressed)."
else
  log "Live Gemini generation was not requested; no completion prompt was sent."
fi

log "All selected release checks passed."
