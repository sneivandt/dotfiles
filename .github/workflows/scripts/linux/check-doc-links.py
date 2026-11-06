#!/usr/bin/env python3
"""Check local inline Markdown links and heading anchors, preserving filenames."""

import os
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote
from urllib.parse import urlsplit


# Inline links/images only; unrelated external URLs remain out of scope.
LINK = re.compile(r'\]\(\s*(?:<([^>\n]*)>|((?:\\.|[^\s()\\]|\([^()]*\))+))')


def repository_destination(root, target):
    url = urlsplit(target)
    prefix = "/sneivandt/dotfiles/"
    if url.scheme != "https" or url.netloc != "github.com" or not url.path.startswith(prefix):
        return None
    parts = unquote(url.path[len(prefix):]).split("/")
    if len(parts) < 3 or parts[0] not in {"blob", "tree"} or parts[1] != "main":
        return None
    relative = Path(*parts[2:])
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError("repository source path must stay inside the checkout")
    resolved = (root / relative).resolve()
    if not resolved.is_relative_to(root.resolve()):
        raise ValueError("repository source path resolves outside the checkout")
    return resolved, unquote(url.fragment), parts[0]


def markdown_files(root):
    if subprocess.run(["git", "-C", str(root), "rev-parse", "--is-inside-work-tree"],
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0:
        paths = subprocess.check_output([
            "git", "-C", str(root), "ls-files", "--cached", "--others",
            "--exclude-standard", "-z", "*.md",
        ])
        return [root / os.fsdecode(path) for path in paths.split(b"\0")
                if path and (root / os.fsdecode(path)).is_file()]
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
            try:
                source = repository_destination(root, target)
            except ValueError as error:
                failures.append(f"{doc.relative_to(root).as_posix()}: invalid repository source -> {target}: {error}")
                continue
            if source is not None:
                resolved, fragment, kind = source
                if not resolved.exists() or (kind == "blob" and not resolved.is_file()) or (
                    kind == "tree" and not resolved.is_dir()
                ):
                    failures.append(f"{doc.relative_to(root).as_posix()}: broken repository source -> {target}")
                elif fragment and resolved.is_file() and re.fullmatch(r"L\d+(?:-L\d+)?", fragment):
                    lines = [int(value) for value in re.findall(r"\d+", fragment)]
                    count = len(resolved.read_text(encoding="utf-8").splitlines())
                    if not 1 <= lines[0] <= lines[-1] <= count:
                        failures.append(f"{doc.relative_to(root).as_posix()}: broken repository line range -> {target}")
                elif fragment and resolved.is_file() and resolved.suffix == ".md":
                    if fragment not in markdown_anchors(resolved.read_text(encoding="utf-8")):
                        failures.append(f"{doc.relative_to(root).as_posix()}: broken repository anchor -> {target}")
                continue
            if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", target) or target.startswith("//"):
                continue
            path, _, fragment = target.partition("#")
            path = unquote(path.split("?", 1)[0])
            resolved = (root / path.lstrip("/") if path.startswith("/") else doc.parent / path) if path else doc
            if not resolved.exists():
                failures.append(f"{doc.relative_to(root).as_posix()}: broken link -> {target}")
            elif fragment and resolved.suffix == ".md":
                anchors = markdown_anchors(resolved.read_text(encoding="utf-8"))
                if unquote(fragment) not in anchors:
                    failures.append(f"{doc.relative_to(root).as_posix()}: broken anchor -> {target}")
    return failures


if __name__ == "__main__":
    errors = check(Path(sys.argv[1]).resolve())
    for error in errors:
        print(error)
    sys.exit(bool(errors))
