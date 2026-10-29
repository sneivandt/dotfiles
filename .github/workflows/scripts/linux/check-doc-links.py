#!/usr/bin/env python3
"""Check local inline Markdown links and heading anchors, preserving filenames."""

import os
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote


# Inline links/images only; reference links and external URLs remain out of scope.
LINK = re.compile(r'\]\(\s*(?:<([^>\n]*)>|((?:\\.|[^\s()\\]|\([^()]*\))+))')


def markdown_files(root):
    if subprocess.run(["git", "-C", str(root), "rev-parse", "--is-inside-work-tree"],
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0:
        paths = subprocess.check_output(["git", "-C", str(root), "ls-files", "-z", "*.md"])
        return [root / os.fsdecode(path) for path in paths.split(b"\0") if path]
    return [path for path in root.rglob("*.md") if ".git" not in path.relative_to(root).parts]


def markdown_anchors(text):
    anchors = set()
    seen = {}
    fence = None
    for line in text.splitlines():
        marker = re.match(r"^\s*(`{3,}|~{3,})", line)
        if marker:
            value = marker[1]
            if fence is None:
                fence = value
            elif value[0] == fence[0] and len(value) >= len(fence):
                fence = None
            continue
        heading = re.match(r"^#{1,6}\s+(.+)", line)
        if fence is not None or heading is None:
            continue
        slug = re.sub(r"\s+#+\s*$", "", heading[1]).lower()
        slug = re.sub(r"<[^>]*>", "", slug)
        slug = re.sub(r"[`*~]", "", slug)
        slug = re.sub(r"[^\w -]", "", slug).replace(" ", "-")
        count = seen.get(slug, 0)
        seen[slug] = count + 1
        anchors.add(slug if count == 0 else f"{slug}-{count}")
    return anchors


def check(root):
    failures = []
    for doc in markdown_files(root):
        for match in LINK.finditer(doc.read_text(encoding="utf-8")):
            target = match[1] if match[1] is not None else match[2]
            target = re.sub(r"\\([!\"#$%&'()*+,\-./:;<=>?@\[\]\\^_`{|}~])", r"\1", target)
            if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", target) or target.startswith("//"):
                continue
            path, _, fragment = target.partition("#")
            path = unquote(path.split("?", 1)[0])
            resolved = (root / path.lstrip("/") if path.startswith("/") else doc.parent / path) if path else doc
            if not resolved.exists():
                failures.append(f"{doc.relative_to(root)}: broken link -> {target}")
            elif fragment and resolved.suffix == ".md":
                anchors = markdown_anchors(resolved.read_text(encoding="utf-8"))
                if unquote(fragment) not in anchors:
                    failures.append(f"{doc.relative_to(root)}: broken anchor -> {target}")
    return failures


if __name__ == "__main__":
    errors = check(Path(sys.argv[1]).resolve())
    for error in errors:
        print(error)
    sys.exit(bool(errors))
