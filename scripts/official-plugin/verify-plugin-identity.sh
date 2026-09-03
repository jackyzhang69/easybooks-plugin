#!/usr/bin/env bash
# Fail-closed plugin identity gate against the pinned platform registry.
#
# Resolves platform governance at PLATFORM_GOVERNANCE_REV (single source of pin),
# then runs governance/scripts/verify_plugin_identity.py without live ACCOUNTD_APPS.
#
# Usage: verify-plugin-identity.sh <package-dir>
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PIN_FILE="$HERE/PLATFORM_GOVERNANCE_REV"
PIN="$(tr -d '[:space:]' < "$PIN_FILE")"
PACKAGE_DIR="${1:?usage: verify-plugin-identity.sh <package-dir>}"
CLONED_TMP=""

cleanup() {
  if [ -n "$CLONED_TMP" ] && [ -d "$CLONED_TMP" ]; then
    rm -rf "$CLONED_TMP"
  fi
}
trap cleanup EXIT

if [[ ! "$PIN" =~ ^[0-9a-fA-F]{40}$ ]]; then
  echo "verify-plugin-identity: invalid PLATFORM_GOVERNANCE_REV (expected 40-char hex): $PIN" >&2
  exit 2
fi

if [ ! -d "$PACKAGE_DIR" ]; then
  echo "verify-plugin-identity: package dir is not a directory: $PACKAGE_DIR" >&2
  exit 2
fi

platform_usable() {
  local dir="$1"
  git -C "$dir" rev-parse --git-dir >/dev/null 2>&1 || return 1
  local head
  head="$(git -C "$dir" rev-parse HEAD)"
  [ "$head" = "$PIN" ] && return 0
  git -C "$dir" merge-base --is-ancestor "$PIN" HEAD 2>/dev/null
}

resolve_platform_root() {
  if [ -n "${PLATFORM_GOVERNANCE_ROOT:-}" ]; then
    if ! platform_usable "$PLATFORM_GOVERNANCE_ROOT"; then
      local head
      head="$(git -C "$PLATFORM_GOVERNANCE_ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
      echo "verify-plugin-identity: PLATFORM_GOVERNANCE_ROOT at $head does not match pin $PIN" >&2
      exit 2
    fi
    printf '%s\n' "$PLATFORM_GOVERNANCE_ROOT"
    return
  fi

  local local_platform="/Users/jacky/platform"
  if platform_usable "$local_platform"; then
    printf '%s\n' "$local_platform"
    return
  fi

  CLONED_TMP="$(mktemp -d "${TMPDIR:-/tmp}/easybooks-platform-governance.XXXXXX")"
  git clone --quiet https://github.com/jackyzhang69/platform.git "$CLONED_TMP/platform"
  git -C "$CLONED_TMP/platform" checkout --quiet "$PIN"
  printf '%s\n' "$CLONED_TMP/platform"
}

PLATFORM_ROOT="$(resolve_platform_root)"
VERIFIER="$PLATFORM_ROOT/governance/scripts/verify_plugin_identity.py"
if [ ! -f "$VERIFIER" ]; then
  echo "verify-plugin-identity: verifier missing at $VERIFIER" >&2
  exit 2
fi

JSON_OUT="$(python3 "$VERIFIER" easybooks --package-dir "$PACKAGE_DIR" --json)"
printf '%s\n' "$JSON_OUT"

RESULT="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["result"])' <<< "$JSON_OUT")"
if [ "$RESULT" != "OK" ]; then
  echo "verify-plugin-identity: result=$RESULT (expected OK)" >&2
  exit 1
fi
