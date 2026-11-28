#!/usr/bin/env python3
"""Check CI scheduling and reject accidental skips in the aggregate gate."""

import importlib.util
import json
import os
from pathlib import Path
import re
import runpy
import sys


def _load_sibling(name: str):
    if name in sys.modules:
        return sys.modules[name]
    path = Path(__file__).resolve().with_name(f"{name}.py")
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot load sibling module {name} from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


_META = _load_sibling("ci_metadata")
_YAML = _load_sibling("workflow_yaml")

WORKFLOW = Path(__file__).resolve().parents[1] / "ci.yml"
CLI_ROOT = Path(__file__).resolve().parents[3] / "cli"
RUN_MUTATIONS = Path(__file__).resolve().with_name("run-mutations.py")
INFORMATIONAL_JOBS = set(_META.INFORMATIONAL_JOBS)
JOB_CONDITIONS = _META.JOB_CONDITIONS
SPECIAL_CONDITIONS = _META.SPECIAL_CONDITIONS
ARTIFACT_DEPENDENCIES = _META.ARTIFACT_DEPENDENCIES
MATRIX_JOBS = _META.MATRIX_JOBS

parse_workflow = _YAML.parse_workflow
normalize_needs = _YAML.normalize_needs
job_blocks = _YAML.job_blocks


def _as_job(block: str | dict[str, object]) -> dict[str, object]:
    if isinstance(block, dict):
        return block
    parsed = parse_workflow(block, "job")
    if not isinstance(parsed, dict):
        raise ValueError("job block must be a YAML mapping")
    return parsed


def gate_dependencies(block: str | dict[str, object]) -> set[str]:
    needs = _as_job(block).get("needs")
    if not isinstance(needs, list) or not needs:
        raise ValueError("ci-success must have a needs list")
    return {str(name) for name in needs}


def job_condition(block: str | dict[str, object]) -> str | None:
    value = _as_job(block).get("if")
    return str(value) if value is not None else None


def job_needs(block: str | dict[str, object]) -> set[str]:
    return normalize_needs(_as_job(block).get("needs"))


def _job_steps(
    job: dict[str, object], workflow_name: str, job_name: str
) -> list[dict[str, object]]:
    steps = job.get("steps")
    if steps is None:
        return []
    if not isinstance(steps, list):
        raise ValueError(f"{workflow_name} job {job_name!r} steps must be a YAML sequence")
    result: list[dict[str, object]] = []
    for index, step in enumerate(steps, start=1):
        if not isinstance(step, dict):
            raise ValueError(
                f"{workflow_name} job {job_name!r} step {index} must be a YAML mapping"
            )
        result.append(step)
    return result


def _job_run_text(job: dict[str, object]) -> str:
    steps = job.get("steps")
    if not isinstance(steps, list):
        return ""
    return "\n".join(str(step.get("run", "")) for step in steps if isinstance(step, dict))


def check_checkout_steps(workflow: str, workflow_name: str = "workflow") -> None:
    """Validate YAML structure, duplicate keys, and checkout persist-credentials."""
    parsed = parse_workflow(workflow, workflow_name)
    jobs = parsed.get("jobs")
    if not isinstance(jobs, dict) or not jobs:
        raise ValueError(f"{workflow_name} has no jobs")
    for job_name, job in jobs.items():
        if not isinstance(job, dict):
            raise ValueError(f"{workflow_name} job {job_name!r} must be a YAML mapping")
        for step_index, step in enumerate(_job_steps(job, workflow_name, job_name), start=1):
            with_block = step.get("with")
            if isinstance(with_block, dict) and "with" in with_block:
                raise ValueError(
                    f"{workflow_name} job {job_name!r} step {step_index} has nested 'with:' inside 'with:'"
                )
            uses = step.get("uses")
            if (
                isinstance(uses, str)
                and uses.startswith("actions/checkout@")
                and (
                    not isinstance(with_block, dict)
                    or with_block.get("persist-credentials") != "false"
                )
            ):
                raise ValueError(
                    f"{workflow_name} job {job_name!r} step {step_index} checkout must set "
                    "persist-credentials: false in a properly nested with block"
                )


