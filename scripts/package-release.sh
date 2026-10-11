#!/usr/bin/env bash
set -Eeuo pipefail
set +x

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "Usage: bash scripts/package-release.sh <output-directory> [vMAJOR.MINOR.PATCH]" >&2
  exit 2
fi

OUT_DIR="$1"
REQUESTED_TAG=""
if [[ $# -eq 2 ]]; then REQUESTED_TAG="$2"; fi
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"

command -v jq >/dev/null || { echo "jq is required" >&2; exit 1; }

VERSION="$(cargo metadata --locked --no-deps --format-version 1 | jq -er '[.packages[] | select(.name == "acp-adaptor" or .name == "llm-provider") | .version] | unique | if length == 1 then .[0] else error("workspace package versions are inconsistent") end')"
TAG="v$VERSION"
if [[ -n "$REQUESTED_TAG" ]]; then TAG="$REQUESTED_TAG"; fi

if ! [[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Release tag must use vMAJOR.MINOR.PATCH (received: $TAG)" >&2
  exit 1
fi
if [[ "$TAG" != "v$VERSION" ]]; then
  echo "Release tag $TAG does not match Cargo workspace version v$VERSION" >&2
  exit 1
fi

echo "Building Linux x86-64 release $TAG."
cargo build --locked --release --bin gemini-acp --bin gemini-web2api

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT
STAGE="$TEMP_DIR/stage"
mkdir -p "$STAGE"

for binary in gemini-acp gemini-web2api; do
  source_file="target/release/$binary"
  if [[ ! -x "$source_file" ]]; then
    echo "Expected executable not found: $source_file" >&2
    exit 1
  fi
  cp "$source_file" "$STAGE/$binary"
done
cp README.md PROJECT.md "$STAGE/"

ASSET_NAME="ny-gemini-acp-$TAG-linux-x86_64.tar.gz"
CHECKSUM_NAME="$ASSET_NAME.sha256"
ARCHIVE_PATH="$OUT_DIR/$ASSET_NAME"
CHECKSUM_PATH="$OUT_DIR/$CHECKSUM_NAME"

tar -czf "$ARCHIVE_PATH" -C "$STAGE" gemini-acp gemini-web2api README.md PROJECT.md
(
  cd "$OUT_DIR"
  sha256sum "$ASSET_NAME" > "$CHECKSUM_NAME"
)

echo "Created Linux release archive: $ARCHIVE_PATH"
echo "Created SHA-256 checksum: $CHECKSUM_PATH"

if printenv GITHUB_OUTPUT >/dev/null 2>&1; then
  {
    printf 'tag=%s\n' "$TAG"
    printf 'archive_path=%s\n' "$ARCHIVE_PATH"
    printf 'checksum_path=%s\n' "$CHECKSUM_PATH"
  } >> "$GITHUB_OUTPUT"
fi
