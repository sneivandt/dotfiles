#!/usr/bin/env python3
"""Check CI scheduling and reject accidental skips in the aggregate gate."""

import json
import os
from pathlib import Path
import re
import runpy
import sys


WORKFLOW = Path(__file__).resolve().parents[2] / "ci.yml"
CLI_ROOT = Path(__file__).resolve().parents[4] / "cli"
RUN_MUTATIONS = Path(__file__).resolve().with_name("run-mutations.py")
INFORMATIONAL_JOBS = {"coverage"}
JOB_CONDITIONS = {
    "rust-fmt": "run_rust_fmt",
    "lint": "run_lint",
    "managed-script-regressions": "run_managed_scripts",
    "docs": "run_docs_site",
    "docs-links": "run_docs_checks",
    "docs-contracts": "run_docs_contracts",
    "desktop-qml": "run_desktop_qml",
    "validate-config": "run_validate_config",
    "audit": "run_audit",
    "deny": "run_deny",
    "build-linux": "run_build_linux",
    "msrv": "run_rust_checks",
    "coverage": "run_rust_checks",
    "build-windows": "run_build_windows",
    "integration-linux": "run_profile_linux",
    "integration-windows": "run_profile_windows",
    "test-install-uninstall": "run_roundtrip_linux",
    "test-install-uninstall-windows": "run_roundtrip_windows",
    "test-applications": "run_app_tests",
    "test-applications-windows": "run_app_windows",
    "test-git-hooks": "run_git_hooks",
    "test-shell-wrapper-linux": "run_wrapper_linux",
    "test-shell-wrapper-windows": "run_wrapper_windows",
    "test-stocks": "run_stocks",
    "test-hook-inputs": "run_hook_inputs",
}
SPECIAL_CONDITIONS = {
    "validate-config": "needs.classify-changes.outputs.run_validate_config == 'true' && needs.build-linux.result == 'success'",
}


def job_blocks(workflow: str) -> dict[str, str]:
    jobs = workflow.split("\njobs:\n", 1)
    if len(jobs) != 2:
        raise ValueError("CI workflow has no jobs section")
    headings = list(re.finditer(r"(?m)^  ([A-Za-z_][A-Za-z0-9_-]*):[ \t]*$", jobs[1]))
    if not headings:
        raise ValueError("CI workflow has no jobs")
    return {
        heading.group(1): jobs[1][heading.end() : headings[index + 1].start() if index + 1 < len(headings) else None]
        for index, heading in enumerate(headings)
    }


def gate_dependencies(block: str) -> set[str]:
    match = re.search(
        r"(?m)^    needs:[ \t]*$\n((?:^      - [A-Za-z_][A-Za-z0-9_-]*[ \t]*$\n?)+)",
        block,
    )
    if match is None:
        raise ValueError("ci-success must have a multiline needs list")
    return set(re.findall(r"(?m)^      - ([A-Za-z_][A-Za-z0-9_-]*)[ \t]*$", match.group(1)))


def job_condition(block: str) -> str | None:
    match = re.search(r"(?m)^    if: (.+)$", block)
    return match.group(1).strip() if match else None


def job_needs(block: str) -> set[str]:
    match = re.search(r"(?m)^    needs: \[([^]]+)\]$", block)
    return {name.strip() for name in match.group(1).split(",")} if match else set()


_BLOCK_SCALAR_RE = re.compile(r"^[|>][+-]?\d*(?:\s+#.*)?$")
_KEY_LINE_RE = re.compile(r"^([A-Za-z0-9_-]+):(?:\s*(.*))?$")
_YAML_DOUBLE_ESCAPES = {
    "\\": "\\", '"': '"', "/": "/", "n": "\n", "r": "\r", "t": "\t",
    "0": "\0", "a": "\a", "b": "\b", "v": "\v", "f": "\f", "e": "\x1b", " ": " ",
}


def _decode_yaml_scalar(raw: str) -> str:
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
        return text
    if text[0] == '"':
        out = []
        i = 1
        while i < len(text):
            ch = text[i]
            if ch == "\\" and i + 1 < len(text):
                nxt = text[i + 1]
                if nxt in _YAML_DOUBLE_ESCAPES:
                    out.append(_YAML_DOUBLE_ESCAPES[nxt])
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
        return text
    return re.split(r"[ \t]+#", text, maxsplit=1)[0].strip()