def check(workflow: str) -> None:
    check_checkout_steps(workflow, "ci.yml")
    arch_digests = set(re.findall(r"\barchlinux@sha256:[0-9a-f]{64}\b", workflow))
    if len(arch_digests) != 1:
        raise ValueError(
            f"ci.yml must pin a single archlinux@sha256 digest across jobs, found: {sorted(arch_digests)}"
        )
    parsed = parse_workflow(workflow, "ci.yml")
    jobs = parsed["jobs"]
    assert isinstance(jobs, dict)
    gate = jobs.get("ci-success")
    if not isinstance(gate, dict):
        raise ValueError("missing ci-success job")
    if job_condition(gate) != "always()":
        raise ValueError("ci-success must run under if: always()")
    for job in INFORMATIONAL_JOBS:
        if job not in jobs or jobs[job].get("continue-on-error") != "true":
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
    classify_job = jobs["classify-changes"]
    classify_outputs = classify_job.get("outputs") or {}
    for name, expected in expected_conditions.items():
        actual_condition = job_condition(jobs[name])
        if actual_condition != expected:
            raise ValueError(
                f"{name} condition changed: expected {expected!r}, got {actual_condition!r}"
            )
        if "classify-changes" not in job_needs(jobs[name]):
            raise ValueError(f"{name} must depend on classify-changes")
        output = JOB_CONDITIONS[name]
        if classify_outputs.get(output) != f"${{{{ steps.classify.outputs.{output} }}}}":
            raise ValueError(f"classify-changes must expose {output}")

    for name, platform in ARTIFACT_DEPENDENCIES.items():
        if f"build-{platform}" not in job_needs(jobs[name]):
            raise ValueError(f"{name} must depend on its {platform} artifact")
    for name, output in MATRIX_JOBS.items():
        strategy = jobs[name].get("strategy")
        matrix = strategy.get("matrix") if isinstance(strategy, dict) else None
        if matrix != f"${{{{ fromJSON(needs.classify-changes.outputs.{output}) }}}}":
            raise ValueError(f"{name} must use the classified matrix")
    gate_steps = _job_steps(gate, "ci.yml", "ci-success")
    if not any(
        "check-ci-contract.py --results" in str(step.get("run", ""))
        and isinstance(step.get("env"), dict)
        and step["env"].get("CI_NEEDS") == "${{ toJSON(needs) }}"
        for step in gate_steps
    ):
        raise ValueError("ci-success must verify selected jobs, not just absence of failures")

    for job, command in {
        "docs-links": "check_docs.py links",
        "docs-contracts": "check_docs.py contracts",
    }.items():
        if command not in _job_run_text(jobs[job]):
            raise ValueError(f"{job} must execute its documentation checks")
    classify_perms = classify_job.get("permissions") or {}
    if not isinstance(classify_perms, dict) or classify_perms.get("deployments") != "read":
        raise ValueError("documentation selection is missing deployments: read")
    classify_steps = _job_steps(classify_job, "ci.yml", "classify-changes")
    if not any("docs-publication-base.py" in str(step.get("run", "")) for step in classify_steps):
        raise ValueError("documentation selection is missing docs-publication-base.py")
    if not any(
        isinstance(step.get("env"), dict)
        and step["env"].get("DOCS_BASE_SHA") == "${{ steps.published-docs.outputs.base_sha }}"
        for step in classify_steps
    ):
        raise ValueError(
            "documentation selection is missing DOCS_BASE_SHA: ${{ steps.published-docs.outputs.base_sha }}"
        )
    if not any(
        "hooks/check-sensitive.sh --range" in str(step.get("run", ""))
        and isinstance(step.get("env"), dict)
        and step["env"].get("BASE_SHA") == "${{ steps.range.outputs.base_sha }}"
        and step["env"].get("HEAD_SHA") == "${{ steps.range.outputs.head_sha }}"
        for step in classify_steps
    ):
        raise ValueError(
            "classify-changes must scan the commit range with hooks/check-sensitive.sh --range"
        )


