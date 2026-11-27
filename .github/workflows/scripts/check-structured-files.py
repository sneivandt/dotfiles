#!/usr/bin/env python3
"""Validate tracked JSON, JSONC, TOML, YAML, and XML files without third-party dependencies."""

from __future__ import annotations

import fnmatch
import importlib.util
import json
import os
from pathlib import Path
import sys
import xml.etree.ElementTree as ET

import tomllib

SKIP_DIRS = frozenset(
    {
        ".git",
        "__pycache__",
    }
)

SKIP_REL_DIRS = frozenset(
    {
        "cli/target",
        "docs/dist",
        "docs/node_modules",
        "docs/.vitepress/cache",
    }
)

JSONC_PATTERNS = (
    ".vscode/*.json",
    "symlinks/config/Code/User/*.json",
    "symlinks/AppData/Local/Packages/Microsoft.WindowsTerminal_*/LocalState/settings.json",
)

XML_EXACT_PATHS = frozenset(
    {
        "symlinks/config/fontconfig/fonts.conf",
        "symlinks/config/nuget/nuget.config",
    }
)


def _load_workflow_yaml():
    name = "workflow_yaml"
    if name in sys.modules:
        return sys.modules[name]
    path = Path(__file__).resolve().with_name(f"{name}.py")
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot load sibling module {name} from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


_YAML = _load_workflow_yaml()


def _normalize_rel_path(rel_path: str) -> str:
    normalized = rel_path.replace("\\", "/")
    while normalized.startswith("./"):
        normalized = normalized[2:]
    return normalized


def is_jsonc_path(rel_path: str) -> bool:
    normalized = _normalize_rel_path(rel_path)
    return any(fnmatch.fnmatch(normalized, pattern) for pattern in JSONC_PATTERNS)


def is_xml_path(rel_path: str) -> bool:
    normalized = _normalize_rel_path(rel_path)
    return normalized.lower().endswith(".xml") or normalized in XML_EXACT_PATHS


def strip_jsonc(text: str) -> str:
    """Strip // and /* */ comments and trailing commas outside string literals."""
    out: list[str] = []
    index = 0
    length = len(text)
    in_string = False

    while index < length:
        ch = text[index]
        if in_string:
            out.append(ch)
            if ch == "\\" and index + 1 < length:
                out.append(text[index + 1])
                index += 2
                continue
            if ch == '"':
                in_string = False
            index += 1
            continue

        if ch == '"':
            in_string = True
            out.append(ch)
            index += 1
            continue

        if ch == "/" and index + 1 < length:
            nxt = text[index + 1]
            if nxt == "/":
                index += 2
                while index < length and text[index] not in "\r\n":
                    index += 1
                continue
            if nxt == "*":
                end = text.find("*/", index + 2)
                if end == -1:
                    raise ValueError("unterminated block comment")
                out.extend(
                    comment_char for comment_char in text[index : end + 2] if comment_char in "\r\n"
                )
                index = end + 2
                continue

        out.append(ch)
        index += 1

    # Remove trailing commas before closing '}' or ']' outside string literals.
    cleaned = "".join(out)
    result: list[str] = []
    index = 0
    length = len(cleaned)
    in_string = False
    while index < length:
        ch = cleaned[index]
        if in_string:
            result.append(ch)
            if ch == "\\" and index + 1 < length:
                result.append(cleaned[index + 1])
                index += 2
                continue
            if ch == '"':
                in_string = False
            index += 1
            continue
        if ch == '"':
            in_string = True
            result.append(ch)
            index += 1
            continue
        if ch == ",":
            lookahead = index + 1
            while lookahead < length and cleaned[lookahead] in " \t\r\n":
                lookahead += 1
            if lookahead < length and cleaned[lookahead] in "}]":
                index += 1
                continue
        result.append(ch)
        index += 1
    return "".join(result)


def _reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    mapping: dict[str, object] = {}
    for key, value in pairs:
        if key in mapping:
            raise ValueError(f"duplicate JSON key {key!r}")
        mapping[key] = value
    return mapping


def validate_file(path: Path, rel_path: str) -> str | None:
    """Return an error string if path is invalid, or None on success."""
    try:
        raw = path.read_bytes()
        text = raw.decode("utf-8")
    except (OSError, UnicodeDecodeError) as error:
        return f"{rel_path}: {error}"

    suffix = path.suffix.lower()
    try:
        if suffix == ".toml":
            tomllib.loads(text)
        elif suffix in {".yml", ".yaml"}:
            _YAML.parse_workflow(text, rel_path)
        elif suffix == ".json":
            payload = strip_jsonc(text) if is_jsonc_path(rel_path) else text
            json.loads(payload, object_pairs_hook=_reject_duplicate_keys)
        elif is_xml_path(rel_path):
            ET.fromstring(text)
    except Exception as error:
        return f"{rel_path}: {error}"
    return None


def collect_files(root: Path) -> list[tuple[Path, str]]:
    """Collect structured files under root, excluding generated build/cache directories."""
    files: list[tuple[Path, str]] = []
    for dirpath, dirnames, filenames in os.walk(root):
        current = Path(dirpath)
        rel_dir = current.relative_to(root)
        kept: list[str] = []
        for name in sorted(dirnames):
            if name in SKIP_DIRS:
                continue
            rel_child = (rel_dir / name).as_posix() if rel_dir != Path() else name
            if rel_child in SKIP_REL_DIRS:
                continue
            kept.append(name)
        dirnames[:] = kept
        for filename in sorted(filenames):
            candidate = current / filename
            if candidate.is_symlink() or not candidate.is_file():
                continue
            rel = candidate.relative_to(root).as_posix()
            if candidate.suffix.lower() not in {
                ".json",
                ".toml",
                ".yml",
                ".yaml",
            } and not is_xml_path(rel):
                continue
            files.append((candidate, rel))
    return files


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    targets: list[tuple[Path, str]] = []
    if len(args) == 2 and args[0] == "--root":
        root = Path(args[1]).resolve()
        targets = collect_files(root)
    elif args:
        cwd = Path.cwd().resolve()
        for raw in args:
            path = Path(raw)
            try:
                rel = path.resolve().relative_to(cwd).as_posix()
            except ValueError:
                rel = raw.replace("\\", "/")
                while rel.startswith("./"):
                    rel = rel[2:]
            targets.append((path, rel))
    else:
        print("Usage: check-structured-files.py (--root <dir> | <file>...)", file=sys.stderr)
        return 2

    errors: list[str] = []
    for path, rel in targets:
        error = validate_file(path, rel)
        if error is not None:
            errors.append(error)

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
