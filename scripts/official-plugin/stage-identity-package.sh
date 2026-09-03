#!/usr/bin/env bash
# Assemble a read-only plugin tree sufficient for verify_plugin_identity.py.
# Combines plugin-metadata manifests with the canonical skill mirror.
#
# Usage: stage-identity-package.sh <out-dir>
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${1:?usage: stage-identity-package.sh <out-dir>}"

rm -rf "$OUT"
mkdir -p "$OUT/skills"

cp -R "$ROOT/plugin-metadata/.claude-plugin" "$OUT/"
cp -R "$ROOT/plugin-metadata/.codex-plugin" "$OUT/"
cp "$ROOT/plugin-metadata/runtime-manifest.json" "$OUT/"
cp -R "$ROOT/.claude/skills/easybooks" "$OUT/skills/easybooks"

echo "staged identity package at $OUT"
