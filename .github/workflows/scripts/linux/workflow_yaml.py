"""Stdlib-only, strict YAML subset parser for GitHub Actions workflow contracts.

Parses arbitrary valid block indentation, block/flow sequences, nested mappings,
quoted scalars, and block scalars while strictly rejecting duplicate mapping
keys and malformed indentation.
"""

from __future__ import annotations

import re

_BLOCK_SCALAR_RE = re.compile(r"^([|>])([+-])?([1-9])?(?:[ \t]+#.*)?$")
_DOUBLE_ESCAPES = {
    "\\": "\\",
    '"': '"',
    "/": "/",
    "n": "\n",
    "r": "\r",
    "t": "\t",
    "0": "\0",
    "a": "\a",
    "b": "\b",
    "v": "\v",
    "f": "\f",
    "e": "\x1b",
    " ": " ",
}


def decode_scalar(raw: str) -> str:
    """Decode a single-line YAML plain, single-quoted, or double-quoted scalar."""
    text = raw.strip()
    if not text:
        return ""
    if text[0] == "'":
        out: list[str] = []
        i = 1
        while i < len(text):
            if text[i] == "'":
                if i + 1 < len(text) and text[i + 1] == "'":
                    out.append("'")
                    i += 2
                    continue
                trailing = text[i + 1 :].lstrip(" \t")
                if not trailing or trailing.startswith("#"):
                    return "".join(out)
                break
            out.append(text[i])
            i += 1
        raise ValueError(f"unterminated single-quoted scalar: {text!r}")
    if text[0] == '"':
        out = []
        i = 1
        while i < len(text):
            ch = text[i]
            if ch == "\\" and i + 1 < len(text):
                nxt = text[i + 1]
                if nxt in _DOUBLE_ESCAPES:
                    out.append(_DOUBLE_ESCAPES[nxt])
                    i += 2
                    continue
                hex_lengths = {"x": 2, "u": 4, "U": 8}
                if nxt in hex_lengths and i + 2 + hex_lengths[nxt] <= len(text):
                    digits = text[i + 2 : i + 2 + hex_lengths[nxt]]
                    try:
                        out.append(chr(int(digits, 16)))
                        i += 2 + hex_lengths[nxt]
                        continue
                    except ValueError:
                        pass
                out.append(nxt)
                i += 2
                continue
            if ch == '"':
                trailing = text[i + 1 :].lstrip(" \t")
                if not trailing or trailing.startswith("#"):
                    return "".join(out)
                break
            out.append(ch)
            i += 1
        raise ValueError(f"unterminated double-quoted scalar: {text!r}")
    return re.split(r"[ \t]+#", text, maxsplit=1)[0].strip()


def _split_mapping_entry(text: str) -> tuple[str, str] | None:
    """Split 'key: value' respecting quoted keys, returning None if not a mapping."""
    if not text:
        return None
    if text[0] in ("'", '"'):
        quote = text[0]
        i = 1
        while i < len(text):
            if quote == '"' and text[i] == "\\" and i + 1 < len(text):
                i += 2
                continue
            if text[i] == quote:
                if quote == "'" and i + 1 < len(text) and text[i + 1] == "'":
                    i += 2
                    continue
                rest = text[i + 1 :]
                if rest == ":" or rest.startswith((": ", ":\t")):
                    return decode_scalar(text[: i + 1]), rest[1:].lstrip(" \t")
                return None
            i += 1
        return None
    for i, ch in enumerate(text):
        if ch == ":" and (i + 1 == len(text) or text[i + 1] in (" ", "\t")):
            key = text[:i].strip()
            if not key or key.startswith("#"):
                return None
            return key, text[i + 1 :].lstrip(" \t")
        if ch == "#" and (i == 0 or text[i - 1] in (" ", "\t")):
            return None
    return None


