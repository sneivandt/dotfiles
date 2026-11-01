#!/usr/bin/env python3
"""Validate the repository's deliberately small YAML frontmatter convention.

Accept a plain name and an indented block description only. Reject unsupported
YAML forms rather than partially interpreting general YAML without a parser.
"""

from pathlib import Path
import re
import sys


NAME = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*")
RESERVED = {"true", "false", "yes", "no", "on", "off", "null"}


def metadata(text):
    lines = text.splitlines()
    if not lines or lines[0] != "---":
        raise ValueError("missing opening frontmatter delimiter")
    try:
        end = lines.index("---", 1)
    except ValueError as error:
        raise ValueError("missing closing frontmatter delimiter") from error
    fields = {}
    index = 1
    while index < end:
        line = lines[index]
        if not line.strip():
            index += 1
            continue
        match = re.fullmatch(r"(name|description): (.+)", line)
        if match is None:
            raise ValueError("expected only name and description metadata keys")
        key, value = match.groups()
        if key in fields:
            raise ValueError(f"duplicate metadata key: {key}")
        index += 1
        if key == "name":
            if not NAME.fullmatch(value) or value in RESERVED:
                raise ValueError("name must be a plain lowercase hyphenated string")
        else:
            if value not in {">", ">-", ">+", "|", "|-", "|+"}:
                raise ValueError("description must use an indented YAML block (> or |)")
            body = []
            while index < end and (not lines[index].strip() or lines[index].startswith("  ")):
                body.append(lines[index])
                index += 1
            if not any(line.strip() for line in body):
                raise ValueError("description must not be empty")
            # Fixed indentation avoids accepting tabs/indentation indicators as
            # though they were ordinary YAML block content.
            if any(line.strip() and (not line.startswith("  ") or line.startswith("   ")
                                     or "\t" in line[:2]) for line in body):
                raise ValueError("description lines must use two-space indentation")
            value = "\n".join(line[2:] for line in body)
        fields[key] = value
    missing = {"name", "description"} - fields.keys()
    if missing:
        raise ValueError("missing required metadata: " + ", ".join(sorted(missing)))
    return fields


def check(root):
    skills = root / ".agents/skills"
    files = sorted(skills.glob("*/SKILL.md"))
    failures = []
    names = {}
    if not files:
        return [".agents/skills: no skill entry points found"]
    for path in files:
        label = str(path.relative_to(root))
        try:
            fields = metadata(path.read_text(encoding="utf-8"))
        except (ValueError, OSError) as error:
            failures.append(f"{label}: {error}")
            continue
        name = fields["name"]
        if name != path.parent.name:
            failures.append(f"{label}: name {name!r} does not match directory {path.parent.name!r}")
        if name in names:
            failures.append(f"{label}: duplicate skill name {name!r}; also in {names[name]}")
        names[name] = label
    return failures


if __name__ == "__main__":
    root = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parents[4]
    errors = check(root)
    for error in errors:
        print(error)
    if not errors:
        print("Repository skill metadata passed")
    sys.exit(bool(errors))