def check_checkout_steps(workflow: str, workflow_name: str = "workflow") -> None:
    """Validate step/with indentation, duplicate keys, and checkout persist-credentials."""
    for job_name, block in job_blocks(workflow).items():
        lines = block.splitlines()
        index = 0
        while index < len(lines) and not re.match(r"^    steps:[ \t]*(?:#.*)?$", lines[index]):
            index += 1
        if index >= len(lines):
            continue
        index += 1

        step_lines: list[tuple[int, str]] = []
        block_scalar_indent: int | None = None
        while index < len(lines):
            raw = lines[index]
            stripped = raw.strip()
            indent = len(raw) - len(raw.lstrip(" "))
            if block_scalar_indent is not None:
                if stripped and indent <= block_scalar_indent:
                    block_scalar_indent = None
                else:
                    step_lines.append((index, raw))
                    index += 1
                    continue
            if not stripped or stripped.startswith("#"):
                step_lines.append((index, raw))
                index += 1
                continue
            if indent <= 4:
                break
            if indent == 6 and not raw.startswith("      - "):
                raise ValueError(
                    f"{workflow_name} job {job_name!r} has malformed step indentation: {raw.strip()!r}"
                )
            key_text = raw[8:] if raw.startswith("      - ") else (raw[8:] if indent == 8 else "")
            if key_text:
                match = _KEY_LINE_RE.match(key_text)
                if match and _BLOCK_SCALAR_RE.match(match.group(2) or ""):
                    block_scalar_indent = 8
            step_lines.append((index, raw))
            index += 1

        steps: list[list[str]] = []
        for _, raw in step_lines:
            if raw.startswith("      - "):
                steps.append([raw])
            elif steps:
                steps[-1].append(raw)

        for step_index, step in enumerate(steps, start=1):
            seen_keys: set[str] = set()
            uses_value: str | None = None
            in_with = False
            with_keys: set[str] = set()
            persist_credentials: str | None = None
            scalar_indent: int | None = None

            for raw in step:
                stripped = raw.strip()
                indent = len(raw) - len(raw.lstrip(" "))
                if scalar_indent is not None:
                    if stripped and indent <= scalar_indent:
                        scalar_indent = None
                    else:
                        continue
                if not stripped or stripped.startswith("#"):
                    continue

                if raw.startswith("      - ") or indent == 8:
                    in_with = False
                    entry = raw[8:]
                    match = _KEY_LINE_RE.match(entry)
                    if match is None:
                        raise ValueError(
                            f"{workflow_name} job {job_name!r} step {step_index} has invalid step key: {entry!r}"
                        )
                    key, value = match.group(1), _decode_yaml_scalar(match.group(2) or "")
                    if key in seen_keys:
                        raise ValueError(
                            f"{workflow_name} job {job_name!r} step {step_index} has duplicate key {key!r}"
                        )
                    seen_keys.add(key)
                    if key == "uses":
                        uses_value = value
                    elif key == "with":
                        in_with = True
                    if _BLOCK_SCALAR_RE.match((match.group(2) or "").strip()):
                        scalar_indent = 8
                elif in_with and indent == 10:
                    entry = raw[10:]
                    match = _KEY_LINE_RE.match(entry)
                    if match is None:
                        raise ValueError(
                            f"{workflow_name} job {job_name!r} step {step_index} has invalid with entry: {entry!r}"
                        )
                    key, value = match.group(1), _decode_yaml_scalar(match.group(2) or "")
                    if key in with_keys:
                        raise ValueError(
                            f"{workflow_name} job {job_name!r} step {step_index} has duplicate with key {key!r}"
                        )
                    with_keys.add(key)
                    if key == "with":
                        raise ValueError(
                            f"{workflow_name} job {job_name!r} step {step_index} has nested 'with:' inside 'with:'"
                        )
                    if key == "persist-credentials":
                        persist_credentials = value
                    if _BLOCK_SCALAR_RE.match((match.group(2) or "").strip()):
                        scalar_indent = 10
                elif in_with and indent < 10:
                    raise ValueError(
                        f"{workflow_name} job {job_name!r} step {step_index} has malformed with indentation: {stripped!r}"
                    )

            if uses_value and uses_value.startswith("actions/checkout@"):
                if "with" not in seen_keys or persist_credentials != "false":
                    raise ValueError(
                        f"{workflow_name} job {job_name!r} step {step_index} checkout must set persist-credentials: false in a properly nested with block"
                    )