def _split_flow_items(inner: str, sep: str = ",") -> list[str]:
    items: list[str] = []
    buf: list[str] = []
    quote: str | None = None
    depth = 0
    i = 0
    while i < len(inner):
        ch = inner[i]
        if quote is not None:
            buf.append(ch)
            if quote == '"' and ch == "\\" and i + 1 < len(inner):
                buf.append(inner[i + 1])
                i += 2
                continue
            if ch == quote:
                if quote == "'" and i + 1 < len(inner) and inner[i + 1] == "'":
                    buf.append("'")
                    i += 2
                    continue
                quote = None
            i += 1
            continue
        if ch in ("'", '"'):
            quote = ch
            buf.append(ch)
        elif ch in "[{":
            depth += 1
            buf.append(ch)
        elif ch in "]}":
            depth -= 1
            if depth < 0:
                raise ValueError(f"unbalanced flow delimiter in {inner!r}")
            buf.append(ch)
        elif ch == sep and depth == 0:
            items.append("".join(buf).strip())
            buf = []
        else:
            buf.append(ch)
        i += 1
    if quote is not None or depth != 0:
        raise ValueError(f"unterminated flow collection: {inner!r}")
    tail = "".join(buf).strip()
    if tail:
        items.append(tail)
    return items


def _strip_inline_comment(text: str) -> str:
    quote: str | None = None
    i = 0
    while i < len(text):
        ch = text[i]
        if quote is not None:
            if quote == '"' and ch == "\\" and i + 1 < len(text):
                i += 2
                continue
            if ch == quote:
                if quote == "'" and i + 1 < len(text) and text[i + 1] == "'":
                    i += 2
                    continue
                quote = None
            i += 1
            continue
        if ch in ("'", '"'):
            quote = ch
        elif ch == "#" and (i == 0 or text[i - 1] in (" ", "\t")):
            return text[:i].rstrip(" \t")
        i += 1
    return text.rstrip(" \t")


def _parse_inline_value(raw: str, source: str, line_no: int) -> object:
    text = _strip_inline_comment(raw.strip())
    if not text:
        return None
    if text.startswith("[") and text.endswith("]"):
        inner = text[1:-1].strip()
        if not inner:
            return []
        return [_parse_inline_value(item, source, line_no) for item in _split_flow_items(inner)]
    if text.startswith("{") and text.endswith("}"):
        inner = text[1:-1].strip()
        if not inner:
            return {}
        result: dict[str, object] = {}
        for item in _split_flow_items(inner):
            pair = _split_mapping_entry(item)
            if pair is None:
                raise ValueError(f"{source}:{line_no}: invalid flow mapping entry: {item!r}")
            key, val = pair
            if key in result:
                raise ValueError(f"{source}:{line_no}: duplicate key {key!r}")
            result[key] = _parse_inline_value(val, source, line_no)
        return result
    return decode_scalar(text)


