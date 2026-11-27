"""Mutation selection, documentation publication, and release tag tests."""

import contextlib
import io
import os
from pathlib import Path
import runpy
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

from ci_changes_tests.helpers import (
    CONTRACT,
    MUTATION,
    SCRIPTS,
    SCRIPTS_PREFIX,
)


class MutationSelectionTests(unittest.TestCase):
    def test_source_tests_build_and_unrelated_paths(self):
        for paths, sources, windows in (
            (
                ["docs/example.md", "conf/README.md", "symlinks/config/quickshell/bar/Bar.qml"],
                [],
                False,
            ),
            (
                ["symlinks/config/hypr/conf/binds.lua", "symlinks/config/Code/User/settings.json"],
                [],
                False,
            ),
            (["cli/src/feature.rs"], ["cli/src/feature.rs"], True),
            (["cli/src/feature/tests.rs"], ["cli/src/feature", "cli/src/feature.rs"], True),
            (["cli/src/tests.rs"], ["cli/src"], True),
            (["cli/src/app/test_helpers/fixture.rs"], ["cli/src"], True),
            (["cli/src/domains/ai/apm/test_fixture.rs"], ["cli/src"], True),
            (["cli/tests/integration/update_command.rs"], ["cli/src"], True),
            (
                ["cli/src/domains/ai/apm/autopilot/tests/schedules.rs", "cli/src/feature.rs"],
                [
                    "cli/src/domains/ai/apm/autopilot",
                    "cli/src/domains/ai/apm/autopilot.rs",
                    "cli/src/feature.rs",
                ],
                True,
            ),
            (
                ["cli/src/domains/packages/arch_recipes/tests/releases.rs"],
                [
                    "cli/src/domains/packages/arch_recipes",
                    "cli/src/domains/packages/arch_recipes.rs",
                ],
                False,
            ),
            (["cli/tests/fixtures/apm-yaml-fragment.yaml"], ["cli/src/infra/yaml.rs"], True),
            (
                ["cli/tests/fixtures/apm-yaml-fragment.yaml", "cli/tests/common/mod.rs"],
                ["cli/src"],
                True,
            ),
            (["cli/src/example.json"], ["cli/src"], True),
            (["cli/Cargo.lock"], ["cli/src"], True),
            (["rust-toolchain.toml"], ["cli/src"], True),
            ([".github/workflows/mutation.yml"], ["cli/src"], True),
            ([SCRIPTS_PREFIX + "select-mutations.py"], ["cli/src"], True),
            ([SCRIPTS_PREFIX + "run-mutations.py"], ["cli/src"], True),
            (["conf/symlinks.toml"], ["cli/src/app/config"], True),
            (
                ["conf/system-files.toml"],
                [
                    "cli/src/app/config",
                    "cli/src/domains/system/resources/system_file",
                    "cli/src/domains/system/resources/system_file.rs",
                ],
                True,
            ),
            (
                ["packages/apm/release.json"],
                [
                    "cli/src/domains/packages/arch_recipes",
                    "cli/src/domains/packages/arch_recipes.rs",
                ],
                False,
            ),
            (
                ["packages/example/example.install"],
                [
                    "cli/src/domains/packages/arch_recipes",
                    "cli/src/domains/packages/arch_recipes.rs",
                ],
                False,
            ),
            (
                ["system/pam.d/login"],
                [
                    "cli/src/domains/system/resources/system_file",
                    "cli/src/domains/system/resources/system_file.rs",
                ],
                False,
            ),
            (["symlinks/config/git/templates/hooks/post-rewrite"], ["cli/src/domains/git"], True),
            (
                ["symlinks/config/systemd/user/dotfiles-first-login.service"],
                [
                    "cli/src/domains/system/resources/systemd_unit",
                    "cli/src/domains/system/resources/systemd_unit.rs",
                ],
                False,
            ),
            (
                ["docs/content/reference/home-symlinks.md"],
                ["cli/src/domains/files/symlinks.rs"],
                True,
            ),
            (
                ["docs/content/reference/systemd-units.md"],
                [
                    "cli/src/domains/system/resources/systemd_unit",
                    "cli/src/domains/system/resources/systemd_unit.rs",
                ],
                False,
            ),
            (
                ["cli/src/domains/system/resources/system_file/merge_pam.rs"],
                ["cli/src/domains/system/resources/system_file/merge_pam.rs"],
                False,
            ),
            (
                ["cli/src/domains/packages/arch_recipes.rs", "cli/src/domains/files/chmod.rs"],
                ["cli/src/domains/files/chmod.rs", "cli/src/domains/packages/arch_recipes.rs"],
                False,
            ),
            (
                ["cli/src/infra/fs/planned/linux.rs", "cli/src/domains/shell/login_shell.rs"],
                ["cli/src/domains/shell/login_shell.rs", "cli/src/infra/fs/planned/linux.rs"],
                False,
            ),
            (
                ["cli/src/domains/files/chmod.rs", "cli/src/infra/fs/place.rs"],
                ["cli/src/domains/files/chmod.rs", "cli/src/infra/fs/place.rs"],
                True,
            ),
        ):
            with self.subTest(paths=paths):
                self.assertEqual(MUTATION["selection"](paths), (sources, windows))

    def test_shell_completions_selects_windows_mutations_while_login_shell_skips(self):
        for path, expected_windows in (
            ("cli/src/domains/shell/completions.rs", True),
            ("cli/src/domains/shell/mod.rs", True),
            ("cli/src/domains/shell/login_shell.rs", False),
            ("cli/src/domains/shell/resources/shell.rs", False),
        ):
            with self.subTest(path=path):
                self.assertEqual(MUTATION["selection"]([path]), ([path], expected_windows))

    def test_only_successful_schedules_advance_the_baseline(self):
        head, base = "a" * 40, "b" * 40
        api = Mock(return_value=[{"head_sha": base}])
        with patch.dict(
            MUTATION["baseline"].__globals__,
            {
                "api_pages": api,
                "ancestor": lambda sha, tip: True,
            },
        ):
            self.assertEqual(MUTATION["baseline"]("fixture/repository", head), base)
        self.assertEqual(api.call_count, 1)
        self.assertIn("event=schedule&status=success", api.call_args.args[0])

    def test_bootstrap_requires_all_legacy_shards_and_reachable_history(self):
        head, unchecked, passed, unrelated = (letter * 40 for letter in "abcd")
        jobs = [
            {"name": f"Rust: Mutation (changed code, shard {shard})", "conclusion": "success"}
            for shard in range(8)
        ] + [
            {"name": f"Rust: Native Windows mutation (shard {shard})", "conclusion": "success"}
            for shard in range(2)
        ]
        api = Mock(
            side_effect=[
                [{"head_sha": unrelated}],
                [{"head_sha": unchecked, "id": 1}, {"head_sha": passed, "id": 2}],
                [],
                jobs,
            ]
        )
        with patch.dict(
            MUTATION["baseline"].__globals__,
            {
                "api_pages": api,
                "ancestor": lambda sha, tip: sha != unrelated,
            },
        ):
            self.assertEqual(MUTATION["baseline"]("fixture/repository", head), passed)
        api = Mock(side_effect=[[], [], []])
        with patch.dict(MUTATION["baseline"].__globals__, {"api_pages": api}):
            self.assertEqual(MUTATION["baseline"]("fixture/repository", head), "")

    def test_api_failure_is_not_an_empty_success(self):
        with (
            patch.dict(
                MUTATION["baseline"].__globals__,
                {
                    "api_pages": Mock(side_effect=subprocess.CalledProcessError(1, ["gh"])),
                },
            ),
            self.assertRaises(subprocess.CalledProcessError),
        ):
            MUTATION["baseline"]("fixture/repository", "a" * 40)

    def test_gh_api_retries_transient_server_errors_only(self):
        gh_api = MUTATION["gh_api"]
        sleeps = []
        transient = subprocess.CompletedProcess(
            ["gh"], 1, stdout="", stderr="gh: Server Error (HTTP 502)\n"
        )
        ok = subprocess.CompletedProcess(["gh"], 0, stdout='{"jobs":[]}', stderr="")
        with patch("subprocess.run", side_effect=[transient, ok]):
            self.assertEqual(gh_api("endpoint", sleep=sleeps.append), '{"jobs":[]}')
        self.assertEqual(sleeps, [1])

        sleeps.clear()
        not_found = subprocess.CompletedProcess(
            ["gh"], 1, stdout="", stderr="gh: Not Found (HTTP 404)\n"
        )
        with (
            patch("subprocess.run", side_effect=[not_found]) as run_mock,
            self.assertRaises(subprocess.CalledProcessError),
        ):
            gh_api("endpoint", sleep=sleeps.append)
        self.assertEqual(run_mock.call_count, 1)
        self.assertEqual(sleeps, [])


