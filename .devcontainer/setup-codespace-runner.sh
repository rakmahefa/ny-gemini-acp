#!/usr/bin/env bash
set -euo pipefail

REPO="${CODESPACE_REPOSITORY:-}"
RUNNER_ROOT="${HOME}/.codespace-actions-runner"
RUNNER_STATE="${RUNNER_ROOT}/.runner"
RUNNER_PID="${RUNNER_ROOT}/runner.pid"

if [[ -z "$REPO" ]]; then
  echo "CODESPACE_REPOSITORY is not configured; skipping self-hosted runner setup."
  exit 0
fi

if ! command -v gh >/dev/null 2>&1; then
  echo "GitHub CLI (gh) is required for the Codespace runner setup." >&2
  exit 1
fi

# Codespaces secrets may be stored with a trailing CR/LF. Strip CR/LF before
# exporting the token so it remains valid as an HTTP Authorization header.
GH_TOKEN_RAW="${CODESPACE_RUNNER_PAT:-${GH_TOKEN:-}}"
GH_TOKEN="$(printf '%s' "$GH_TOKEN_RAW" | tr -d '\r\n')"
export GH_TOKEN
unset GH_TOKEN_RAW

if [[ -z "$GH_TOKEN" ]] && ! gh auth status >/dev/null 2>&1; then
  echo "Codespace runner is not registered: set the CODESPACE_RUNNER_PAT Codespaces secret or authenticate gh." >&2
  exit 0
fi

mkdir -p "$RUNNER_ROOT"

if [[ ! -f "${RUNNER_ROOT}/run.sh" ]]; then
  echo "Installing the latest GitHub Actions runner..."

  case "$(uname -m)" in
    x86_64) RUNNER_ARCH="x64" ;;
    aarch64|arm64) RUNNER_ARCH="arm64" ;;
    *)
      echo "Unsupported runner architecture: $(uname -m)" >&2
      exit 1
      ;;
  esac

  VERSION="$(gh api repos/actions/runner/releases/latest --jq '.tag_name' | sed 's/^v//')"
  ARCHIVE="actions-runner-linux-${RUNNER_ARCH}-${VERSION}.tar.gz"
  URL="https://github.com/actions/runner/releases/download/v${VERSION}/${ARCHIVE}"
  TMP_ARCHIVE="${RUNNER_ROOT}/${ARCHIVE}"

  curl -fsSL "$URL" -o "$TMP_ARCHIVE"
  tar -xzf "$TMP_ARCHIVE" -C "$RUNNER_ROOT"
  rm -f "$TMP_ARCHIVE"
fi

if [[ ! -f "$RUNNER_STATE" ]]; then
  echo "Registering Codespace as a self-hosted runner for ${REPO}..."

  REGISTRATION_TOKEN="$(gh api --method POST "repos/${REPO}/actions/runners/registration-token" --jq '.token')"
  CODESPACE_RUNNER_NAME="${CODESPACE_NAME:-ny-gemini-acp-codespace}"
  CODESPACE_RUNNER_NAME="$(printf '%s' "$CODESPACE_RUNNER_NAME" | tr -c '[:alnum:]_.-' '-')"
  CODESPACE_RUNNER_NAME="${CODESPACE_RUNNER_NAME:0:64}"

  "$RUNNER_ROOT/config.sh" \
    --unattended \
    --replace \
    --url "https://github.com/${REPO}" \
    --token "$REGISTRATION_TOKEN" \
    --name "$CODESPACE_RUNNER_NAME" \
    --labels "codespace" \
    --work "_work"
fi

if [[ -f "$RUNNER_PID" ]] && kill -0 "$(cat "$RUNNER_PID")" 2>/dev/null; then
  echo "Codespace Actions runner is already running."
  exit 0
fi

rm -f "$RUNNER_PID"

echo "Starting Codespace Actions runner..."
nohup "$RUNNER_ROOT/run.sh" >"${RUNNER_ROOT}/runner.log" 2>&1 &
echo $! > "$RUNNER_PID"

echo "Codespace Actions runner is running."