def check(workflow: str) -> None:
    check_checkout_steps(workflow, "ci.yml")
    jobs = job_blocks(workflow)
    gate = jobs.get("ci-success")
    if gate is None:
        raise ValueError("missing ci-success job")
    if not re.search(r"(?m)^    if: always\(\)\s*$", gate):
        raise ValueError("ci-success must run under if: always()")
    for job in INFORMATIONAL_JOBS:
        if job not in jobs or not re.search(r"(?m)^    continue-on-error: true\s*$", jobs[job]):
            raise ValueError(f"informational job {job} is missing or no longer non-gating")
    required = jobs.keys() - INFORMATIONAL_JOBS - {"ci-success"}
    actual = gate_dependencies(gate)
    missing = required - actual
    extra = actual - required
    if missing or extra:
        raise ValueError(
            f"ci-success needs mismatch: missing={sorted(missing)}, unexpected={sorted(extra)}"
        )
    expected_conditions = {
        name: f"needs.classify-changes.outputs.{output} == 'true'"
        for name, output in JOB_CONDITIONS.items()
    } | SPECIAL_CONDITIONS
    condition_jobs = jobs.keys() - {"classify-changes", "ci-success"}
    if condition_jobs != expected_conditions.keys():
        raise ValueError(
            "CI condition contract mismatch: "
            f"missing={sorted(condition_jobs - expected_conditions.keys())}, "
            f"unexpected={sorted(expected_conditions.keys() - condition_jobs)}"
        )
    for name, expected in expected_conditions.items():
        actual = job_condition(jobs[name])
        if actual != expected:
            raise ValueError(f"{name} condition changed: expected {expected!r}, got {actual!r}")
        if "classify-changes" not in job_needs(jobs[name]):
            raise ValueError(f"{name} must depend on classify-changes")
        output = JOB_CONDITIONS[name]
        if f"{output}: ${{{{ steps.classify.outputs.{output} }}}}" not in jobs["classify-changes"]:
            raise ValueError(f"classify-changes must expose {output}")

    for name, platform in {
        "validate-config": "linux",
        "integration-linux": "linux",
        "integration-windows": "windows",
        "test-install-uninstall": "linux",
        "test-install-uninstall-windows": "windows",
        "test-applications": "linux",
        "test-applications-windows": "windows",
        "test-shell-wrapper-linux": "linux",
        "test-shell-wrapper-windows": "windows",
    }.items():
        if f"build-{platform}" not in job_needs(jobs[name]):
            raise ValueError(f"{name} must depend on its {platform} artifact")
    for name, output in {"lint": "lint_matrix", "test-applications": "app_matrix"}.items():
        if f"matrix: ${{{{ fromJSON(needs.classify-changes.outputs.{output}) }}}}" not in jobs[name]:
            raise ValueError(f"{name} must use the classified matrix")
    if "check-ci-contract.py --results" not in gate or "CI_NEEDS: ${{ toJSON(needs) }}" not in gate:
        raise ValueError("ci-success must verify selected jobs, not just absence of failures")

    for job, command in {
        "docs-links": "test-docs.sh docs_links",
        "docs-contracts": "test-docs.sh docs_task_selectors",
    }.items():
        if command not in jobs[job]:
            raise ValueError(f"{job} must execute its documentation checks")
    for required in (
        "deployments: read", "docs-publication-base.py",
        "DOCS_BASE_SHA: ${{ steps.published-docs.outputs.base_sha }}",
    ):
        if required not in jobs["classify-changes"]:
            raise ValueError(f"documentation selection is missing {required}")