class DocumentationPublicationTests(unittest.TestCase):
    def setUp(self):
        self.policy = runpy.run_path(str(SCRIPTS / "docs-publication-base.py"))

    def test_baseline_requires_a_successful_ancestor_deployment(self):
        sha = "a" * 40
        deployments = [
            {"id": 1, "sha": "b" * 40},
            {"id": 2, "sha": "c" * 40},
            {"id": 3, "sha": sha},
        ]
        for ancestry, expected in ((0, sha), (1, "")):
            with (
                self.subTest(ancestry=ancestry),
                patch.dict(
                    self.policy["publication_base"].__globals__,
                    api=Mock(
                        side_effect=[
                            deployments,
                            [{"state": "failure"}],
                            [{"state": "success", "environment_url": ""}],
                            [
                                {"state": "inactive"},
                                {"state": "success", "environment_url": "https://example.invalid"},
                            ],
                        ]
                    ),
                ),
                patch.object(subprocess, "run", return_value=Mock(returncode=ancestry)),
            ):
                self.assertEqual(self.policy["publication_base"]("fixture/repo", "head"), expected)

    def test_missing_or_unavailable_baseline_requests_a_build(self):
        for response in ([], subprocess.CalledProcessError(1, "gh"), ValueError("bad json")):
            with self.subTest(response=response), tempfile.TemporaryDirectory() as temporary:
                output = Path(temporary) / "output"
                api = (
                    Mock(side_effect=response)
                    if isinstance(response, Exception)
                    else Mock(return_value=response)
                )
                with (
                    patch.dict(self.policy["main"].__globals__, api=api),
                    patch.dict(
                        os.environ,
                        {
                            "GITHUB_REPOSITORY": "fixture/repo",
                            "GITHUB_SHA": "head",
                            "GITHUB_OUTPUT": str(output),
                        },
                    ),
                    contextlib.redirect_stdout(io.StringIO()),
                ):
                    self.policy["main"]()
                self.assertEqual(output.read_text(), "base_sha=\n")

    def test_pages_checks_the_exact_site_job_before_downloading(self):
        pages = CONTRACT["WORKFLOW"].with_name("pages.yml").read_text()
        selection = CONTRACT["job_blocks"](pages)["select-build"]
        script = selection.split("        run: |\n", 1)[1]
        script = "\n".join(line[10:] for line in script.splitlines())
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            gh = root / "gh"
            gh.write_text(
                "#!/bin/sh\nset -eu\n"
                'case "$*" in\n'
                '  *actions/runs/123/jobs*) printf "%s\\n" "$SITE_RESULT"; exit "$API_STATUS" ;;\n'
                '  *git/ref/heads/main*) printf "%s\\n" "$CURRENT_SHA" ;;\n'
                "  *) exit 90 ;;\nesac\n"
            )
            gh.chmod(0o755)
            for result, current, workflow_sha, api_status, expected, exit_code in (
                ("skipped", "head", "head", "0", "false", 0),
                ("success", "head", "head", "0", "true", 0),
                ("success", "newer", "head", "0", "false", 0),
                ("success", "head", "newer", "0", "false", 0),
                ("failure", "head", "head", "0", None, 1),
                ("cancelled", "head", "head", "0", None, 1),
                ("", "head", "head", "0", None, 1),
                ("success", "head", "head", "1", None, 1),
            ):
                with self.subTest(
                    result=result, current=current, workflow_sha=workflow_sha, api_status=api_status
                ):
                    output = root / "output"
                    output.unlink(missing_ok=True)
                    completed = subprocess.run(
                        ["sh", "-eu", "-c", script],
                        env=os.environ
                        | {
                            "PATH": str(root) + os.pathsep + os.environ["PATH"],
                            "SITE_RESULT": result,
                            "CURRENT_SHA": current,
                            "API_STATUS": api_status,
                            "GITHUB_REPOSITORY": "fixture/repo",
                            "CI_RUN_ID": "123",
                            "BUILD_SHA": "head",
                            "GITHUB_SHA": workflow_sha,
                            "GITHUB_OUTPUT": str(output),
                        },
                        capture_output=True,
                        text=True,
                    )
                    self.assertEqual(completed.returncode, exit_code, completed.stderr)
                    self.assertEqual(
                        output.read_text() if output.exists() else None,
                        f"current={expected}\n" if expected else None,
                    )


