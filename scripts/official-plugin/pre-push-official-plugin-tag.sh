#!/usr/bin/env bash
# Git pre-push helper: refuse plugin tags when GitHub secret names are missing.
# Reads stdin in git pre-push format: <local_ref> <local_sha> <remote_ref> <remote_sha>
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PLUGIN_ID="${OFFICIAL_PLUGIN_ID:-}"
if [ -z "$PLUGIN_ID" ]; then
  echo "pre-push-official-plugin-tag: set OFFICIAL_PLUGIN_ID" >&2
  exit 2
fi
while read -r local_ref _ remote_ref _; do
  case "$local_ref$remote_ref" in
    *refs/tags/plugin-v*|*refs/tags/anychat-v*|*refs/tags/anydoc-v*|*refs/tags/plugin-v*)
      python3 "$HERE/preflight-official-plugin-release.py" --plugin-id "$PLUGIN_ID"
      ;;
  esac
done