def check_mutation(workflow: str) -> None:
    check_checkout_steps(workflow, "mutation.yml")
    jobs = job_blocks(workflow)
    if jobs.keys() != {"select", "mutation", "mutation-windows", "mutation-results"}:
        raise ValueError("unexpected mutation workflow jobs")
    if re.search(r"(?m)^  (push|pull_request|workflow_run):", workflow):
        raise ValueError("mutation testing must not run on normal CI events")
    for required in ("  schedule:", "  workflow_dispatch:", "select-mutations.py",
                     "actions: read", "contents: read"):
        if required not in workflow:
            raise ValueError(f"mutation scheduling is missing {required}")
    for name, flag in (("mutation", "run_linux"), ("mutation-windows", "run_windows")):
        if job_condition(jobs[name]) != f"needs.select.outputs.{flag} == 'true'":
            raise ValueError(f"mutation selection condition changed for {name}")
        if job_needs(jobs[name]) != {"select"}:
            raise ValueError(f"{name} must depend on mutation selection")
    if job_condition(jobs["mutation-results"]) != "always()" or job_needs(jobs["mutation-results"]) != {
        "select", "mutation", "mutation-windows",
    }:
        raise ValueError("mutation results must report every selected job")
    if "continue-on-error:" in workflow:
        raise ValueError("mutation workflow must propagate failures")
    results = jobs["mutation-results"]
    for required in ("pattern: mutation-result-*", "report-mutations.py",
                     "mutation-inputs/mutation-plan.json"):
        if required not in results:
            raise ValueError(f"mutation results must consume shard reports: {required}")
    mutation = jobs["mutation"]
    for required in (
        "matrix: ${{ fromJSON(needs.select.outputs.linux_matrix) }}",
        "name: mutation-inputs", "run-mutations.py", 'run "$MUTATION_SUITE" --partition',
        "fail-fast: false",
    ):
        if required not in mutation:
            raise ValueError(f"Linux mutation coverage missing {required}")
    for required in ("run-mutations.py plan", "cli/mutation-plan.json", "cli/mutants.diff"):
        if required not in jobs["select"]:
            raise ValueError(f"mutation selection missing {required}")
    parallel = re.search(r"(?m)^      max-parallel: ([0-9]+)$", mutation)
    if parallel is None or not 1 <= int(parallel.group(1)) <= 14:
        raise ValueError("Linux mutation concurrency must leave runner capacity for ordinary CI")
    native = jobs["mutation-windows"]
    for required in ("runs-on: windows-latest", "shard: [0, 1, 2, 3, 4, 5, 6, 7]",
                     "run-mutations.py", 'run windows --partition "$env:MUTATION_SHARD/8"'):
        if required not in native:
            raise ValueError(f"native Windows mutation coverage missing {required}")


def check_release(workflow: str) -> None:
    check_checkout_steps(workflow, "release.yml")
    jobs = job_blocks(workflow)
    if job_condition(jobs["version"]) != "needs.check-ci.outputs.run_release == 'true'":
        raise ValueError("release version resolution must require changed binary inputs")
    classify = jobs["check-ci"]
    for guard in (
        "github.event.workflow_run.conclusion == 'success'",
        "github.event.workflow_run.event == 'push'",
        "github.event.workflow_run.head_branch == 'main'",
        "github.event.workflow_run.head_repository.full_name == github.repository",
        "ref: ${{ steps.resolve.outputs.sha }}",
        "run_release: ${{ steps.classify.outputs.run_release }}",
        "classify-release.sh",
    ):
        if guard not in classify:
            raise ValueError(f"release classification is missing {guard}")
    for name in ("build-linux", "build-windows", "release"):
        if "version" not in job_needs(jobs[name]):
            raise ValueError(f"release {name} must depend on the selected version")
    for name, command in {
        "version": 'release-tag.sh next "$date_part"',
        "release": 'release-tag.sh check "$VERSION"',
    }.items():
        if command not in jobs[name]:
            raise ValueError(f"release {name} must use the checked remote tag helper")

