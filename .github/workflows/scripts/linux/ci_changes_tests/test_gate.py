"""CI contract and aggregate gate regression tests."""

from itertools import pairwise
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from ci_changes_tests.helpers import (
    CONTRACT,
    MUTATION_REPORT,
    MUTATION_RUN,
    SCRIPTS,
    classify,
)


class GateTests(unittest.TestCase):
    def test_archlinux_image_digest_must_be_consistent(self):
        workflow = CONTRACT["WORKFLOW"].read_text(encoding="utf-8")
        CONTRACT["check"](workflow)
        drifted = re.sub(
            r"archlinux@sha256:[0-9a-f]{64}",
            "archlinux@sha256:" + "0" * 64,
            workflow,
            count=1,
        )
        with self.assertRaisesRegex(ValueError, "single archlinux@sha256 digest"):
            CONTRACT["check"](drifted)

    def test_scheduled_security_checks_are_independent_and_read_only(self):
        workflow = (
            CONTRACT["WORKFLOW"].with_name("dependency-security.yml").read_text(encoding="utf-8")
        )
        self.assertRegex(workflow, r"(?m)^  schedule:$")
        self.assertRegex(workflow, r"(?m)^    - cron: '.+'$")
        self.assertIn("  workflow_dispatch:", workflow)
        self.assertIn("permissions:\n  contents: read\n", workflow)
        self.assertNotRegex(workflow, r"(?m)^\s+(?:paths|paths-ignore):")
        for block in CONTRACT["job_blocks"](workflow).values():
            self.assertIsNone(CONTRACT["job_condition"](block))
            self.assertFalse(CONTRACT["job_needs"](block))
        self.assertIn("run: cargo audit", workflow)
        self.assertIn("if: ${{ !cancelled() }}\n        run: cargo deny check all", workflow)
        self.assertNotIn("continue-on-error:", workflow)
        self.assertNotIn("write", workflow)

    def results(self, paths):
        outputs = {name: str(value).lower() for name, value in classify(paths).items()}
        needs = {
            name: {"result": "success" if outputs[output] == "true" else "skipped"}
            for name, output in CONTRACT["JOB_CONDITIONS"].items()
            if name not in CONTRACT["INFORMATIONAL_JOBS"]
        }
        needs["classify-changes"] = {"result": "success", "outputs": outputs}
        return needs

    def test_intentional_skips(self):
        for paths in (
            ["README.md"],
            ["dotfiles.ps1"],
            ["conf/packages.toml"],
            ["cli/src/main.rs"],
            ["symlinks/config/quickshell/bar/Bar.qml"],
            ["packages/apm/PKGBUILD"],
        ):
            with self.subTest(paths=paths):
                CONTRACT["check_results"](self.results(paths))

    def test_failures_cancellations_and_unexpected_skips(self):
        for job, result in (
            ("classify-changes", "failure"),
            ("docs", "skipped"),
            ("docs", "cancelled"),
            ("docs", "failure"),
            ("build-linux", "failure"),
            ("build-linux", "success"),
        ):
            with self.subTest(job=job, result=result):
                needs = self.results(["docs/content/development/testing.md"])
                needs[job]["result"] = result
                with self.assertRaises(ValueError):
                    CONTRACT["check_results"](needs)
        needs = self.results(["README.md"])
        del needs["classify-changes"]["outputs"]["run_build_linux"]
        with self.assertRaises(ValueError):
            CONTRACT["check_results"](needs)

    def test_every_selected_job_must_report_success(self):
        paths = [".github/workflows/ci.yml", "cli/src/main.rs"]
        for job in self.results(paths):
            for result in ("failure", "cancelled", "skipped"):
                with self.subTest(job=job, result=result):
                    needs = self.results(paths)
                    needs[job]["result"] = result
                    with self.assertRaises(ValueError):
                        CONTRACT["check_results"](needs)

    def test_missing_jobs_and_malformed_selection_outputs_fail_closed(self):
        baseline = self.results(["docs/content/development/testing.md"])
        for job in baseline:
            with self.subTest(missing_job=job):
                needs = self.results(["README.md"])
                del needs[job]
                with self.assertRaises(ValueError):
                    CONTRACT["check_results"](needs)
        gated_outputs = {
            output
            for job, output in CONTRACT["JOB_CONDITIONS"].items()
            if job not in CONTRACT["INFORMATIONAL_JOBS"]
        }
        for output in sorted(gated_outputs):
            for value in (None, "", "TRUE", "false\n", True):
                with self.subTest(output=output, value=value):
                    needs = self.results(["README.md"])
                    needs["classify-changes"]["outputs"][output] = value
                    with self.assertRaises(ValueError):
                        CONTRACT["check_results"](needs)

    def test_workflow_contracts(self):
        workflow = CONTRACT["WORKFLOW"].read_text(encoding="utf-8")
        CONTRACT["check"](workflow)
        release = CONTRACT["WORKFLOW"].with_name("release.yml").read_text(encoding="utf-8")
        CONTRACT["check_release"](release)
        pages = CONTRACT["WORKFLOW"].with_name("pages.yml").read_text(encoding="utf-8")
        CONTRACT["check_pages"](pages, workflow)
        for broken in (
            pages.replace("github.event.workflow_run.event == 'push'", "true"),
            pages.replace(
                "github.event.workflow_run.head_repository.full_name == github.repository", "true"
            ),
            pages.replace("run-id: ${{ github.event.workflow_run.id }}", "run-id: 1"),
            pages.replace("github-pages-${{ github.run_attempt }}", "github-pages"),
            pages.replace(" name: ${{ env.PAGES_ARTIFACT }}", " name: github-pages"),
            pages.replace("needs.package.outputs.artifact_name", "github.run_attempt"),
            pages.replace(
                'select(.name == "Docs: Content and site")', 'select(.name == "Other job")'
            ),
            pages.replace("if: needs.select-build.outputs.current == 'true'", "if: always()", 1),
        ):
            with self.assertRaises(ValueError):
                CONTRACT["check_pages"](broken, workflow)
        for broken in (
            workflow.replace("      - docs\n", ""),
            workflow.replace("test-docs.sh docs_links", "true"),
            workflow.replace("test-docs.sh docs_task_selectors", "true"),
            workflow.replace("DOCS_BASE_SHA:", "UNUSED_BASE_SHA:"),
            workflow.replace("      - desktop-qml\n", ""),
            workflow.replace("run_build_windows == 'true'", "run_build_linux == 'true'"),
            workflow.replace("check-ci-contract.py --results", "echo success"),
            workflow.replace(
                "        with:\n          persist-credentials: false",
                "      with:\n        persist-credentials: false",
                1,
            ),
            workflow.replace(
                "        with:\n          fetch-depth: 0\n          persist-credentials: false",
                "        with:\n          persist-credentials: false\n        with:\n          fetch-depth: 0",
                1,
            ),
            workflow.replace(
                "        with:\n          fetch-depth: 0\n          persist-credentials: false",
                "        with:\n          fetch-depth: 0",
                1,
            ),
        ):
            with self.assertRaises(ValueError):
                CONTRACT["check"](broken)
        for label, broken in (
            ("run_release condition", release.replace("run_release == 'true'", "true")),
            ("concurrency group", release.replace("  group: release\n", "")),
            (
                "cancel-in-progress",
                release.replace("  cancel-in-progress: false\n", "  cancel-in-progress: true\n"),
            ),
            (
                "workflow_dispatch",
                release.replace("  workflow_run:\n", "  workflow_dispatch:\n  workflow_run:\n", 1),
            ),
            (
                "head_sha fallback",
                release.replace(
                    "needs.check-ci.outputs.sha",
                    "github.event.workflow_run.head_sha || github.sha",
                    1,
                ),
            ),
            ("version sha handoff", release.replace("needs.check-ci.outputs.sha", "github.sha")),
            ("retention-days", release.replace("          retention-days: 1\n", "", 1)),
            ("id-token permission", release.replace("      id-token: write\n", "")),
            ("attestations permission", release.replace("      attestations: write\n", "")),
            ("attestation subject", release.replace("            dotfiles-linux-aarch64\n", "", 1)),
            (
                "discoverability check",
                release.replace(
                    'gh attestation verify "$artifact" --repo "$GITHUB_REPOSITORY"', "true"
                ),
            ),
            (
                "target_commitish",
                release.replace("target_commitish: ${{ needs.version.outputs.sha }}\n", ""),
            ),
            (
                "step order",
                release.replace(
                    "      - name: Verify attestation discoverability",
                    "__TEMP_STEP__",
                )
                .replace(
                    "      - name: Create release",
                    "      - name: Verify attestation discoverability",
                )
                .replace(
                    "__TEMP_STEP__",
                    "      - name: Create release",
                ),
            ),
        ):
            with self.subTest(release_case=label), self.assertRaises(ValueError):
                CONTRACT["check_release"](broken)
        contract_cli = SCRIPTS / "check-ci-contract.py"
        ok = subprocess.run(
            [sys.executable, "-B", str(contract_cli), "--release", "-"],
            input=release,
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(ok.returncode, 0, ok.stderr)
        bad = subprocess.run(
            [sys.executable, "-B", str(contract_cli), "--release", "-"],
            input=release.replace("  group: release\n", ""),
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertNotEqual(bad.returncode, 0)
        for platform in ("linux", "windows"):
            build = CONTRACT["job_blocks"](workflow)["build-" + platform]
            for step in (
                ("Clippy", "Tests", "Rustdoc") if platform == "linux" else ("Clippy", "Tests")
            ):
                self.assertIn(
                    f"- name: {step}\n        if: needs.classify-changes.outputs.run_rust_checks == 'true'",
                    build,
                )

    def test_checkout_contract_decodes_quoted_yaml_scalars(self):
        def make_workflow(uses: str, with_block: str = "") -> str:
            return (
                "name: Fixture\n"
                "jobs:\n"
                "  sample:\n"
                "    runs-on: ubuntu-latest\n"
                "    steps:\n"
                f"      - uses: {uses}\n"
                f"{with_block}"
            )

        sha = "de0fac2e4500dabe0009e67214ff5f5447ce83dd"
        for uses in (
            f"actions/checkout@{sha} # v6.0.2",
            f"'actions/checkout@{sha}' # v6.0.2",
            f'"actions/checkout@{sha}" # v6.0.2',
        ):
            for persist in (
                "false",
                "'false'",
                '"false"',
                "'false' # required",
                '"false"\t# required',
            ):
                with self.subTest(valid=True, uses=uses, persist=persist):
                    CONTRACT["check_checkout_steps"](
                        make_workflow(
                            uses, f"        with:\n          persist-credentials: {persist}\n"
                        ),
                        "fixture.yml",
                    )
            for with_block in (
                "",
                "        with:\n          fetch-depth: 0\n",
                "        with:\n          persist-credentials: true\n",
                "        with:\n          persist-credentials: 'true'\n",
                '        with:\n          persist-credentials: "true"\n',
                "        with:\n          persist-credentials: 'false' extra\n",
            ):
                with (
                    self.subTest(valid=False, uses=uses, with_block=with_block),
                    self.assertRaises(ValueError),
                ):
                    CONTRACT["check_checkout_steps"](make_workflow(uses, with_block), "fixture.yml")

    def test_workflow_yaml_handles_varied_indentation_and_rejects_duplicates(self):
        sha = "de0fac2e4500dabe0009e67214ff5f5447ce83dd"
        four_space = (
            "name: FourSpace\n"
            "jobs:\n"
            "    first:\n"
            "        runs-on: ubuntu-latest\n"
            "        needs:\n"
            "            - dep-a\n"
            "            - dep-b\n"
            "        steps:\n"
            f"            - uses: actions/checkout@{sha}\n"
            "              with:\n"
            "                  persist-credentials: false\n"
            "    second:\n"
            "        runs-on: ubuntu-latest\n"
            "        needs: [first]\n"
            "        steps:\n"
            "            - run: echo ok\n"
        )
        parsed = CONTRACT["parse_workflow"](four_space, "four-space.yml")
        self.assertEqual(
            CONTRACT["normalize_needs"](parsed["jobs"]["first"].get("needs")), {"dep-a", "dep-b"}
        )
        self.assertEqual(
            CONTRACT["normalize_needs"](parsed["jobs"]["second"].get("needs")), {"first"}
        )
        CONTRACT["check_checkout_steps"](four_space, "four-space.yml")
        duplicate_key = (
            "name: Duplicate\n"
            "jobs:\n"
            "  sample:\n"
            "    runs-on: ubuntu-latest\n"
            "    runs-on: windows-latest\n"
        )
        with self.assertRaises(ValueError):
            CONTRACT["parse_workflow"](duplicate_key, "duplicate.yml")

    def test_linux_mutation_excludes_only_native_backends_and_equivalent_guard(self):
        workflow = CONTRACT["WORKFLOW"].with_name("mutation.yml").read_text(encoding="utf-8")
        mutation = CONTRACT["job_blocks"](workflow)["mutation"]
        self.assertIn("runs-on: ubuntu-latest", mutation)
        exclusions = [
            re.compile(pattern)
            for option, pattern in MUTATION_RUN["LINUX_EXCLUSIONS"]
            if option == "--exclude-re"
        ]
        self.assertTrue(exclusions)

        def excluded(path, description):
            name = f"src/{path}.rs:390:5: {description}"
            return any(pattern.search(name) for pattern in exclusions)

        windows_only = {
            "infra/elevation": ("run_elevated_child", "wait_if_elevated"),
            "app/commands/execution/elevation": ("prepare_windows_elevation",),
            "domains/dotfiles/resources/path_entry": (
                "decode_reg_string",
                "encode_reg_string",
                "append_user_path",
                "user_path_contains",
            ),
            "domains/system/resources/registry": (
                "native::raw_value_to_string",
                "native::raw_value_type",
                "native::read_value",
                "native::write_value",
                "native::parse_path",
            ),
        }
        for path, functions in windows_only.items():
            for function in functions:
                for description in (
                    f"replace {function} -> String with String::new()",
                    f'replace {function} -> String with "xyzzy".into()',
                    f"replace == with != in {function}",
                    f"replace && with || in {function}",
                ):
                    with self.subTest(path=path, description=description):
                        self.assertTrue(excluded(path, description))

        portable = {
            "infra/elevation": ("should_pause", "wait_if_elevated_policy"),
            "app/commands/execution/elevation": (
                "prepare_elevation",
                "prepare_sudo_elevation",
                "elevation_plan_disposition",
                "build_elevated_child_args",
            ),
            "domains/dotfiles/resources/path_entry": (
                "PathSource::is_on_path",
                "PathEntryResource::new",
                "decode_reg_string_shared",
                "path_contains_entry",
                "expand_env_vars",
                "normalize_path_entry",
                "append_user_path_shared",
                "user_path_contains_shared",
            ),
            "domains/ai/resources/agent_settings": ("AgentSettingResource::apply",),
            "domains/system/resources/registry": (
                "parse_hkcu_subkey",
                "value_matches",
                "RegistryResource::state_from_cached",
            ),
            "another_module": ("decode_reg_string", "native::read_value", "wait_if_elevated"),
        }
        for path, functions in portable.items():
            for function in functions:
                for description in (
                    f"replace {function} -> bool with true",
                    f"replace == with != in {function}",
                ):
                    with self.subTest(path=path, description=description):
                        self.assertFalse(excluded(path, description))

        settings = "domains/ai/resources/agent_settings"
        equivalent = (
            "replace match guard metadata.is_symlink() with true "
            "in <impl Resource for AgentSettingResource>::apply"
        )
        self.assertTrue(excluded(settings, equivalent))
        for path, description in (
            (settings, equivalent.replace("with true", "with false")),
            (settings, equivalent.replace("metadata.is_symlink()", "value_is_finite()")),
            (settings, equivalent.replace("::apply", "::current_state")),
            ("another_module", equivalent),
        ):
            with self.subTest(path=path, description=description):
                self.assertFalse(excluded(path, description))

    def test_windows_elevation_broker_mutations_stay_in_native_shards(self):
        options, target = MUTATION_RUN["suite_options"]("windows")
        for path, function in (
            ("src/app/commands/execution/elevation.rs", "prepare_windows_elevation"),
            ("src/infra/elevation.rs", "run_elevated_child"),
            ("src/infra/fs/mutation/windows.rs", "ensure_single_link"),
            ("src/infra/fs/place.rs", "rename_noreplace_windows"),
            ("src/infra/fs/mod.rs", "is_dir_like"),
            ("src/domains/system/config/registry.rs", "uppercase_native_registry_name"),
            ("src/infra/exec/process.rs", "take_stdout_windows"),
        ):
            self.assertIn(("--file", path), tuple(pairwise(options)))
            self.assertRegex(
                f"{path}:306:5: replace {function}", options[options.index("--re") + 1]
            )
        self.assertEqual(target, ["--cargo-arg=--lib"])

    def test_native_file_mutants_only_run_on_windows(self):
        linux, _ = MUTATION_RUN["suite_options"]("linux")
        windows, _ = MUTATION_RUN["suite_options"]("windows")
        native_pattern = windows[windows.index("--re") + 1]
        for file, function in (
            ("src/infra/fs/mutation/windows.rs", "ensure_single_link"),
            ("src/infra/fs/place/windows.rs", "wide_path"),
            ("src/infra/fs/place/windows/private_dir.rs", "verify_directory"),
            ("src/infra/run_lock/coordination/windows.rs", "acquire"),
            ("src/infra/run_lock/windows.rs", "legacy_owner"),
        ):
            self.assertIn(("--exclude", file), list(pairwise(linux)))
            self.assertIn(("--file", file), list(pairwise(windows)))
            self.assertRegex(f"{file}:43:5: replace {function} with ()", native_pattern)
        self.assertNotRegex("src/infra/fs/place.rs:65:5: replace wide_path with ()", native_pattern)

    def test_mutation_symbol_contract(self):
        CONTRACT["check_mutation_symbols"]()
        for broken in (
            MUTATION_RUN | {"NATIVE_MODULES": (*MUTATION_RUN["NATIVE_MODULES"], "src/missing.rs")},
            MUTATION_RUN | {"WINDOWS_FILES": (*MUTATION_RUN["WINDOWS_FILES"], "src/missing.rs")},
            MUTATION_RUN
            | {
                "LINUX_EXCLUSIONS": (
                    *MUTATION_RUN["LINUX_EXCLUSIONS"],
                    ("--exclude-re", r"^src/infra/fs/place\.rs:.*\bmissing_symbol\b"),
                ),
            },
            MUTATION_RUN
            | {
                "WINDOWS_PATTERN": MUTATION_RUN["WINDOWS_PATTERN"].replace(
                    "rename_noreplace_windows",
                    "check_information",
                ),
            },
        ):
            with self.subTest(broken=broken["WINDOWS_PATTERN"]), self.assertRaises(ValueError):
                CONTRACT["check_mutation_symbols"](mutation_run=broken)

    def test_mutations_are_separate_and_fail_visibly(self):
        workflow = CONTRACT["WORKFLOW"].with_name("mutation.yml").read_text(encoding="utf-8")
        CONTRACT["check_mutation"](workflow)
        self.assertNotIn("mutation", CONTRACT["job_blocks"](CONTRACT["WORKFLOW"].read_text()))
        for broken in (
            workflow.replace("  schedule:", "  push:"),
            workflow.replace("  mutation:\n", "  mutation:\n    continue-on-error: true\n"),
            workflow.replace("if: always()", "if: success()"),
            workflow.replace("report-mutations.py", "other.py"),
            workflow.replace("fromJSON(needs.select.outputs.linux_matrix)", "'{}'"),
            workflow.replace("      max-parallel: 14\n", ""),
            workflow.replace("      max-parallel: 14\n", "      max-parallel: 48\n"),
            workflow.replace("needs.select.outputs.run_linux", "needs.select.outputs.run_windows"),
        ):
            with self.subTest(broken=broken), self.assertRaises(ValueError):
                CONTRACT["check_mutation"](broken)

    def test_mutation_result_gate_rejects_failures_and_accidental_skips(self):
        for linux, windows, result, success in (
            ("false", "false", "skipped", True),
            ("true", "false", "failure", False),
            ("true", "false", "skipped", False),
            ("true", "false", "cancelled", False),
            ("false", "false", "success", False),
            ("", "false", "skipped", False),
        ):
            with self.subTest(linux=linux, windows=windows, result=result):
                needs = {
                    "select": {
                        "result": "success",
                        "outputs": {
                            "run_linux": linux,
                            "run_windows": windows,
                        },
                    },
                    "mutation": {"result": result},
                    "mutation-windows": {"result": "skipped"},
                }
                plan = {
                    "base": "a" * 40,
                    "head": "b" * 40,
                    "linux_sources": [],
                    "linux_shards": [],
                    "run_windows": False,
                }
                _, _, failures = MUTATION_REPORT["summarize"](plan, Path("."), needs)
                self.assertEqual(not failures, success)

    def test_selected_linux_report_must_be_complete(self):
        plan = {
            "base": "a" * 40,
            "head": "b" * 40,
            "linux_sources": ["cli/src"],
            "linux_shards": [{"id": "process", "suite": "process", "count": 2}],
            "run_windows": False,
        }
        needs = {
            "select": {
                "result": "success",
                "outputs": {"run_linux": "true", "run_windows": "false"},
            },
            "mutation": {"result": "success"},
            "mutation-windows": {"result": "skipped"},
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, _, failures = MUTATION_REPORT["summarize"](plan, root, needs)
            self.assertTrue(failures)
            shard = root / "mutation-result-process"
            shard.mkdir()
            report = {
                "start_time": "2026-11-22T09:00:00Z",
                "end_time": "2026-11-22T09:01:00Z",
                "caught": 1,
                "missed": 0,
                "timeout": 0,
                "unviable": 1,
                "total_mutants": 2,
            }
            for missed, timed_out, end, count, expected in (
                (0, 0, report["end_time"], 2, True),
                (0, 0, None, 2, False),
                (0, 0, report["end_time"], 3, False),
                (1, 0, report["end_time"], 2, False),
                (0, 1, report["end_time"], 2, False),
            ):
                with self.subTest(missed=missed, timed_out=timed_out, end=end, count=count):
                    outcome = report | {
                        "missed": missed,
                        "timeout": timed_out,
                        "end_time": end,
                        "caught": 1 - missed - timed_out,
                        "total_mutants": count,
                    }
                    mutation = "src/infra/exec/process.rs:30:4: replace || with && in collect_child"
                    (shard / "outcomes.json").write_text(json.dumps(outcome), encoding="utf-8")
                    (shard / "missed.txt").write_text((mutation + "\n") * missed, encoding="utf-8")
                    (shard / "timeout.txt").write_text(
                        (mutation + "\n") * timed_out, encoding="utf-8"
                    )
                    lines, details, failures = MUTATION_REPORT["summarize"](plan, root, needs)
                    self.assertEqual(not failures, expected)
                    self.assertIn(plan["base"], "\n".join(lines))
                    if details:
                        _, _, suite, name = details[0]
                        self.assertEqual(name, mutation)
                        self.assertIn("run process --exact", MUTATION_REPORT["replay"](suite, name))

    def test_linux_shards_cover_inventory_without_empty_jobs(self):
        for lib_count, app_count, expected_lib, expected_app in (
            (0, 0, 0, 0),
            (1, 0, 1, 0),
            (300, 250, 1, 1),
            (301, 251, 2, 2),
            (3900, 750, 13, 3),
            (20881, 2000, 13, 3),
        ):
            with self.subTest(lib_count=lib_count, app_count=app_count):
                shards = MUTATION_RUN["linux_matrix"](
                    {
                        "linux-lib": lib_count,
                        "linux-app": app_count,
                        **dict.fromkeys(MUTATION_RUN["FOCUSED"], 0),
                        "liveness": 2,
                        "process": 1,
                    },
                )
                self.assertEqual(
                    shards[0],
                    {"id": "focused", "suite": "focused", "partition": "", "count": 3},
                )
                app_shards = [shard for shard in shards if shard["suite"] == "linux-app"]
                lib_shards = [shard for shard in shards if shard["suite"] == "linux-lib"]
                self.assertEqual(len(app_shards), expected_app)
                self.assertEqual(len(lib_shards), expected_lib)
                self.assertEqual(sum(shard["count"] for shard in app_shards), app_count)
                self.assertEqual(sum(shard["count"] for shard in lib_shards), lib_count)
                self.assertTrue(all(shard["count"] > 0 for shard in shards))
                self.assertLessEqual(len(shards), 17)

    def test_focused_merge_preserves_subsuite_replay_and_completeness(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stage = root / "stage"
            out = root / "mutants.out"
            mutation = "src/infra/exec/process.rs:30:4: replace || with && in collect_child"
            for suite, start, end, caught, missed, entries in (
                ("liveness", "2026-11-22T09:00:00Z", "2026-11-22T09:00:30Z", 2, 0, []),
                ("process", "2026-11-22T09:00:31Z", "2026-11-22T09:01:00Z", 0, 1, [mutation]),
            ):
                d = stage / suite
                d.mkdir(parents=True)
                (d / "outcomes.json").write_text(
                    json.dumps(
                        {
                            "start_time": start,
                            "end_time": end,
                            "caught": caught,
                            "missed": missed,
                            "timeout": 0,
                            "unviable": 0,
                            "total_mutants": caught + missed,
                            "outcomes": [],
                        }
                    ),
                    encoding="utf-8",
                )
                (d / "missed.txt").write_text("".join(f"{e}\n" for e in entries), encoding="utf-8")
                (d / "timeout.txt").write_text("", encoding="utf-8")
            self.assertTrue(
                MUTATION_RUN["merge_focused_reports"](stage, ["liveness", "process"], out)
            )
            results = root / "results"
            (results / "mutation-result-focused").mkdir(parents=True)
            for name in ("outcomes.json", "missed.txt", "timeout.txt"):
                (results / "mutation-result-focused" / name).write_text(
                    (out / name).read_text(encoding="utf-8"),
                    encoding="utf-8",
                )
            plan = {
                "base": "a" * 40,
                "head": "b" * 40,
                "linux_sources": ["cli/src"],
                "linux_shards": [{"id": "focused", "suite": "focused", "count": 3}],
                "run_windows": False,
            }
            needs = {
                "select": {
                    "result": "success",
                    "outputs": {"run_linux": "true", "run_windows": "false"},
                },
                "mutation": {"result": "failure"},
                "mutation-windows": {"result": "skipped"},
            }
            _, details, failures = MUTATION_REPORT["summarize"](plan, results, needs)
            self.assertTrue(failures)
            self.assertEqual(
                details, [("Survivor", "rust-mutation-results-focused", "process", mutation)]
            )
            self.assertIn(
                "run process --exact", MUTATION_REPORT["replay"](details[0][2], details[0][3])
            )

    def test_nextest_and_incremental_profile_command_flags(self):
        lib_cmd = MUTATION_RUN["build_run_command"](
            "linux-lib", test_tool="nextest", partition="0/4"
        )
        self.assertIn("--profile=mutants", lib_cmd)
        self.assertIn("--test-tool=nextest", lib_cmd)
        self.assertIn("--shard", lib_cmd)
        self.assertIn("--sharding=round-robin", lib_cmd)
        self.assertIn("--cargo-arg=--lib", lib_cmd)
        self.assertIn("--exclude", lib_cmd)
        app_cmd = MUTATION_RUN["build_run_command"](
            "linux-app", test_tool="nextest", partition="0/2"
        )
        self.assertIn("not binary(domain_boundaries)", app_cmd)
        self.assertNotIn("--cargo-arg=--lib", app_cmd)
        self.assertIn("--file", app_cmd)
        linux_cmd = MUTATION_RUN["build_run_command"]("linux", test_tool="nextest", partition="0/4")
        self.assertIn("not binary(domain_boundaries)", linux_cmd)
        self.assertNotIn("--cargo-arg=--lib", linux_cmd)
        exact_cmd = MUTATION_RUN["build_run_command"](
            "linux-lib", test_tool="nextest", exact="src/lib.rs:1:1: x"
        )
        self.assertNotIn("--sharding=round-robin", exact_cmd)
        win_cmd = MUTATION_RUN["build_run_command"]("windows", test_tool="nextest", partition="0/2")
        self.assertIn("--cargo-arg=--lib", win_cmd)
        self.assertIn("--test-tool=nextest", win_cmd)
        self.assertIn("--sharding=round-robin", win_cmd)
        self.assertNotIn("not binary(domain_boundaries)", win_cmd)
        with patch.dict(os.environ, {"CARGO_INCREMENTAL": "0"}, clear=False):
            env = MUTATION_RUN["mutation_env"]()
            self.assertEqual(env["CARGO_INCREMENTAL"], "1")
            self.assertTrue(env["DOTFILES_VERSION"])

    def test_focused_mutants_are_excluded_from_the_ordinary_suite(self):
        options, _ = MUTATION_RUN["suite_options"]("linux")
        for suite, example, expected_target in (
            (
                "liveness",
                "src/engine/access.rs:6:3: replace AccessRegistry::try_acquire -> Option<AccessGuard> with None",
                "engine::access::tests::",
            ),
            (
                "context",
                "src/engine/context.rs:6:3: delete ! in Context::scoped_access",
                "engine::context::tests::",
            ),
            (
                "process",
                "src/infra/exec/process.rs:6:3: replace || with && in collect_child",
                "infra::exec::tests::interruption::bounded_command_wait_",
            ),
            (
                "dispatch",
                "src/engine/orchestrate.rs:34:11: delete ! in process_items",
                "engine::tests::orchestrate::pipelines_accumulate_mixed_states_in_both_execution_modes",
            ),
            (
                "utc",
                "src/infra/logging/utils.rs:197:64: replace / with % in civil_from_epoch_secs",
                "infra::logging::utils::tests::unix_seconds_convert_to_utc_across_day_and_calendar_boundaries",
            ),
        ):
            focused, target = MUTATION_RUN["suite_options"](suite)
            pattern = focused[focused.index("--re") + 1]
            self.assertIn(("--exclude-re", pattern), tuple(pairwise(options)))
            self.assertRegex(example, pattern)
            self.assertEqual(target, ["--cargo-arg=--lib", "--", expected_target])
            _, nextest_target = MUTATION_RUN["suite_options"](suite, test_tool="nextest")
            self.assertEqual(
                nextest_target,
                [
                    "--cargo-arg=--lib",
                    "--",
                    "--status-level=none",
                    "--final-status-level=fail",
                    expected_target,
                ],
            )
            exact, _ = MUTATION_RUN["suite_options"](suite, example)
            self.assertEqual(exact.count("--re"), 1)
            self.assertRegex(example, exact[-1])
            self.assertNotRegex(example + "_other", exact[-1])
