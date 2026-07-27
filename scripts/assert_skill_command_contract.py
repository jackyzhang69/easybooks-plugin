#!/usr/bin/env python3
"""Keep agent-facing EasyBooks commands aligned with the CLI parser contract."""

from __future__ import annotations

import json
from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[1]
SKILL_ROOTS = (ROOT / ".claude/skills", ROOT / ".agents/skills")
SUBCOMMANDS_WITH_TOP_LEVEL_JSON = ("doctor", "whoami", "login")


def main() -> int:
    failures: list[str] = []
    checked_files = 0
    for skill_root in SKILL_ROOTS:
        for path in sorted(skill_root.glob("*/SKILL.md")):
            checked_files += 1
            source = path.read_text(encoding="utf-8")
            for command in SUBCOMMANDS_WITH_TOP_LEVEL_JSON:
                pattern = re.compile(rf"\b{command}\s+--json\b")
                for match in pattern.finditer(source):
                    line = source.count("\n", 0, match.start()) + 1
                    failures.append(
                        f"{path.relative_to(ROOT)}:{line}: --json must precede the {command} subcommand"
                    )

    required_examples = {
        ROOT / ".claude/skills/connect-easybooks/SKILL.md": (
            '"$EASYBOOKS_BIN" --json doctor --no-fetch --check-upgrade',
        ),
        ROOT / ".claude/skills/easybooks-capabilities/SKILL.md": (
            "easybooks --json doctor",
        ),
    }
    for path, examples in required_examples.items():
        source = path.read_text(encoding="utf-8")
        for example in examples:
            if example not in source:
                failures.append(f"{path.relative_to(ROOT)}: missing required example: {example}")

    result = {
        "ok": not failures,
        "checked_files": checked_files,
        "failures": failures,
    }
    print(json.dumps(result, sort_keys=True))
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