def check_pages(workflow: str, ci_workflow: str) -> None:
    jobs = job_blocks(workflow)
    select, package, deploy = jobs["select-build"], jobs["package"], jobs["deploy"]
    for guard in (
        "github.event.workflow_run.conclusion == 'success'",
        "github.event.workflow_run.event == 'push'",
        "github.event.workflow_run.head_branch == 'main'",
        "github.event.workflow_run.head_repository.full_name == github.repository",
        "current: ${{ steps.current.outputs.current }}",
        'BUILD_SHA: ${{ github.event.workflow_run.head_sha }}',
        'if [ "$current_sha" = "$BUILD_SHA" ] && [ "$GITHUB_SHA" = "$BUILD_SHA" ]; then',
        'CI_RUN_ID: ${{ github.event.workflow_run.id }}',
        'case "$site_result" in',
        'select(.name == "Docs: Content and site") | .conclusion',
        'skipped)',
        'success) ;;',
        '*) echo "Cannot verify the CI documentation site job: $site_result" >&2; exit 1 ;;',
    ):
        if guard not in select:
            raise ValueError(f"Pages build selection is missing {guard}")
    if "environment:" in select:
        raise ValueError("Pages build selection must run before entering the deployment environment")
    for guard in (
        "needs: select-build",
        "if: needs.select-build.outputs.current == 'true'",
        "run-id: ${{ github.event.workflow_run.id }}",
        "name: docs-site",
        "PAGES_ARTIFACT: github-pages-${{ github.run_attempt }}",
        "artifact_name: ${{ env.PAGES_ARTIFACT }}",
        " name: ${{ env.PAGES_ARTIFACT }}",
    ):
        if guard not in package:
            raise ValueError(f"Pages packaging is missing {guard}")
    for guard in (
        "needs: package",
        "artifact_name: ${{ needs.package.outputs.artifact_name }}",
        "url: ${{ steps.deployment.outputs.page_url }}",
    ):
        if guard not in deploy:
            raise ValueError(f"Pages deployment is missing {guard}")
    if "actions/upload-pages-artifact@" in deploy:
        raise ValueError("Retrying Pages deployment must not upload another artifact")
    if "actions/checkout@" in package or "actions/checkout@" in deploy:
        raise ValueError("Pages deployment must use the verified artifact, not execute a checkout")
    docs = job_blocks(ci_workflow)["docs"]
    for required in (
        "test-docs.sh docs_site",
        "npx playwright install --with-deps --only-shell chromium",
        "name: docs-site",
        "path: docs/dist/",
        "if: github.event_name == 'push' && github.ref == 'refs/heads/main'",
    ):
        if required not in docs:
            raise ValueError(f"CI documentation artifact is missing {required}")