def check_mutation(workflow: str) -> None:
    check_checkout_steps(workflow, "mutation.yml")
    parsed = parse_workflow(workflow, "mutation.yml")
    jobs = parsed["jobs"]
    assert isinstance(jobs, dict)
    if jobs.keys() != {"select", "mutation", "mutation-windows", "mutation-results"}:
        raise ValueError("unexpected mutation workflow jobs")
    triggers = parsed.get("on")
    if not isinstance(triggers, dict):
        raise ValueError("mutation workflow must define triggers")
    for forbidden in ("push", "pull_request", "workflow_run"):
        if forbidden in triggers:
            raise ValueError("mutation testing must not run on normal CI events")
    for required in ("schedule", "workflow_dispatch"):
        if required not in triggers:
            raise ValueError(f"mutation scheduling is missing {required}")
    perms = parsed.get("permissions")
    select_perms = jobs["select"].get("permissions")
    if (
        not isinstance(perms, dict)
        or perms.get("contents") != "read"
        or not isinstance(select_perms, dict)
        or select_perms.get("actions") != "read"
        or select_perms.get("contents") != "read"
    ):
        raise ValueError("mutation scheduling is missing read-only actions/contents permissions")
    if "select-mutations.py" not in _job_run_text(jobs["select"]):
        raise ValueError("mutation scheduling is missing select-mutations.py")
    for name, flag in (("mutation", "run_linux"), ("mutation-windows", "run_windows")):
        if job_condition(jobs[name]) != f"needs.select.outputs.{flag} == 'true'":
            raise ValueError(f"mutation selection condition changed for {name}")
        if job_needs(jobs[name]) != {"select"}:
            raise ValueError(f"{name} must depend on mutation selection")
    if job_condition(jobs["mutation-results"]) != "always()" or job_needs(
        jobs["mutation-results"]
    ) != {
        "select",
        "mutation",
        "mutation-windows",
    }:
        raise ValueError("mutation results must report every selected job")
    for job_name, job in jobs.items():
        if "continue-on-error" in job or any(
            "continue-on-error" in step for step in _job_steps(job, "mutation.yml", job_name)
        ):
            raise ValueError("mutation workflow must propagate failures")

    results_steps = _job_steps(jobs["mutation-results"], "mutation.yml", "mutation-results")
    if not any(
        isinstance(step.get("with"), dict) and step["with"].get("pattern") == "mutation-result-*"
        for step in results_steps
    ):
        raise ValueError("mutation results must consume shard reports: pattern: mutation-result-*")
    results_run = _job_run_text(jobs["mutation-results"])
    for required in ("report-mutations.py", "mutation-inputs/mutation-plan.json"):
        if required not in results_run:
            raise ValueError(f"mutation results must consume shard reports: {required}")

    mutation = jobs["mutation"]
    strategy = mutation.get("strategy")
    if not isinstance(strategy, dict):
        raise ValueError("Linux mutation job must define a matrix strategy")
    if strategy.get("matrix") != "${{ fromJSON(needs.select.outputs.linux_matrix) }}":
        raise ValueError(
            "Linux mutation coverage missing matrix: ${{ fromJSON(needs.select.outputs.linux_matrix) }}"
        )
    if strategy.get("fail-fast") != "false":
        raise ValueError("Linux mutation coverage missing fail-fast: false")
    parallel = strategy.get("max-parallel")
    if not isinstance(parallel, str) or not parallel.isdigit() or not 1 <= int(parallel) <= 14:
        raise ValueError("Linux mutation concurrency must leave runner capacity for ordinary CI")

    mutation_steps = _job_steps(mutation, "mutation.yml", "mutation")
    mutation_run = _job_run_text(mutation)
    if not any(
        isinstance(step.get("with"), dict) and step["with"].get("name") == "mutation-inputs"
        for step in mutation_steps
    ):
        raise ValueError("Linux mutation coverage missing name: mutation-inputs")
    if not any(
        isinstance(step.get("with"), dict) and "cargo-nextest@" in str(step["with"].get("tool", ""))
        for step in mutation_steps
    ):
        raise ValueError("Linux mutation coverage missing cargo-nextest@")
    for required in (
        "run-mutations.py",
        'run "$MUTATION_SUITE" --partition "$MUTATION_PARTITION" --test-tool nextest',
    ):
        if required not in mutation_run:
            raise ValueError(f"Linux mutation coverage missing {required}")

    select_run = _job_run_text(jobs["select"])
    select_steps = _job_steps(jobs["select"], "mutation.yml", "select")
    select_paths = "\n".join(
        str(step["with"].get("path", ""))
        for step in select_steps
        if isinstance(step.get("with"), dict)
    )
    if "run-mutations.py plan" not in select_run:
        raise ValueError("mutation selection missing run-mutations.py plan")
    for required in ("cli/mutation-plan.json", "cli/mutants.diff"):
        if required not in select_paths:
            raise ValueError(f"mutation selection missing {required}")

    native = jobs["mutation-windows"]
    if native.get("runs-on") != "windows-latest":
        raise ValueError("native Windows mutation coverage missing runs-on: windows-latest")
    native_strategy = native.get("strategy")
    native_matrix = native_strategy.get("matrix") if isinstance(native_strategy, dict) else None
    if not isinstance(native_matrix, dict) or native_matrix.get("shard") != ["0", "1"]:
        raise ValueError("native Windows mutation coverage missing shard: [0, 1]")
    native_steps = _job_steps(native, "mutation.yml", "mutation-windows")
    if not any(
        isinstance(step.get("with"), dict) and "cargo-nextest@" in str(step["with"].get("tool", ""))
        for step in native_steps
    ):
        raise ValueError("native Windows mutation coverage missing cargo-nextest@")
    native_run = _job_run_text(native)
    for required in (
        "run-mutations.py",
        'run windows --partition "$env:MUTATION_SHARD/2" --test-tool nextest',
    ):
        if required not in native_run:
            raise ValueError(f"native Windows mutation coverage missing {required}")


