#!/usr/bin/env bash
# Build the EasyBooks CLI for local Darwin/arm64 and stage it into bin/darwin-arm64/.
#
# This is the dev counterpart to .github/workflows/publish.yml: it produces the
# single binary the binary resolver (CONTRACT §0) looks for at
# $CLAUDE_PLUGIN_ROOT/bin/<platform>/easybooks. No lazy assets, no bun, no pdf.
#
# Optional codesign: set CODESIGN_IDENTITY to a valid signing identity to sign
# the staged binary (e.g. "Developer ID Application: Your Name (TEAMID)").
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TARGET="aarch64-apple-darwin"
PLATFORM="darwin-arm64"
BIN_NAME="easybooks"

cargo build --release --target "$TARGET" --bin "$BIN_NAME"

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
SRC="$TARGET_DIR/$TARGET/release/$BIN_NAME"
DEST_DIR="bin/$PLATFORM"
DEST="$DEST_DIR/$BIN_NAME"

mkdir -p "$DEST_DIR"
cp "$SRC" "$DEST"
chmod +x "$DEST"

if [[ -n "${CODESIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --sign "$CODESIGN_IDENTITY" "$DEST"
  echo "{\"event\":\"codesign\",\"identity\":\"$CODESIGN_IDENTITY\",\"binary\":\"$DEST\"}"
else
  echo "{\"event\":\"info\",\"detail\":\"CODESIGN_IDENTITY not set; staged unsigned binary\"}"
fi

echo "{\"event\":\"build-local\",\"status\":\"ok\",\"platform\":\"$PLATFORM\",\"binary\":\"$DEST\"}"