def check_mutation_symbols(cli_root: Path = CLI_ROOT, mutation_run: dict | None = None) -> None:
    if mutation_run is None:
        mutation_run = runpy.run_path(str(RUN_MUTATIONS))

    def read_source(rel_path: str) -> str:
        if not rel_path.startswith("src/"):
            raise ValueError(f"mutation path must stay under src/: {rel_path}")
        target = cli_root / rel_path
        if not target.is_file():
            raise ValueError(f"mutation reference points to missing file: {rel_path}")
        return target.read_text(encoding="utf-8")

    def require_fn(rel_path: str, source: str, symbol: str) -> None:
        if not re.search(rf"\bfn\s+{re.escape(symbol)}\b", source):
            raise ValueError(f"mutation reference {rel_path} is missing fn {symbol}")

    native_modules = mutation_run["NATIVE_MODULES"]
    for rel_path in native_modules:
        read_source(rel_path)

    windows_sources = {rel_path: read_source(rel_path) for rel_path in mutation_run["WINDOWS_FILES"]}

    for option, value in mutation_run["LINUX_EXCLUSIONS"]:
        if option == "--exclude":
            read_source(value)
        elif option == "--exclude-re":
            match = re.match(r"^\^(src/[^:]+?):(.*)$", value)
            if match is None:
                raise ValueError(f"unrecognized LINUX_EXCLUSIONS file prefix: {value}")
            rel_path = match.group(1).replace(r"\.", ".")
            remainder = match.group(2)
            source = read_source(rel_path)
            if group := re.fullmatch(r"\.\*\\b\(([^)]+)\)\\b", remainder):
                for symbol in group.group(1).split("|"):
                    require_fn(rel_path, source, symbol)
            elif single := re.fullmatch(r"\.\*\\b([A-Za-z_][A-Za-z0-9_]*)\\b", remainder):
                require_fn(rel_path, source, single.group(1))
            elif module := re.fullmatch(r"\.\*\\b([A-Za-z_][A-Za-z0-9_]*)::", remainder):
                if not re.search(rf"\bmod\s+{re.escape(module.group(1))}\b", source):
                    raise ValueError(f"mutation reference {rel_path} is missing mod {module.group(1)}")
            elif impl := re.search(
                r"\bin <impl ([A-Za-z0-9_:]+) for ([A-Za-z0-9_]+)(?:<[^>]*>)?>::([A-Za-z_][A-Za-z0-9_]*)\$$",
                remainder,
            ):
                trait, target_type, method = impl.groups()
                if not re.search(rf"\bimpl\b[^{{;]*\b{re.escape(trait)}\b\s+for\s+{re.escape(target_type)}\b", source):
                    raise ValueError(f"mutation reference {rel_path} is missing impl {trait} for {target_type}")
                require_fn(rel_path, source, method)
            elif target_fn := re.search(r"\bin ([A-Za-z_][A-Za-z0-9_]*)\$$", remainder):
                require_fn(rel_path, source, target_fn.group(1))
            else:
                raise ValueError(f"unrecognized LINUX_EXCLUSIONS symbol pattern: {value}")
        else:
            raise ValueError(f"unexpected LINUX_EXCLUSIONS option: {option}")

    native_suffix = "|^(?:" + "|".join(re.escape(path) for path in native_modules) + "):"
    windows_pattern = mutation_run["WINDOWS_PATTERN"]
    if not windows_pattern.endswith(native_suffix):
        raise ValueError("WINDOWS_PATTERN must include all NATIVE_MODULES")
    symbol_group = windows_pattern[: -len(native_suffix)]
    if not (symbol_group.startswith("(") and symbol_group.endswith(")")):
        raise ValueError(f"unrecognized WINDOWS_PATTERN symbol list: {symbol_group}")
    for symbol in symbol_group[1:-1].split("|"):
        if "::" in symbol:
            module, fn_name = symbol.split("::", 1)
            found = any(
                re.search(rf"\bmod\s+{re.escape(module)}\b", src)
                and re.search(rf"\bfn\s+{re.escape(fn_name)}\b", src)
                for src in windows_sources.values()
            )
        else:
            found = any(re.search(rf"\bfn\s+{re.escape(symbol)}\b", src) for src in windows_sources.values())
        if not found:
            raise ValueError(f"WINDOWS_PATTERN symbol not found in WINDOWS_FILES: {symbol}")

    def expand_path_pattern(expr: str) -> set[str]:
        unescaped = expr.replace(r"\.", ".")

        def parse(pos: int, nested: bool = False) -> tuple[list[str], int]:
            alts: list[str] = []
            cur = [""]
            while pos < len(unescaped):
                ch = unescaped[pos]
                if ch == "(":
                    sub, pos = parse(pos + 1, nested=True)
                    cur = [prefix + part for prefix in cur for part in sub]
                elif ch == "|":
                    alts.extend(cur)
                    cur = [""]
                    pos += 1
                elif ch == ")":
                    if not nested:
                        raise ValueError(f"unbalanced FOCUSED file prefix: {expr}")
                    alts.extend(cur)
                    return alts, pos + 1
                else:
                    cur = [prefix + ch for prefix in cur]
                    pos += 1
            if nested:
                raise ValueError(f"unclosed FOCUSED file prefix: {expr}")
            alts.extend(cur)
            return alts, pos

        paths, end = parse(0)
        if end != len(unescaped) or not paths or any(
            re.search(r"[\\*+?\[\]{}^$]", path) for path in paths
        ):
            raise ValueError(f"unrecognized FOCUSED file prefix: {expr}")
        return set(paths)

    def split_top_level_alts(expr: str) -> list[str]:
        parts: list[str] = []
        buf: list[str] = []
        depth = 0
        index = 0
        while index < len(expr):
            ch = expr[index]
            if ch == "\\" and index + 1 < len(expr):
                buf.append(ch)
                buf.append(expr[index + 1])
                index += 2
                continue
            if ch == "(":
                depth += 1
            elif ch == ")":
                depth -= 1
                if depth < 0:
                    raise ValueError(f"unbalanced FOCUSED symbol group: {expr}")
            elif ch == "|" and depth == 0:
                parts.append("".join(buf))
                buf = []
                index += 1
                continue
            buf.append(ch)
            index += 1
        if depth != 0:
            raise ValueError(f"unclosed FOCUSED symbol group: {expr}")
        parts.append("".join(buf))
        return parts

    for suite, (files, pattern, _test) in mutation_run["FOCUSED"].items():
        if not files:
            raise ValueError(f"FOCUSED suite {suite!r} has no files")
        focused_sources = {rel_path: read_source(rel_path) for rel_path in files}
        match = re.match(r"^\^(src/[^:]+):\[0-9\]\+:\[0-9\]\+: (.+)\$$", pattern)
        if match is None:
            raise ValueError(f"unrecognized FOCUSED pattern for {suite!r}: {pattern}")
        pattern_files = expand_path_pattern(match.group(1))
        missing_files = pattern_files - focused_sources.keys()
        if missing_files:
            raise ValueError(
                f"FOCUSED suite {suite!r} omits files matched by its regex: {sorted(missing_files)}"
            )
        body = match.group(2)
        if body.startswith("replace (") and body.endswith(")"):
            clauses = split_top_level_alts(body[len("replace (") : -1])
        elif body.startswith("(") and body.endswith(")"):
            clauses = split_top_level_alts(body[1:-1])
        else:
            clauses = [body]
        for clause in clauses:
            in_fn = re.search(
                r"\bin (?:([A-Za-z_][A-Za-z0-9_]*)::)?([A-Za-z_][A-Za-z0-9_]*)$",
                clause,
            )
            replace_fn = re.match(
                r"^(?:replace )?(?:([A-Za-z_][A-Za-z0-9_]*)::)?([A-Za-z_][A-Za-z0-9_]*)(?: -> | with )",
                clause,
            )
            symbol_match = in_fn or replace_fn
            if symbol_match is None:
                raise ValueError(f"unrecognized FOCUSED symbol clause for {suite!r}: {clause}")
            type_name, fn_name = symbol_match.groups()
            found = any(
                re.search(rf"\bfn\s+{re.escape(fn_name)}\b", src)
                and (
                    type_name is None
                    or re.search(rf"\bimpl\b[^{{;]*\b{re.escape(type_name)}\b", src)
                )
                for src in focused_sources.values()
            )
            if not found:
                qualified = f"{type_name}::{fn_name}" if type_name else fn_name
                raise ValueError(
                    f"FOCUSED suite {suite!r} symbol not found in its files: {qualified}"
                )