class _Parser:
    def __init__(self, text: str, source: str) -> None:
        self.source = source
        self.lines = text.replace("\r\n", "\n").splitlines()
        self.n = len(self.lines)
        self.pos = 0

    def _indent_of(self, line_no_0: int) -> int:
        raw = self.lines[line_no_0]
        stripped = raw.lstrip(" ")
        if stripped.startswith("\t"):
            raise ValueError(
                f"{self.source}:{line_no_0 + 1}: tab indentation is not allowed in YAML"
            )
        return len(raw) - len(stripped)

    def _skip_blank_and_comments(self) -> None:
        while self.pos < self.n:
            stripped = self.lines[self.pos].strip()
            if not stripped or stripped.startswith("#"):
                self.pos += 1
            else:
                break

    def _consume_block_scalar(self, parent_indent: int, header: str) -> str:
        match = _BLOCK_SCALAR_RE.match(header)
        if match is None:
            raise ValueError(f"{self.source}:{self.pos}: invalid block scalar header {header!r}")
        _style, chomp, explicit = match.groups()
        content_indent = parent_indent + int(explicit) if explicit else None
        collected: list[str] = []
        while self.pos < self.n:
            raw = self.lines[self.pos]
            if not raw.strip():
                collected.append("")
                self.pos += 1
                continue
            indent = self._indent_of(self.pos)
            if content_indent is None:
                if indent <= parent_indent:
                    break
                content_indent = indent
            elif indent < content_indent:
                break
            collected.append(raw[content_indent:])
            self.pos += 1
        while collected and collected[-1] == "" and chomp != "+":
            collected.pop()
        result = "\n".join(collected)
        if chomp != "-" and collected:
            result += "\n"
        return result

    def _consume_flow_collection(self, first_text: str, line_no: int) -> str:
        opener = first_text[0]
        closer = "]" if opener == "[" else "}"
        parts = [_strip_inline_comment(first_text)]
        while True:
            joined = " ".join(parts).strip()
            if joined.endswith(closer):
                return joined
            if self.pos >= self.n:
                raise ValueError(f"{self.source}:{line_no}: unterminated flow collection")
            parts.append(_strip_inline_comment(self.lines[self.pos].strip()))
            self.pos += 1

    def _consume_multiline_scalar(self, first_text: str, parent_indent: int, line_no: int) -> str:
        parts = [first_text]
        while self.pos < self.n:
            raw = self.lines[self.pos]
            stripped = raw.strip()
            if not stripped or stripped.startswith("#"):
                break
            indent = self._indent_of(self.pos)
            if indent <= parent_indent:
                break
            parts.append(stripped)
            self.pos += 1
        return decode_scalar(" ".join(parts))

    def _parse_value(self, raw_value: str, parent_indent: int, line_no: int) -> object:
        stripped = raw_value.strip()
        if stripped and not stripped.startswith("#"):
            if _BLOCK_SCALAR_RE.match(stripped):
                return self._consume_block_scalar(parent_indent, stripped)
            if stripped[0] in "[{":
                flow = self._consume_flow_collection(stripped, line_no)
                return _parse_inline_value(flow, self.source, line_no)
            if stripped[0] in ("'", '"'):
                try:
                    return decode_scalar(stripped)
                except ValueError:
                    return self._consume_multiline_scalar(stripped, parent_indent, line_no)
            # Plain scalar may continue on subsequent deeper-indented lines (e.g. long `if:` expressions).
            value = _strip_inline_comment(stripped)
            continuations = [value]
            while self.pos < self.n:
                nxt = self.lines[self.pos]
                nxt_stripped = nxt.strip()
                if not nxt_stripped or nxt_stripped.startswith("#"):
                    break
                if self._indent_of(self.pos) <= parent_indent:
                    break
                continuations.append(_strip_inline_comment(nxt_stripped))
                self.pos += 1
            return " ".join(continuations)

        self._skip_blank_and_comments()
        if self.pos >= self.n:
            return None
        next_indent = self._indent_of(self.pos)
        next_text = self.lines[self.pos][next_indent:]
        if next_indent == parent_indent and (next_text == "-" or next_text.startswith("- ")):
            return self._parse_sequence(next_indent)
        if next_indent <= parent_indent:
            return None
        return self._parse_block(next_indent)

    def _parse_sequence(self, seq_indent: int) -> list[object]:
        items: list[object] = []
        while True:
            self._skip_blank_and_comments()
            if self.pos >= self.n:
                break
            indent = self._indent_of(self.pos)
            if indent < seq_indent:
                break
            if indent > seq_indent:
                raise ValueError(
                    f"{self.source}:{self.pos + 1}: unexpected indentation in sequence: {self.lines[self.pos].strip()!r}"
                )
            text = self.lines[self.pos][indent:]
            if not (text == "-" or text.startswith("- ")):
                break
            line_no = self.pos + 1
            self.pos += 1
            after = text[1:].lstrip(" \t")
            if not after or after.startswith("#"):
                items.append(self._parse_value("", seq_indent, line_no))
                continue
            item_indent = indent + (len(text) - len(after))
            pair = _split_mapping_entry(after)
            if pair is not None:
                key, val = pair
                mapping: dict[str, object] = {}
                mapping[key] = self._parse_value(val, item_indent, line_no)
                self._parse_mapping_entries(mapping, item_indent)
                items.append(mapping)
            else:
                items.append(self._parse_value(after, seq_indent, line_no))
        return items

    def _parse_mapping_entries(self, mapping: dict[str, object], map_indent: int) -> None:
        while True:
            self._skip_blank_and_comments()
            if self.pos >= self.n:
                break
            indent = self._indent_of(self.pos)
            if indent < map_indent:
                break
            if indent > map_indent:
                raise ValueError(
                    f"{self.source}:{self.pos + 1}: unexpected indentation in mapping: {self.lines[self.pos].strip()!r}"
                )
            text = self.lines[self.pos][indent:]
            if text == "-" or text.startswith("- "):
                break
            line_no = self.pos + 1
            pair = _split_mapping_entry(text)
            if pair is None:
                raise ValueError(f"{self.source}:{line_no}: invalid mapping entry: {text!r}")
            key, val = pair
            if key in mapping:
                raise ValueError(f"{self.source}:{line_no}: duplicate key {key!r}")
            self.pos += 1
            mapping[key] = self._parse_value(val, map_indent, line_no)

    def _parse_block(self, block_indent: int) -> object:
        text = self.lines[self.pos][block_indent:]
        if text == "-" or text.startswith("- "):
            return self._parse_sequence(block_indent)
        mapping: dict[str, object] = {}
        self._parse_mapping_entries(mapping, block_indent)
        return mapping

    def parse(self) -> dict[str, object]:
        self._skip_blank_and_comments()
        if self.pos >= self.n:
            return {}
        root_indent = self._indent_of(self.pos)
        parsed = self._parse_block(root_indent)
        self._skip_blank_and_comments()
        if self.pos < self.n:
            raise ValueError(
                f"{self.source}:{self.pos + 1}: unexpected trailing content: {self.lines[self.pos].strip()!r}"
            )
        if not isinstance(parsed, dict):
            raise ValueError(f"{self.source}: workflow root must be a YAML mapping")
        return parsed


