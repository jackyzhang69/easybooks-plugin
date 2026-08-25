#!/usr/bin/env python3
"""Fail-closed skill ↔ CLI surface contract.

Usage:
  python3 assert-skill-surface.py --bin <absolute path of the binary being published> \\
                                  --skills <skills/<product> dir>

This script is the mechanical substitute for “skills cannot stay frozen when
the CLI surface changes”. It never consults PATH. Missing `commands --json`
is FAIL, never skip.

Direction B (coverage, required):
  Every public commands[].path from `<bin> commands --json` must appear at
  least once as a contiguous token sequence in SKILL.md or references/*.md.
  Matching accepts backtick-quoted or unquoted joined paths with word
  boundaries (e.g. `feedback create`, anychat query).

Direction A (backtick contract, required — not a no-op):
  A backtick-quoted token is treated as a CLI invocation when:
    1. it starts with the binary name, `$BIN`, `$<PRODUCT>_BIN`,
       `"$BIN"`, `"$<PRODUCT>_BIN"`, or `& $env:<PRODUCT>_BIN`, or
    2. the first non-flag token is a known public top-level command AND the
       binary name appears on the same line.
  Remaining tokens up to the first flag (`-…`) form a command path. That
  path is valid if it equals a public path, is a prefix of a public path,
  or a public path is a prefix of it (over-specified subcommand). FAIL if a
  backtick invocation names a command that is not public.

Unquoted `<bin> <subcommand>` mentions (not in backticks) are WARN only:
they give false confidence. Skills should backtick-quote live invocations.
WARN does not fail the process.

Trigger preservation (if references/triggers.md exists):
  Each markdown bullet in that file must appear in SKILL.md (frontmatter +
  the first 80 lines). This replaces “fresh-session trigger evaluation”.

Hidden/admin/debug commands must not appear in commands --json; this script
does not second-guess the CLI dump. Volatile catalogs (form lists) are not
part of the dump by contract.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path


SCHEMA = "plugin-cli-commands-v1"
FIRST_SCREEN_LINES = 80
HIDDEN_TOP_LEVEL = {"admin", "help", "debug", "internal"}

BIN_ENV_RE = re.compile(
    r"""^(?:
            \$\{?(?:BIN|[A-Z][A-Z0-9_]*_BIN)\}? |
            "(?:\$\{?(?:BIN|[A-Z][A-Z0-9_]*_BIN)\}?)" |
            (?:\&\s+)?\$env:[A-Z][A-Z0-9_]*_BIN
        )$""",
    re.VERBOSE,
)


class Check:
    def __init__(self) -> None:
        self.failures: list[str] = []
        self.warnings: list[str] = []

    def fail(self, msg: str) -> None:
        self.failures.append(msg)

    def warn(self, msg: str) -> None:
        self.warnings.append(msg)


def die(msg: str, code: int = 2) -> None:
    print(f"assert-skill-surface: {msg}", file=sys.stderr)
    raise SystemExit(code)


def load_markdown(skills_dir: Path) -> tuple[Path, str, str]:
    skill = skills_dir / "SKILL.md"
    if not skill.is_file():
        die(f"missing router SKILL.md under {skills_dir}")
    parts = [skill.read_text(encoding="utf-8")]
    refs = skills_dir / "references"
    if refs.is_dir():
        for path in sorted(refs.rglob("*.md")):
            if path.name == "SKILL.md":
                die(f"SKILL.md is not allowed under references/: {path}")
            parts.append(path.read_text(encoding="utf-8"))
    combined = "\n".join(parts)
    router = parts[0]
    return skill, router, combined


def run_commands_json(bin_path: Path) -> dict:
    if not bin_path.is_absolute():
        die(f"--bin must be an absolute path, got {bin_path}")
    if not bin_path.is_file():
        die(f"--bin is not a file: {bin_path}", 1)
    argv = [str(bin_path), "commands", "--json"]
    # Source-tree Python clients are not execve-able on every host; invoke
    # them through this interpreter. Missing `commands --json` is still FAIL.
    if bin_path.suffix == ".py":
        argv = [sys.executable, str(bin_path), "commands", "--json"]
    try:
        proc = subprocess.run(
            argv,
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError as exc:
        die(f"could not execute {bin_path}: {exc}", 1)
    if proc.returncode != 0:
        die(
            "commands --json failed "
            f"(exit {proc.returncode}; missing command is FAIL, never skip)\n"
            f"stdout: {proc.stdout[:500]}\nstderr: {proc.stderr[:500]}",
            1,
        )
    raw = proc.stdout.strip()
    if not raw:
        die("commands --json produced empty stdout", 1)
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as exc:
        die(f"commands --json is not JSON: {exc}\n{raw[:500]}", 1)
    if not isinstance(payload, dict):
        die("commands --json must be a JSON object", 1)
    return payload


def validate_schema(payload: dict, bin_path: Path, check: Check) -> list[list[str]]:
    if payload.get("schema") != SCHEMA:
        check.fail(f"schema is {payload.get('schema')!r}, expected {SCHEMA!r}")
    if not isinstance(payload.get("bin"), str) or not payload["bin"]:
        check.fail("bin is missing")
    if not isinstance(payload.get("version"), str) or not payload["version"]:
        check.fail("version is missing")
    commands = payload.get("commands")
    if not isinstance(commands, list) or not commands:
        check.fail("commands must be a non-empty array")
        return []
    paths: list[list[str]] = []
    seen: set[tuple[str, ...]] = set()
    for item in commands:
        if not isinstance(item, dict) or not isinstance(item.get("path"), list):
            check.fail(f"command entry is not {{path: [...]}}: {item!r}")
            continue
        path = [str(part) for part in item["path"]]
        if not path or any(not part for part in path):
            check.fail(f"empty command path: {item!r}")
            continue
        if path[0] in HIDDEN_TOP_LEVEL:
            check.fail(f"hidden/admin/debug path leaked into public dump: {' '.join(path)}")
            continue
        key = tuple(path)
        if key in seen:
            check.fail(f"duplicate command path: {' '.join(path)}")
            continue
        seen.add(key)
        paths.append(path)
    return paths


def word_boundary_present(haystack: str, needle: str) -> bool:
    return re.search(r"(?<![\w-])" + re.escape(needle) + r"(?![\w-])", haystack) is not None


def direction_b(paths: list[list[str]], combined: str, check: Check) -> None:
    for path in paths:
        joined = " ".join(path)
        if word_boundary_present(combined, joined):
            continue
        check.fail(
            f"Direction B: public command `{joined}` is not mentioned in SKILL.md or references/"
        )


def tokenize_backtick(span: str) -> list[str]:
    span = span.strip()
    span = span.replace("\n", " ")
    # Strip wrapping quotes left after markdown extraction.
    parts = re.findall(r"""[^\s"'`]+|"[^"]*"|'[^']*'""", span)
    out: list[str] = []
    for part in parts:
        if (part.startswith('"') and part.endswith('"')) or (
            part.startswith("'") and part.endswith("'")
        ):
            part = part[1:-1]
        if part:
            out.append(part)
    return out


def looks_like_bin_token(token: str, bin_name: str) -> bool:
    token = token.strip()
    if token in {bin_name, f"{bin_name}.exe"}:
        return True
    if BIN_ENV_RE.match(token):
        return True
    if token.endswith(f"/{bin_name}") or token.endswith(f"\\{bin_name}") or token.endswith(
        f"\\{bin_name}.exe"
    ):
        return True
    return False


def path_from_tokens(tokens: list[str], bin_name: str, top_level: set[str]) -> list[str] | None:
    if not tokens:
        return None
    idx = 0
    if looks_like_bin_token(tokens[0], bin_name):
        idx = 1
    elif tokens[0] in top_level:
        idx = 0
    else:
        return None
    path: list[str] = []
    while idx < len(tokens):
        tok = tokens[idx]
        if tok.startswith("-"):
            break
        if tok.startswith("<") or tok.startswith("$") or tok in {"…", "..."}:
            break
        path.append(tok)
        idx += 1
    return path or None


def path_known(candidate: list[str], public: list[list[str]]) -> bool:
    if not candidate:
        return False
    for path in public:
        if path == candidate:
            return True
        if path[: len(candidate)] == candidate:
            return True
        if candidate[: len(path)] == path:
            return True
    return False


def direction_a(
    combined: str,
    router_lines: list[str],
    bin_name: str,
    public: list[list[str]],
    check: Check,
) -> None:
    top_level = {path[0] for path in public}
    backtick_re = re.compile(r"`([^`]+)`")
    line_starts = [0]
    for line in combined.splitlines(True):
        line_starts.append(line_starts[-1] + len(line))

    def line_of(pos: int) -> str:
        # Approximate: find the line containing pos.
        running = 0
        for line in combined.splitlines():
            nxt = running + len(line) + 1
            if pos < nxt:
                return line
            running = nxt
        return ""

    for match in backtick_re.finditer(combined):
        span = match.group(1)
        tokens = tokenize_backtick(span)
        if not tokens:
            continue
        line = line_of(match.start())
        starts_with_bin = looks_like_bin_token(tokens[0], bin_name)
        first_cmd = None
        # Skip leading flags after an optional bin token.
        scan = tokens[1:] if starts_with_bin else tokens
        for tok in scan:
            if tok.startswith("-"):
                continue
            first_cmd = tok
            break
        same_line_has_bin = bin_name in line or looks_like_bin_token(
            tokens[0], bin_name
        )
        if not starts_with_bin and not (
            first_cmd in top_level and same_line_has_bin
        ):
            continue
        path = path_from_tokens(tokens, bin_name, top_level)
        if not path:
            continue
        if not path_known(path, public):
            check.fail(
                "Direction A: backtick `"
                + span
                + f"` looks like `{bin_name} {' '.join(path)}` "
                "but that path is not in commands --json"
            )

    # Unquoted `<bin> <subcommand>` — warn only.
    unquoted = re.compile(
        rf"(?<![\w`$-]){re.escape(bin_name)}(?:\.exe)?\s+([a-z][\w-]*)",
        re.IGNORECASE,
    )
    for match in unquoted.finditer(combined):
        # Skip if this occurrence sits inside backticks: the preceding char
        # was already excluded for ` but inner spans can still match. Cheap
        # check: if the nearest backticks wrap it, skip.
        start = match.start()
        before = combined.rfind("`", 0, start)
        after = combined.find("`", start)
        if before != -1 and after != -1 and combined[before:after].count("`") % 2 == 1:
            continue
        cmd = match.group(1)
        if cmd in top_level or any(p[:1] == [cmd] for p in public):
            check.warn(
                f"unquoted `{bin_name} {cmd}` — quote live CLI invocations in backticks"
            )


def check_triggers(skills_dir: Path, router: str, check: Check) -> None:
    triggers = skills_dir / "references" / "triggers.md"
    if not triggers.is_file():
        check.fail("missing references/triggers.md (mechanical trigger preservation)")
        return
    phrases: list[str] = []
    for line in triggers.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped.startswith("- "):
            phrases.append(stripped[2:].strip().strip('"').strip("'"))
    if not phrases:
        check.fail("references/triggers.md has no `- ` phrases")
        return
    first_screen = "\n".join(router.splitlines()[:FIRST_SCREEN_LINES])
    for phrase in phrases:
        if not phrase:
            continue
        if phrase not in first_screen:
            check.fail(
                f"trigger phrase {phrase!r} is missing from SKILL.md frontmatter + first {FIRST_SCREEN_LINES} lines"
            )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--bin", required=True, help="absolute path of the binary being published")
    parser.add_argument("--skills", required=True, help="skills/<product> directory")
    args = parser.parse_args(argv)

    bin_path = Path(args.bin)
    skills_dir = Path(args.skills).expanduser().resolve()
    if not skills_dir.is_dir():
        die(f"--skills is not a directory: {skills_dir}")

    check = Check()
    _skill_path, router, combined = load_markdown(skills_dir)
    payload = run_commands_json(bin_path)
    paths = validate_schema(payload, bin_path, check)
    bin_name = str(payload.get("bin") or bin_path.stem)
    if paths:
        direction_b(paths, combined, check)
        direction_a(combined, router.splitlines(), bin_name, paths, check)
    check_triggers(skills_dir, router, check)

    for warning in check.warnings:
        print(f"WARN  {warning}")
    if check.failures:
        for failure in check.failures:
            print(f"FAIL  {failure}")
        print(f"RESULT: FAIL ({len(check.failures)} failure(s), {len(check.warnings)} warning(s))")
        return 1
    print(
        f"ok    {bin_name} commands --json ({len(paths)} public paths) matches {skills_dir}"
    )
    print("RESULT: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
