#!/usr/bin/env bash
# Assert that .claude/skills and .agents/skills are byte-identical (CONTRACT §4).
#
# The two skill mirrors must never drift: Claude Code reads .claude/skills,
# Codex/marketplace packaging reads .agents/skills, and both must ship the same
# SKILL.md content.
#
#   scripts/sync-skills.sh          verify only; exit non-zero on any drift
#   scripts/sync-skills.sh --fix    copy .claude/skills -> .agents/skills, then verify
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CLAUDE_SKILLS="$ROOT/.claude/skills"
AGENTS_SKILLS="$ROOT/.agents/skills"

FIX=0
if [[ "${1:-}" == "--fix" ]]; then
  FIX=1
fi

if [[ ! -d "$CLAUDE_SKILLS" ]]; then
  echo "::error::source skills dir missing: $CLAUDE_SKILLS"
  exit 2
fi

if [[ "$FIX" -eq 1 ]]; then
  rm -rf "$AGENTS_SKILLS"
  mkdir -p "$AGENTS_SKILLS"
  # Copy the *contents* of .claude/skills into .agents/skills (mirror, not nest).
  if [[ -n "$(ls -A "$CLAUDE_SKILLS" 2>/dev/null)" ]]; then
    cp -R "$CLAUDE_SKILLS"/. "$AGENTS_SKILLS"/
  fi
  echo "::notice::synced .claude/skills -> .agents/skills"
fi

if [[ ! -d "$AGENTS_SKILLS" ]]; then
  echo "::error::mirror skills dir missing: $AGENTS_SKILLS (run with --fix)"
  exit 1
fi

# diff -r exits non-zero on any byte-level difference, extra, or missing file.
if diff -r "$CLAUDE_SKILLS" "$AGENTS_SKILLS"; then
  echo "::notice::skills mirrors byte-identical (.claude/skills == .agents/skills)"
  exit 0
else
  echo "::error::skills drift between .claude/skills and .agents/skills — run: scripts/sync-skills.sh --fix"
  exit 1
fi
