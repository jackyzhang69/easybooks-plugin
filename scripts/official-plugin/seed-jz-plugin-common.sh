#!/usr/bin/env bash
# Verify vendor/jz-plugin-common matches the git rev pinned in Cargo.toml.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
VENDOR="$ROOT/vendor/jz-plugin-common"
CARGO=""
for candidate in "$ROOT/Cargo.toml" "$ROOT/cli/Cargo.toml"; do
  [ -f "$candidate" ] || continue
  if grep -q 'jz-plugin-common' "$candidate"; then
    CARGO="$candidate"
    break
  fi
done
[ -n "$CARGO" ] || { echo "seed-jz-plugin-common: Cargo.toml with jz-plugin-common not found" >&2; exit 1; }
REV="$(python3 - "$CARGO" <<'PY'
from pathlib import Path
import re
import sys
text = Path(sys.argv[1]).read_text()
match = re.search(r'jz-plugin-common\s*=\s*\{[^}]*rev\s*=\s*"([0-9a-fA-F]+)"', text)
if not match:
    raise SystemExit("jz-plugin-common rev missing from Cargo.toml")
print(match.group(1))
PY
)"
if [ ! -d "$VENDOR/src" ] || [ ! -f "$VENDOR/Cargo.toml" ]; then
  echo "seed-jz-plugin-common: missing vendor snapshot at $VENDOR" >&2
  exit 1
fi
PIN="$(tr -d '[:space:]' < "$VENDOR/SOURCE_REV")"
if [ "$PIN" != "$REV" ]; then
  echo "seed-jz-plugin-common: vendor SOURCE_REV $PIN != Cargo.toml rev $REV" >&2
  exit 1
fi
PLATFORM_ROOT=""
for candidate in \
  "${PLATFORM_GOVERNANCE_ROOT:-}" \
  /Users/jacky/platform \
  /Users/jacky/.local/share/platform-governance
do
  [ -n "$candidate" ] || continue
  if git -C "$candidate" cat-file -e "${REV}^{commit}" 2>/dev/null; then
    PLATFORM_ROOT="$candidate"
    break
  fi
done
if [ -n "$PLATFORM_ROOT" ]; then
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/jz-plugin-common-pin.XXXXXX")"
  cleanup() { rm -rf "$tmp"; }
  trap cleanup EXIT
  git -C "$PLATFORM_ROOT" archive "$REV" crates/jz-plugin-common | tar -x -C "$tmp"
  diff -u "$tmp/crates/jz-plugin-common/Cargo.toml" "$VENDOR/Cargo.toml"
  diff -ru "$tmp/crates/jz-plugin-common/src" "$VENDOR/src"
fi