def parse_workflow(text: str, source: str = "workflow") -> dict[str, object]:
    """Parse a GitHub Actions workflow YAML document into Python mappings/lists/strings."""
    return _Parser(text, source).parse()


def normalize_needs(value: object) -> set[str]:
    """Normalize a GitHub Actions job `needs` field (scalar or sequence) into a set."""
    if value is None:
        return set()
    if isinstance(value, str):
        return {value} if value else set()
    if isinstance(value, list):
        return {str(item) for item in value}
    raise ValueError(f"unsupported needs value: {value!r}")


def job_blocks(workflow: str) -> dict[str, str]:
    """Return raw text slices per job while validating workflow YAML structure."""
    parsed = parse_workflow(workflow)
    jobs = parsed.get("jobs")
    if not isinstance(jobs, dict) or not jobs:
        raise ValueError("CI workflow has no jobs")
    lines = workflow.replace("\r\n", "\n").splitlines(keepends=True)
    jobs_index = next(
        (i for i, line in enumerate(lines) if re.match(r"^jobs:[ \t]*(?:#.*)?\n?$", line)),
        None,
    )
    if jobs_index is None:
        raise ValueError("CI workflow has no jobs section")
    # Detect actual indentation of the first job key rather than assuming 2 spaces.
    job_indent: int | None = None
    spans: list[tuple[str, int]] = []
    for i in range(jobs_index + 1, len(lines)):
        line = lines[i]
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        if indent == 0:
            break
        if job_indent is None:
            job_indent = indent
        if indent == job_indent:
            pair = _split_mapping_entry(line[indent:].rstrip("\n"))
            if pair is not None:
                spans.append((pair[0], i))
    if not spans:
        raise ValueError("CI workflow has no jobs")
    blocks: dict[str, str] = {}
    for idx, (name, start) in enumerate(spans):
        end = spans[idx + 1][1] if idx + 1 < len(spans) else len(lines)
        blocks[name] = "".join(lines[start + 1 : end])
    return blocks