def check_results(needs: dict) -> None:
    expected_jobs = JOB_CONDITIONS.keys() - INFORMATIONAL_JOBS
    if needs.keys() != expected_jobs | {"classify-changes"}:
        raise ValueError("CI results do not contain exactly the gating jobs")
    classification = needs["classify-changes"]
    if classification["result"] != "success":
        raise ValueError("change classification did not succeed")
    outputs = classification["outputs"]
    for name in sorted(expected_jobs):
        output = JOB_CONDITIONS[name]
        selected = outputs.get(output)
        if selected not in {"true", "false"}:
            raise ValueError(f"missing or invalid classification output: {output}")
        expected = "success" if selected == "true" else "skipped"
        actual = needs[name]["result"]
        if actual != expected:
            raise ValueError(f"{name}: expected {expected}, got {actual}")


if __name__ == "__main__":
    try:
        for workflow_path in sorted(WORKFLOW.parent.glob("*.yml")):
            check_checkout_steps(workflow_path.read_text(encoding="utf-8"), workflow_path.name)
        check(WORKFLOW.read_text(encoding="utf-8"))
        check_mutation(WORKFLOW.with_name("mutation.yml").read_text(encoding="utf-8"))
        check_mutation_symbols()
        check_release(WORKFLOW.with_name("release.yml").read_text(encoding="utf-8"))
        check_pages(WORKFLOW.with_name("pages.yml").read_text(encoding="utf-8"),
                    WORKFLOW.read_text(encoding="utf-8"))
        if sys.argv[1:] == ["--results"]:
            check_results(json.loads(os.environ["CI_NEEDS"]))
    except ValueError as error:
        print(f"CI contract failed: {error}", file=sys.stderr)
        sys.exit(1)
    print("CI job gate and scheduling conditions match the contract")
