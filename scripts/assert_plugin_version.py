#!/usr/bin/env python3
"""Assert the EasyBooks tag, source metadata, lockfile, and optional binary agree."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib


def version_from_tag(tag: str) -> str:
    match = re.fullmatch(r"plugin-v(\d+\.\d+\.\d+)", tag)
    if not match:
        raise ValueError("release tag must match plugin-v<semver>")
    return match.group(1)


def collect(root: Path) -> dict[str, str]:
    cargo = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    package = next(item for item in lock["package"] if item["name"] == "easybooks-cli")
    return {
        "cargo_workspace": cargo["workspace"]["package"]["version"],
        "cargo_lock": package["version"],
        "codex_plugin": json.loads((root / "plugin-metadata/.codex-plugin/plugin.json").read_text())["version"],
        "claude_plugin": json.loads((root / "plugin-metadata/.claude-plugin/plugin.json").read_text())["version"],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag")
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()
    versions = collect(args.root)
    expected = version_from_tag(args.tag) if args.tag else versions["cargo_workspace"]
    mismatches = {name: value for name, value in versions.items() if value != expected}
    binary_version = None
    if args.binary:
        completed = subprocess.run(
            [str(args.binary), "--version"],
            capture_output=True,
            text=True,
            check=False,
            timeout=30,
        )
        binary_version = completed.stdout.strip().removeprefix("easybooks ")
        if completed.returncode or binary_version != expected:
            mismatches["binary"] = binary_version or f"exit:{completed.returncode}"
    result = {
        "ok": not mismatches,
        "expected_version": expected,
        "versions": versions,
        "binary_version": binary_version,
        "mismatches": mismatches,
    }
    print(json.dumps(result, sort_keys=True))
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, KeyError, ValueError, tomllib.TOMLDecodeError) as error:
        print(json.dumps({"ok": False, "error": str(error)}, sort_keys=True), file=sys.stderr)
        sys.exit(1)