def check_release(workflow: str) -> None:
    check_checkout_steps(workflow, "release.yml")
    parsed = parse_workflow(workflow, "release.yml")
    concurrency = parsed.get("concurrency")
    if not isinstance(concurrency, dict) or concurrency.get("group") != "release":
        raise ValueError("release workflow is missing serialized release concurrency")
    if concurrency.get("cancel-in-progress") != "false":
        raise ValueError("release workflow is missing non-cancelling release concurrency")
    triggers = parsed.get("on")
    if isinstance(triggers, dict) and "workflow_dispatch" in triggers:
        raise ValueError("release workflow permits cache-poisonable manual dispatches")
    if "github.event.workflow_run.head_sha || github.sha" in workflow:
        raise ValueError("releases can fall back to an unverified workflow commit")

    jobs = parsed["jobs"]
    assert isinstance(jobs, dict)
    if job_condition(jobs["version"]) != "needs.check-ci.outputs.run_release == 'true'":
        raise ValueError("release version resolution must require changed binary inputs")
    version_outputs = jobs["version"].get("outputs") or {}
    version_steps = _job_steps(jobs["version"], "release.yml", "version")
    if "needs.check-ci.outputs.sha" not in str(version_outputs) and not any(
        "needs.check-ci.outputs.sha" in str(step) for step in version_steps
    ):
        raise ValueError("release version resolution is missing the validated CI commit handoff")

    classify_job = jobs["check-ci"]
    classify_condition = job_condition(classify_job) or ""
    for guard in (
        "github.event.workflow_run.conclusion == 'success'",
        "github.event.workflow_run.event == 'push'",
        "github.event.workflow_run.head_branch == 'main'",
        "github.event.workflow_run.head_repository.full_name == github.repository",
    ):
        if guard not in classify_condition:
            raise ValueError(f"release classification is missing {guard}")
    classify_outputs = classify_job.get("outputs") or {}
    if classify_outputs.get("run_release") != "${{ steps.classify.outputs.run_release }}":
        raise ValueError(
            "release classification is missing run_release: ${{ steps.classify.outputs.run_release }}"
        )
    classify_steps = _job_steps(classify_job, "release.yml", "check-ci")
    if not any(
        isinstance(step.get("with"), dict)
        and step["with"].get("ref") == "${{ steps.resolve.outputs.sha }}"
        for step in classify_steps
    ):
        raise ValueError("release classification is missing ref: ${{ steps.resolve.outputs.sha }}")
    if "classify-release.sh" not in _job_run_text(classify_job):
        raise ValueError("release classification is missing classify-release.sh")

    for name in ("build-linux", "build-windows", "release"):
        if "version" not in job_needs(jobs[name]):
            raise ValueError(f"release {name} must depend on the selected version")
    for name in ("build-linux", "build-windows"):
        build_steps = _job_steps(jobs[name], "release.yml", name)
        if not any(
            isinstance(step.get("with"), dict) and step["with"].get("retention-days") == "1"
            for step in build_steps
        ):
            raise ValueError(f"release {name} handoff artifact is not limited to one-day retention")
    for name, command in {
        "version": 'release-tag.sh next "$date_part"',
        "release": 'release-tag.sh check "$VERSION"',
    }.items():
        if command not in _job_run_text(jobs[name]):
            raise ValueError(f"release {name} must use the checked remote tag helper")

    release_job = jobs["release"]
    release_perms = release_job.get("permissions") or {}
    for perm, description in (
        ("id-token", "OIDC permission for provenance signing"),
        ("attestations", "attestation upload permission"),
    ):
        if not isinstance(release_perms, dict) or release_perms.get(perm) != "write":
            raise ValueError(f"release workflow is missing {description}")

    release_steps = _job_steps(release_job, "release.yml", "release")
    step_positions = {
        step.get("name"): idx
        for idx, step in enumerate(release_steps)
        if isinstance(step.get("name"), str)
    }
    attest_idx = step_positions.get("Attest build provenance")
    verify_idx = step_positions.get("Verify attestation discoverability")
    create_idx = step_positions.get("Create release")
    if (
        attest_idx is None
        or verify_idx is None
        or create_idx is None
        or not (attest_idx < verify_idx < create_idx)
    ):
        raise ValueError("release provenance is not verified before publication")

    attest_step = release_steps[attest_idx]
    verify_step = release_steps[verify_idx]
    create_step = release_steps[create_idx]
    if not str(attest_step.get("uses", "")).startswith("actions/attest-build-provenance@"):
        raise ValueError("release workflow is missing the build provenance action")
    attest_with = attest_step.get("with")
    subjects = {
        line.strip()
        for line in str(
            attest_with.get("subject-path", "") if isinstance(attest_with, dict) else ""
        ).splitlines()
        if line.strip()
    }
    for subject, description in (
        ("dotfiles-linux-x86_64", "the Linux x86_64 attestation subject"),
        ("dotfiles-linux-aarch64", "the Linux aarch64 attestation subject"),
        ("dotfiles-windows-x86_64.exe", "the Windows attestation subject"),
    ):
        if subject not in subjects:
            raise ValueError(f"release workflow is missing {description}")
    if 'gh attestation verify "$artifact" --repo "$GITHUB_REPOSITORY"' not in str(
        verify_step.get("run", "")
    ):
        raise ValueError("release workflow is missing the attestation discoverability check")
    create_with = create_step.get("with")
    if (
        not isinstance(create_with, dict)
        or create_with.get("target_commitish") != "${{ needs.version.outputs.sha }}"
    ):
        raise ValueError("release workflow is missing the exact tested release tag target")