class ReleaseTagTests(unittest.TestCase):
    def test_remote_presence_absence_and_errors(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            mock = root / "git"
            mock.write_text(
                "#!/bin/sh\n"
                'printf "%s\\n" "$5" >> "$TAG_LOG"\n'
                '[ "$TAG_STATUS" != sequence ] || {\n'
                '  [ "$5" = refs/tags/v2026.01.01-1 ] && exit 0\n'
                "  exit 2\n"
                "}\n"
                'exit "$TAG_STATUS"\n',
                encoding="utf-8",
            )
            mock.chmod(0o755)
            env = os.environ | {
                "PATH": str(root) + os.pathsep + os.environ["PATH"],
                "TAG_LOG": str(root / "calls"),
            }
            for status, mode, expected in (
                ("0", "check", 1),
                ("2", "check", 0),
                ("128", "check", 128),
                ("1", "check", 1),
                ("2", "next", 0),
                ("sequence", "next", 0),
                ("128", "next", 128),
                ("1", "next", 1),
            ):
                with self.subTest(status=status, mode=mode):
                    (root / "calls").write_text("")
                    result = subprocess.run(
                        [
                            "sh",
                            str(SCRIPTS / "linux" / "release-tag.sh"),
                            mode,
                            "2026.01.01" if mode == "next" else "v2026.01.01-1",
                        ],
                        env=env | {"TAG_STATUS": status},
                        capture_output=True,
                        text=True,
                    )
                    self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                    if mode == "next" and expected == 0:
                        self.assertEqual(
                            result.stdout.strip(),
                            "v2026.01.01-" + ("2" if status == "sequence" else "1"),
                        )
                    if status in {"128", "1"}:
                        self.assertIn("Cannot query release tag", result.stderr)
                        self.assertEqual(len((root / "calls").read_text().splitlines()), 1)