def check_pages(workflow: str, ci_workflow: str) -> None:
    parsed = parse_workflow(workflow, "pages.yml")
    jobs = parsed["jobs"]
    assert isinstance(jobs, dict)
    select, package, deploy = jobs["select-build"], jobs["package"], jobs["deploy"]
    select_condition = job_condition(select) or ""
    for guard in (
        "github.event.workflow_run.conclusion == 'success'",
        "github.event.workflow_run.event == 'push'",
        "github.event.workflow_run.head_branch == 'main'",
        "github.event.workflow_run.head_repository.full_name == github.repository",
    ):
        if guard not in select_condition:
            raise ValueError(f"Pages build selection is missing {guard}")
    select_outputs = select.get("outputs") or {}
    if select_outputs.get("current") != "${{ steps.current.outputs.current }}":
        raise ValueError(
            "Pages build selection is missing current: ${{ steps.current.outputs.current }}"
        )
    select_steps = _job_steps(select, "pages.yml", "select-build")
    select_env = {}
    for step in select_steps:
        if isinstance(step.get("env"), dict):
            select_env.update(step["env"])
    for key, expected in (
        ("BUILD_SHA", "${{ github.event.workflow_run.head_sha }}"),
        ("CI_RUN_ID", "${{ github.event.workflow_run.id }}"),
    ):
        if select_env.get(key) != expected:
            raise ValueError(f"Pages build selection is missing {key}: {expected}")
    select_run = _job_run_text(select)
    for guard in (
        'if [ "$current_sha" = "$BUILD_SHA" ] && [ "$GITHUB_SHA" = "$BUILD_SHA" ]; then',
        'case "$site_result" in',
        'select(.name == "Docs: Content and site") | .conclusion',
        "skipped)",
        "success) ;;",
        '*) echo "Cannot verify the CI documentation site job: $site_result" >&2; exit 1 ;;',
    ):
        if guard not in select_run:
            raise ValueError(f"Pages build selection is missing {guard}")
    if "environment" in select:
        raise ValueError(
            "Pages build selection must run before entering the deployment environment"
        )

    if job_needs(package) != {"select-build"}:
        raise ValueError("Pages packaging is missing needs: select-build")
    if job_condition(package) != "needs.select-build.outputs.current == 'true'":
        raise ValueError(
            "Pages packaging is missing if: needs.select-build.outputs.current == 'true'"
        )
    package_env = package.get("env") or {}
    if (
        not isinstance(package_env, dict)
        or package_env.get("PAGES_ARTIFACT") != "github-pages-${{ github.run_attempt }}"
    ):
        raise ValueError(
            "Pages packaging is missing PAGES_ARTIFACT: github-pages-${{ github.run_attempt }}"
        )
    package_outputs = package.get("outputs") or {}
    if package_outputs.get("artifact_name") != "${{ env.PAGES_ARTIFACT }}":
        raise ValueError("Pages packaging is missing artifact_name: ${{ env.PAGES_ARTIFACT }}")
    package_steps = _job_steps(package, "pages.yml", "package")
    if not any(
        isinstance(step.get("with"), dict)
        and step["with"].get("run-id") == "${{ github.event.workflow_run.id }}"
        and step["with"].get("name") == "docs-site"
        for step in package_steps
    ):
        raise ValueError(
            "Pages packaging is missing docs-site artifact download from workflow_run.id"
        )
    if not any(
        str(step.get("uses", "")).startswith("actions/upload-pages-artifact@")
        and isinstance(step.get("with"), dict)
        and step["with"].get("name") == "${{ env.PAGES_ARTIFACT }}"
        for step in package_steps
    ):
        raise ValueError("Pages packaging is missing name: ${{ env.PAGES_ARTIFACT }}")

    if job_needs(deploy) != {"package"}:
        raise ValueError("Pages deployment is missing needs: package")
    deploy_env = deploy.get("environment") or {}
    if (
        not isinstance(deploy_env, dict)
        or deploy_env.get("url") != "${{ steps.deployment.outputs.page_url }}"
    ):
        raise ValueError(
            "Pages deployment is missing url: ${{ steps.deployment.outputs.page_url }}"
        )
    deploy_steps = _job_steps(deploy, "pages.yml", "deploy")
    if not any(
        isinstance(step.get("with"), dict)
        and step["with"].get("artifact_name") == "${{ needs.package.outputs.artifact_name }}"
        for step in deploy_steps
    ):
        raise ValueError(
            "Pages deployment is missing artifact_name: ${{ needs.package.outputs.artifact_name }}"
        )
    if any(
        str(step.get("uses", "")).startswith("actions/upload-pages-artifact@")
        for step in deploy_steps
    ):
        raise ValueError("Retrying Pages deployment must not upload another artifact")
    if any(
        str(step.get("uses", "")).startswith("actions/checkout@")
        for step in package_steps + deploy_steps
    ):
        raise ValueError("Pages deployment must use the verified artifact, not execute a checkout")

    ci_doc = parse_workflow(ci_workflow, "ci.yml")
    docs = ci_doc["jobs"]["docs"]
    docs_run = _job_run_text(docs)
    for required in (
        "check_docs.py site",
        "npx playwright install --with-deps --only-shell chromium",
    ):
        if required not in docs_run:
            raise ValueError(f"CI documentation artifact is missing {required}")
    docs_steps = _job_steps(docs, "ci.yml", "docs")
    if not any(
        step.get("if") == "github.event_name == 'push' && github.ref == 'refs/heads/main'"
        and isinstance(step.get("with"), dict)
        and step["with"].get("name") == "docs-site"
        and step["with"].get("path") == "docs/dist/"
        for step in docs_steps
    ):
        raise ValueError(
            "CI documentation artifact is missing main-push docs-site upload of docs/dist/"
        )


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

    windows_sources = {
        rel_path: read_source(rel_path) for rel_path in mutation_run["WINDOWS_FILES"]
    }

    for table_name in ("LINUX_EXCLUSIONS", "WINDOWS_EXCLUSIONS"):
        for option, value in mutation_run[table_name]:
            if option == "--exclude":
                read_source(value)
            elif option == "--exclude-re":
                match = re.match(r"^\^(src/[^:]+?):(.*)$", value)
                if match is None:
                    raise ValueError(f"unrecognized {table_name} file prefix: {value}")
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
                        raise ValueError(
                            f"mutation reference {rel_path} is missing mod {module.group(1)}"
                        )
                elif impl := re.search(
                    r"(?:\bin|replace) <impl ([A-Za-z0-9_:]+) for ([A-Za-z0-9_]+)(?:<[^>]*>)?>::([A-Za-z_][A-Za-z0-9_]*)(?: with \\\(\\\))?\$$",
                    remainder,
                ):
                    trait, target_type, method = impl.groups()
                    if not re.search(
                        rf"\bimpl\b[^{{;]*\b{re.escape(trait)}\b\s+for\s+{re.escape(target_type)}\b",
                        source,
                    ):
                        raise ValueError(
                            f"mutation reference {rel_path} is missing impl {trait} for {target_type}"
                        )
                    require_fn(rel_path, source, method)
                elif target_fn := re.search(
                    r"\bin (?:([A-Za-z_][A-Za-z0-9_]*)::)?([A-Za-z_][A-Za-z0-9_]*)\$$",
                    remainder,
                ):
                    type_name, fn_name = target_fn.groups()
                    if type_name is not None and not re.search(
                        rf"\bimpl\b[^{{;]*\b{re.escape(type_name)}\b", source
                    ):
                        raise ValueError(
                            f"mutation reference {rel_path} is missing impl {type_name}"
                        )
                    require_fn(rel_path, source, fn_name)
                elif remainder == r"[0-9]+:[0-9]+: replace \| with \^$":
                    if not re.search(r"\bconst\b[^;]*\|", source):
                        raise ValueError(
                            f"mutation reference {rel_path} has no bitwise-OR const for | -> ^ exclusion"
                        )
                else:
                    raise ValueError(f"unrecognized {table_name} symbol pattern: {value}")
            else:
                raise ValueError(f"unexpected {table_name} option: {option}")

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
            found = any(
                re.search(rf"\bfn\s+{re.escape(symbol)}\b", src) for src in windows_sources.values()
            )
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
        if (
            end != len(unescaped)
            or not paths
            or any(re.search(r"[\\*+?\[\]{}^$]", path) for path in paths)
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
        if len(sys.argv) == 3 and sys.argv[1] == "--release":
            source = (
                sys.stdin.read()
                if sys.argv[2] == "-"
                else Path(sys.argv[2]).read_text(encoding="utf-8")
            )
            check_release(source)
            print("Release workflow matches the contract")
            sys.exit(0)
        for workflow_path in sorted(WORKFLOW.parent.glob("*.yml")):
            check_checkout_steps(workflow_path.read_text(encoding="utf-8"), workflow_path.name)
        check(WORKFLOW.read_text(encoding="utf-8"))
        check_mutation(WORKFLOW.with_name("mutation.yml").read_text(encoding="utf-8"))
        check_mutation_symbols()
        check_release(WORKFLOW.with_name("release.yml").read_text(encoding="utf-8"))
        check_pages(
            WORKFLOW.with_name("pages.yml").read_text(encoding="utf-8"),
            WORKFLOW.read_text(encoding="utf-8"),
        )
        if sys.argv[1:] == ["--results"]:
            check_results(json.loads(os.environ["CI_NEEDS"]))
        elif sys.argv[1:]:
            raise ValueError(f"unsupported arguments: {' '.join(sys.argv[1:])}")
    except ValueError as error:
        print(f"CI contract failed: {error}", file=sys.stderr)
        sys.exit(1)
    print("CI job gate and scheduling conditions match the contract")
