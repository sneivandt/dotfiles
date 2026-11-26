"""Git comparison-range and catch-up regression tests."""

import contextlib
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from ci_changes_tests.helpers import LINUX, MUTATION, PRE_PUSH, SCRIPTS


class GitRangeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="dotfiles-ci-selection-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        home = self.root / "home"
        home.mkdir()
        gitconfig = self.root / "fixture-gitconfig"
        gitconfig.write_text(
            "[core]\n\tfsmonitor = false\n"
            "[maintenance]\n\tauto = false\n"
            "[gc]\n\tauto = 0\n",
            encoding="utf-8",
        )
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(
            HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
            GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=str(gitconfig), GIT_CONFIG_COUNT="0",
            GIT_CEILING_DIRECTORIES=str(self.root),
            GIT_AUTHOR_NAME="Fixture", GIT_AUTHOR_EMAIL="fixture@test.local",
            GIT_COMMITTER_NAME="Fixture", GIT_COMMITTER_EMAIL="fixture@test.local",
            DIR=str(self.repo), GITHUB_OUTPUT=str(self.root / "outputs"),
            GITHUB_EVENT_NAME="push", GITHUB_REPOSITORY="fixture/repository",
        )
        # Imported helpers also spawn Git, without receiving self.env explicitly.
        environment = patch.dict(os.environ, self.env, clear=True)
        environment.start()
        self.addCleanup(environment.stop)
        # TMPDIR may be inside a checkout. Neither archive checks nor Git
        # initialization should discover that enclosing repository.
        working_directory = Path.cwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, working_directory)
        self.git("init", "-q", "--template=", "--initial-branch=fixture")
        self.git("config", "core.hooksPath", os.devnull)
        self.git("config", "core.autocrlf", "false")
        for name in ("ci_metadata.py", "classify-release.sh", "classify-ci-changes.sh", "classify-ci-changes.py"):
            target = self.repo / LINUX / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(SCRIPTS / name, target)
        self.write("cli/src/main.rs", "fn main() {}\n")
        self.base = self.commit()
        self.git("tag", "v2026.01.01-1")
        self.env["BASE_SHA"] = self.base
        self.env["HEAD_SHA"] = self.base

    def git(self, *args):
        git_dir = self.repo / ".git"
        env = self.env | {
            "GIT_DIR": str(git_dir),
            "GIT_COMMON_DIR": str(git_dir),
            "GIT_WORK_TREE": str(self.repo),
            "GIT_INDEX_FILE": str(git_dir / "index"),
        }
        result = subprocess.run(
            ["git", *args], cwd=self.repo, env=env,
            capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, f"git {args!r}: {result.stderr}")
        return result.stdout.strip()

    def write(self, path, text):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")

    def test_imported_helpers_use_the_isolated_git_environment(self):
        for key in ("GIT_CONFIG_PARAMETERS", "GIT_CONFIG_KEY_0",
                    "GIT_CONFIG_VALUE_0", "GIT_EXEC_PATH"):
            self.assertNotIn(key, os.environ)
        self.assertEqual(os.environ["GIT_CONFIG_GLOBAL"], self.env["GIT_CONFIG_GLOBAL"])
        self.assertEqual(
            PRE_PUSH["git"](self.repo, "config", "--global", "--get", "maintenance.auto"),
            "false",
        )
        self.assertEqual(
            Path(PRE_PUSH["git"](self.repo, "rev-parse", "--show-toplevel")),
            self.repo,
        )

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def script(self, name="classify-ci-changes.sh", *, success=True):
        output = Path(self.env["GITHUB_OUTPUT"])
        output.write_text("", encoding="utf-8")
        result = subprocess.run(
            ["sh", str(SCRIPTS / name)], env=self.env, cwd=self.repo,
            capture_output=True, text=True,
        )
        if not success:
            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertEqual(output.read_text(encoding="utf-8"), "")
            return {}
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return {
            key: json.loads(value)
            for key, value in (line.split("=", 1) for line in output.read_text(encoding="utf-8").splitlines())
        }

    def test_docs_push_carries_forward_unverified_rust_changes(self):
        self.write("cli/src/main.rs", "fn main() { println!(\"changed\"); }\n")
        before = self.commit()
        self.write("docs/example.md", "docs only\n")
        head = self.commit()

        tools = self.root / "tools"
        tools.mkdir()
        gh = tools / "gh"
        gh.write_text(
            '#!/bin/sh\n'
            'test "$1" = api && test "$4" = "repos/fixture/repository/actions/workflows/ci.yml/runs" || exit 2\n'
            'case "${10}" in\n'
            '  "head_sha=$PUSH_BEFORE") if [ "$FIXTURE_CI" = success ]; then echo "$PUSH_BEFORE"; fi;;\n'
            '  status=success)\n'
            '    if [ "$FIXTURE_CI" = unavailable ]; then exit 1; fi\n'
            '    printf "%s\\n" "$GITHUB_SHA" "$FIXTURE_BASE";;\n'
            '  *) exit 2;;\n'
            'esac\n',
            encoding="utf-8",
        )
        gh.chmod(0o755)
        self.env.update(
            PATH=str(tools) + os.pathsep + self.env["PATH"],
            PUSH_BEFORE=before, GITHUB_SHA=head, FIXTURE_BASE=self.base,
        )
        for state in ("success", "cancelled", "failed", "in_progress", "unavailable", "initial"):
            with self.subTest(state=state):
                self.env.update(FIXTURE_CI=state, FIXTURE_BASE="" if state == "initial" else self.base)
                if state == "unavailable":
                    self.script("ci-comparison-range.sh", success=False)
                    continue
                output = Path(self.env["GITHUB_OUTPUT"])
                output.write_text("", encoding="utf-8")
                result = subprocess.run(
                    ["sh", str(SCRIPTS / "ci-comparison-range.sh")],
                    env=self.env, cwd=self.repo, check=True, capture_output=True, text=True,
                )
                values = dict(
                    line.split("=", 1) for line in output.read_text(encoding="utf-8").splitlines()
                )
                expected = {"success": before, "initial": self.git("hash-object", "-t", "tree", "/dev/null")}.get(state, self.base)
                self.assertEqual((values["base_sha"], values["head_sha"]), (expected, head))
                self.env.update(BASE_SHA=values["base_sha"], HEAD_SHA=values["head_sha"],
                                CI_FORCE_FULL=values["force_full"])
                jobs = self.script()
                self.assertEqual(jobs["docs_only"], state == "success", result.stderr)
                self.assertEqual(jobs["run_build_linux"], state != "success")
                self.assertEqual(jobs["run_build_windows"], state != "success")

    def test_manual_ci_requests_full_checks_without_mutation_inputs(self):
        self.write("cli/src/main.rs", 'fn main() { println!("changed"); }\n')
        head = self.commit()
        self.env.update(GITHUB_EVENT_NAME="workflow_dispatch", GITHUB_SHA=head)
        output = Path(self.env["GITHUB_OUTPUT"])
        output.write_text("", encoding="utf-8")
        subprocess.run(
            ["sh", str(SCRIPTS / "ci-comparison-range.sh")],
            env=self.env, cwd=self.repo, check=True, capture_output=True, text=True,
        )
        values = dict(
            line.split("=", 1) for line in output.read_text(encoding="utf-8").splitlines()
        )
        self.assertEqual((values["base_sha"], values["head_sha"]), ("", ""))
        self.assertEqual(values["force_full"], "true")
        self.env.update(BASE_SHA=values["base_sha"], HEAD_SHA=values["head_sha"],
                        CI_FORCE_FULL=values["force_full"])
        jobs = self.script()
        self.assertNotIn("run_mutation", jobs)
        self.assertTrue(jobs["run_build_windows"])

    def test_mutation_diff_includes_production_after_inline_test_only_edits(self):
        self.write("cli/src/main.rs", "fn original() -> bool { true }\n#[cfg(test)] mod tests {}\n")
        base = self.commit()
        self.write("cli/src/main.rs", "fn original() -> bool { true }\n#[cfg(test)] mod tests { /* revised */ }\n")
        head = self.commit()
        summary = self.root / "summary"
        with contextlib.chdir(self.repo), patch.dict(os.environ, self.env | {
            "GITHUB_SHA": head, "MUTATION_BASE": base, "GITHUB_STEP_SUMMARY": str(summary),
        }):
            MUTATION["main"]([])
        diff = (self.repo / "cli/mutants.diff").read_text()
        self.assertIn("+fn original() -> bool { true }", diff)
        self.assertIn("has_linux_inputs=true", Path(self.env["GITHUB_OUTPUT"]).read_text())
        self.assertIn(f"{base}..{head}", summary.read_text())
        manifest = (self.repo / "cli/mutation-plan.json").read_bytes()
        with contextlib.chdir(self.repo):
            MUTATION["main"](["--base", base])
            self.assertEqual((self.repo / "cli/mutants.diff").read_text(), diff)
            self.assertEqual((self.repo / "cli/mutation-plan.json").read_bytes(), manifest)
            with self.assertRaises(ValueError):
                MUTATION["baseline"]("fixture/repository", head, "HEAD")
            with self.assertRaises(ValueError):
                MUTATION["baseline"]("fixture/repository", base, head)

    def test_mutation_deletion_rename_and_empty_selection(self):
        self.write("cli/src/feature.rs", "fn feature() -> bool { true }\n")
        base = self.commit()
        self.git("mv", "cli/src/feature.rs", "cli/src/moved.rs")
        head = self.commit()
        with contextlib.chdir(self.repo), patch.dict(os.environ, self.env | {
            "GITHUB_SHA": head, "MUTATION_BASE": base,
            "GITHUB_STEP_SUMMARY": str(self.root / "summary"),
        }):
            MUTATION["main"]([])
        diff = (self.repo / "cli/mutants.diff").read_text()
        self.assertIn("src/moved.rs", diff)
        self.assertNotIn("src/feature.rs", diff)
        with contextlib.chdir(self.repo), patch.dict(os.environ, self.env | {
            "GITHUB_SHA": head, "MUTATION_BASE": head,
            "GITHUB_STEP_SUMMARY": str(self.root / "summary"),
        }):
            MUTATION["main"]([])
        self.assertEqual((self.repo / "cli/mutants.diff").read_bytes(), b"")
        self.assertTrue(Path(self.env["GITHUB_OUTPUT"]).read_text().endswith(
            "has_linux_inputs=false\nrun_windows=false\n",
        ))

    def test_rename_deletion_and_unusual_paths(self):
        self.git("mv", "cli/src/main.rs", "example.md")
        self.env["HEAD_SHA"] = self.commit()
        outputs = self.script()
        for name in ("rust_checks", "build_linux", "build_windows", "profile_linux", "release"):
            self.assertTrue(outputs["run_" + name])
        self.assertFalse(outputs["docs_only"])
        self.env["BASE_SHA"] = self.env["HEAD_SHA"]
        self.write("docs/space and\nnewline.md", "docs\n")
        self.env["HEAD_SHA"] = self.commit()
        self.assertTrue(self.script()["docs_only"])
        self.env["BASE_SHA"] = self.env["HEAD_SHA"]
        self.write("cli/src/space and\nnewline.rs", "fn fixture() {}\n")
        self.env["HEAD_SHA"] = self.commit()
        self.assertTrue(self.script()["run_release"])
        self.env["BASE_SHA"] = self.env["HEAD_SHA"]
        self.git("rm", "cli/src/space and\nnewline.rs")
        self.env["HEAD_SHA"] = self.commit()
        self.assertTrue(self.script()["run_release"])

    def test_pre_push_snapshot_excludes_local_fixes_and_preserves_modes(self):
        self.write("script.sh", "#!/bin/sh\nexit 1\n")
        (self.repo / "script.sh").chmod(0o755)
        head = self.commit()
        self.write("script.sh", "#!/bin/sh\nexit 0\n")
        self.write("untracked.txt", "not committed\n")
        destination = self.root / "snapshot"
        PRE_PUSH["snapshot"](self.repo, head, destination)
        self.assertEqual((destination / "script.sh").read_text(), "#!/bin/sh\nexit 1\n")
        self.assertFalse((destination / "untracked.txt").exists())
        if os.name != "nt":
            self.assertTrue(os.access(destination / "script.sh", os.X_OK))
        self.assertEqual(self.git("rev-parse", "HEAD"), head)
        self.assertEqual((self.repo / "script.sh").read_text(), "#!/bin/sh\nexit 0\n")

    def test_pre_push_uses_committed_failure_and_rejects_invalid_range(self):
        shutil.copyfile(SCRIPTS / "pre-push.py", self.repo / LINUX / "pre-push.py")
        self.write("cli/Cargo.toml", '[package]\nrust-version = "1.91"\n')
        self.write("docs/package.json", '{"private":true}\n')
        self.write(LINUX + "check.py", "import sys\nsys.exit(17)\n")
        tools = self.root / "tools"
        tools.mkdir()
        npm = tools / "npm"
        npm.write_text('#!/bin/sh\n[ "$*" = "ci --no-audit --no-fund" ] || exit 92\n')
        npm.chmod(0o755)
        self.env["PATH"] = str(tools) + os.pathsep + self.env["PATH"]
        base = self.commit()
        self.write("README.md", "changed\n")
        head = self.commit()
        # The index is empty and the working-tree fix must not hide failure.
        self.write(LINUX + "check.py", "import sys\nsys.exit(0)\n")
        command = ["python3", "-B", str(self.repo / LINUX / "pre-push.py")]
        for arguments, expected, message in (
            (["--base", base, "--head", head], 1, "exit status 17"),
            (["--base", base, "--head", head, "--docs-checked", head], 1, "exit status 17"),
            (["--base", base, "--head", head, "--docs-checked", base], 1, "exact commit"),
            (["--base", head, "--head", head], 0, "no checks run"),
            (["--base", "invalid-ref"], 1, "Pre-push validation failed"),
        ):
            result = subprocess.run(command + arguments, env=self.env, capture_output=True, text=True)
            self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
            self.assertIn(message, result.stdout + result.stderr)

    def test_pre_push_selection_ignores_dirty_or_missing_local_policy(self):
        shutil.copyfile(SCRIPTS / "pre-push.py", self.repo / LINUX / "pre-push.py")
        self.write("cli/Cargo.toml", '[package]\nrust-version = "1.91"\n')
        base = self.commit()
        self.write("cli/src/main.rs", "invalid rust\n")
        head = self.commit()
        for target_name in ("classify-ci-changes.py", "ci_metadata.py"):
            target = self.repo / LINUX / target_name
            original = target.read_text(encoding="utf-8")
            for content in ("raise RuntimeError('dirty policy executed')\n", None):
                with self.subTest(target=target_name, content=content):
                    if content is None:
                        target.unlink()
                    else:
                        target.write_text(content, encoding="utf-8")
                    result = subprocess.run(
                        ["python3", "-B", str(self.repo / LINUX / "pre-push.py"),
                         "--plan", "--base", base, "--head", head],
                        env=self.env, capture_output=True, text=True,
                    )
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertIn("clippy", result.stdout)
                    self.assertIn("x86_64-pc-windows-gnu", result.stdout)
            target.write_text(original, encoding="utf-8")

    def test_doc_links_preserve_paths_and_validate_angle_destinations(self):
        helper = runpy.run_path(str(SCRIPTS / "check-doc-links.py"))
        target = "docs/target with spaces.md"
        self.write(target, "# Heading\n# Heading\n```\n# Hidden\n```\n")
        self.write("docs/source with spaces.md", "\n".join((
            "[angle](<target with spaces.md#heading>)",
            "[encoded](target%20with%20spaces.md#heading-1)",
            "[external](<https://example.invalid/a b>)",
            "[mail](mailto:fixture%40example.invalid)",
        )))
        # A quoted Git display pathname must also be preserved on POSIX.
        if os.name != "nt":
            self.write("docs/tab\tand\nnewline.md", "[valid](<target with spaces.md>)\n")
        self.git("add", ".")
        self.assertEqual(helper["check"](self.repo), [])
        self.write("docs/source with spaces.md", "\n".join((
            "[missing](<missing with spaces.md>)",
            "[anchor](<target with spaces.md#absent>)",
            "[fence](<target with spaces.md#hidden>)",
        )))
        failures = helper["check"](self.repo)
        self.assertEqual(len(failures), 3, failures)
        self.assertIn("broken link", failures[0])
        self.assertIn("broken anchor", failures[1])
        # Archive/source snapshots without Git metadata use the same checks.
        archive = self.root / "archive"
        archive.mkdir()
        (archive / "source with spaces.md").write_text("[missing](<missing.md>)\n")
        self.assertEqual(len(helper["check"](archive)), 1)

    def test_doc_links_follow_unstaged_moves_and_ignore_generated_files(self):
        helper = runpy.run_path(str(SCRIPTS / "check-doc-links.py"))
        self.write("docs/guide.md", "# Guide\n")
        self.git("add", ".")
        (self.repo / "docs/guide.md").unlink()
        self.write("docs/content/guide.md", "[missing](missing.md)\n")
        self.write(".gitignore", "docs/node_modules/\n")
        self.write("docs/node_modules/generated.md", "[ignored](absent.md)\n")
        failures = helper["check"](self.repo)
        self.assertEqual(len(failures), 1, failures)
        self.assertIn("docs/content/guide.md: broken link", failures[0])

    def test_built_docs_preserve_routes_anchors_and_assets(self):
        check_site = runpy.run_path(str(SCRIPTS / "check-doc-site.py"))["check_site"]
        common_head = (
            '<link rel="icon" href="/favicon.png">'
            '<link rel="icon" href="/favicon.ico">'
            '<link rel="apple-touch-icon" href="/apple-touch-icon.png">'
            '<meta name="author" content="Stuart Neivandt">'
            '<meta name="theme-color" content="#1a1b26">'
            '<meta property="og:site_name" content="Dotfiles">'
        )
        social_head = (
            '<meta property="og:locale" content="en_US">'
            '<meta property="og:image" content="https://docs.example.invalid/assets/og-image.png">'
            '<meta name="twitter:card" content="summary_large_image">'
            '<meta name="twitter:image" content="https://docs.example.invalid/assets/og-image.png">'
        )
        homepage = (
            '<link rel="shortcut icon" href="/favicon.svg">'
            + common_head
            + social_head
            + '<link rel="canonical" href="https://docs.example.invalid/getting-started">'
            '<meta name="robots" content="noindex, follow">'
            '<meta property="og:type" content="article">'
            '<meta property="og:url" content="https://docs.example.invalid/getting-started">'
            '<h1 id="getting-started">Getting started</h1><h2 id="setup">Setup</h2>'
            '<nav id="VPSidebarNav"><a href="/commands-and-usage#commands">Commands and usage</a></nav>'
            '<a href="https://example.invalid/external">External</a>'
            '<img src="/assets/example.svg">'
        )
        fixture = {
            "docs/public/CNAME": "docs.example.invalid\n",
            "docs/content/404.md": "---\ntitle: Page not found\ndescription: Missing page.\n---\n",
            "docs/content/index.md": "---\ntitle: About these dotfiles\ndescription: Project introduction.\n---\n# About these dotfiles\n\n## Start here\n",
            "docs/content/guides/getting-started.md": "---\ntitle: Getting started\ndescription: Set up a machine.\n---\n# Getting started\n\n## Setup\n",
            "docs/content/guides/commands-and-usage.md": "---\ntitle: Commands and usage\ndescription: Everyday commands.\n---\n# Commands and usage\n\n## Commands\n",
            "docs/dist/index.html": (
                '<link rel="icon" href="/favicon.svg">'
                + common_head
                + social_head
                + '<link rel="canonical" href="https://docs.example.invalid/">'
                '<meta name="robots" content="index, follow, max-image-preview:large">'
                '<meta property="og:type" content="website">'
                '<meta property="og:url" content="https://docs.example.invalid/">'
                '<header><a class="title" href="/">Dotfiles</a></header>'
                '<h1 id="about-these-dotfiles">About these dotfiles</h1><h2 id="start-here">Start here</h2>'
                '<nav id="VPSidebarNav">'
                '<a href="/getting-started">Getting started</a>'
                '<a href="/commands-and-usage">Commands and usage</a>'
                '</nav>'
            ),
            "docs/dist/getting-started.html": homepage,
            "docs/dist/commands-and-usage.html": (
                '<link rel="icon" href="/favicon.svg">'
                + common_head
                + social_head
                + '<link rel="canonical" href="https://docs.example.invalid/commands-and-usage">'
                '<meta name="robots" content="noindex, follow">'
                '<meta property="og:type" content="article">'
                '<meta property="og:url" content="https://docs.example.invalid/commands-and-usage">'
                '<h1 id="commands-and-usage">Commands and usage</h1><h2 id="commands">Commands</h2>'
                '<a href="/getting-started#setup">Setup</a>'
            ),
            "docs/dist/404.html": (
                '<link rel="icon" href="/favicon.svg">'
                + common_head
                + '<meta name="robots" content="noindex, nofollow"><h1>Page not found</h1>'
            ),
            "docs/dist/favicon.svg": "<svg/>",
            "docs/dist/favicon.png": "png",
            "docs/dist/favicon.ico": "ico",
            "docs/dist/apple-touch-icon.png": "png",
            "docs/dist/assets/og-image.png": "png",
            "docs/dist/assets/example.svg": "<svg/>",
            "docs/dist/CNAME": "docs.example.invalid\n",
            "docs/dist/robots.txt": "User-agent: *\nAllow: /\nSitemap: https://docs.example.invalid/sitemap-index.xml\n",
            "docs/dist/sitemap-index.xml": (
                '<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">'
                '<sitemap><loc>https://docs.example.invalid/sitemap-0.xml</loc></sitemap></sitemapindex>'
            ),
            "docs/dist/sitemap-0.xml": (
                '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">'
                '<url><loc>https://docs.example.invalid/</loc></url>'
                '</urlset>'
            ),
        }
        descriptions = {
            "index": "Project introduction.",
            "getting-started": "Set up a machine.",
            "commands-and-usage": "Everyday commands.",
        }
        for name, description in descriptions.items():
            fixture[f"docs/dist/{name}.html"] += f'<meta name="description" content="{description}">'
        homepage = fixture["docs/dist/getting-started.html"]
        for path, content in fixture.items():
            self.write(path, content)
        self.assertEqual(check_site(self.repo), [])
        cases = [
            ("docs/dist/index.html", None, "Missing built homepage"),
            ("docs/content/index.md", None, "Missing Markdown homepage"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('id="about-these-dotfiles"', 'id="wrong"'), "missing heading anchors"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('<h1 id="about-these-dotfiles">About these dotfiles</h1>', '<h1 id="about-these-dotfiles">Welcome</h1>'), "Markdown title does not match page title"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('class="title"', 'class="other"'), "Homepage header must link"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('href="https://docs.example.invalid/"', 'href="https://docs.example.invalid/index.html"'), "expected canonical URL"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('content="index, follow, max-image-preview:large"', 'content="index, noindex"'), "landing page must allow indexing and following"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace("<h1", '<meta name="googlebot" content="none"><h1'), "landing page must allow indexing and following"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('<meta property="og:type" content="website">', '<meta property="og:type" content="article">'), "expected og:type"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('<nav id="VPSidebarNav">', '<nav id="VPSidebarNav"><a href="/index.html">Dotfiles</a>'), "Homepage must not appear in sidebar navigation"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('href="/getting-started"', 'href="/commands-and-usage"'), "missing from sidebar navigation"),
            ("docs/dist/commands-and-usage.html", None, "missing published route /commands-and-usage.html"),
            ("docs/dist/getting-started.html", homepage.replace('id="setup"', 'id="renamed"'), "missing heading anchors"),
            ("docs/dist/getting-started.html", homepage + "<h1>Extra title</h1>", "expected one page title"),
            ("docs/dist/getting-started.html", homepage + '<span id="setup"></span>', "duplicate IDs"),
            ("docs/dist/getting-started.html", homepage.replace("shortcut icon", "stylesheet"), "missing site icon /favicon.svg"),
            ("docs/dist/getting-started.html", homepage.replace('<link rel="apple-touch-icon" href="/apple-touch-icon.png">', ""), "missing apple-touch-icon"),
            ("docs/dist/getting-started.html", homepage.replace('<meta name="author" content="Stuart Neivandt">', ""), "expected author"),
            ("docs/dist/getting-started.html", homepage.replace('<meta name="theme-color" content="#1a1b26">', ""), "expected theme-color"),
            ("docs/dist/getting-started.html", homepage.replace('<meta property="og:site_name" content="Dotfiles">', ""), "expected og:site_name"),
            ("docs/dist/getting-started.html", homepage.replace('<meta property="og:image" content="https://docs.example.invalid/assets/og-image.png">', ""), "expected og:image"),
            ("docs/dist/getting-started.html", homepage.replace('<meta name="twitter:card" content="summary_large_image">', ""), "expected twitter:card"),
            ("docs/dist/getting-started.html", homepage.replace('content="https://docs.example.invalid/getting-started"', 'content="https://docs.example.invalid/getting-started.html"'), "expected og:url"),
            ("docs/dist/getting-started.html", homepage.replace('content="noindex, follow"', 'content="index, follow"'), "non-home guide must emit noindex, follow"),
            ("docs/dist/getting-started.html", homepage.replace('content="noindex, follow"', 'content="noindex, nofollow"'), "non-home guide must emit noindex, follow"),
            ("docs/dist/404.html", fixture["docs/dist/404.html"].replace('content="noindex, nofollow"', 'content="index, follow"'), "expected noindex, nofollow robots meta tag"),
            ("docs/dist/getting-started.html", homepage.replace("<h1", '<link rel="canonical" href="https://docs.example.invalid/getting-started.html"><h1'), "expected canonical URL"),
            ("docs/dist/getting-started.html", homepage.replace("#commands", "#absent"), "broken anchor"),
            ("docs/dist/getting-started.html", homepage.replace("commands-and-usage#commands", "commands-and-usage.html#commands"), "expected clean URL without .html"),
            ("docs/dist/getting-started.html", homepage.replace("commands-and-usage#commands", "commands-and-usage.md#commands"), "broken destination"),
            ("docs/dist/getting-started.html", homepage.replace(">Getting started<", ">Welcome<"), "filename and URL do not match title"),
            ("docs/content/guides/getting-started.md", "# Welcome\n\n## Setup\n", "Markdown title does not match page title"),
            ("docs/content/guides/getting-started.md", fixture["docs/content/guides/getting-started.md"].replace("title: Getting started", "title: Welcome"), "frontmatter title does not match"),
            ("docs/content/guides/getting-started.md", fixture["docs/content/guides/getting-started.md"].replace("description: Set up a machine.", "description: ''"), "nonempty single-line frontmatter description"),
            ("docs/dist/getting-started.html", homepage.replace("Set up a machine.", "Outdated description."), "rendered description does not match"),
            ("docs/dist/getting-started.html", homepage.replace(">Commands and usage<", ">Wrong label<"), "navigation label"),
            ("docs/dist/favicon.svg", None, "broken destination /favicon.svg"),
            ("docs/dist/assets/og-image.png", None, "broken destination https://docs.example.invalid/assets/og-image.png"),
            ("docs/dist/assets/example.svg", None, "broken destination /assets/example.svg"),
            ("docs/dist/CNAME", "wrong.example.invalid\n", "published CNAME"),
            ("docs/dist/robots.txt", None, "Missing published robots.txt"),
            ("docs/dist/robots.txt", "User-agent: *\nAllow: /\n", "does not declare the canonical sitemap"),
            ("docs/dist/sitemap-index.xml", None, "Missing published sitemap-index.xml"),
            ("docs/dist/sitemap-0.xml", None, "Missing published sitemap"),
            ("docs/dist/sitemap-0.xml", fixture["docs/dist/sitemap-0.xml"].replace('invalid/</loc>', 'invalid/index.html</loc>'), "Sitemap URLs differ from indexable landing URL"),
            ("docs/dist/sitemap-0.xml", fixture["docs/dist/sitemap-0.xml"].replace("</urlset>", "<url><loc>https://docs.example.invalid/getting-started</loc></url></urlset>"), "Sitemap URLs differ from indexable landing URL"),
        ]
        for path, content, message in cases:
            with self.subTest(path=path, message=message):
                if content is None:
                    (self.repo / path).unlink()
                else:
                    self.write(path, content)
                self.assertIn(message, "\n".join(check_site(self.repo)))
                self.write(path, fixture[path])
        self.write("docs/content/reference/commands-and-usage.md",
                   fixture["docs/content/guides/commands-and-usage.md"])
        self.assertIn("source filenames must be unique", "\n".join(check_site(self.repo)))
        (self.repo / "docs/content/reference/commands-and-usage.md").unlink()
        self.write("docs/dist/guides/getting-started.html", homepage)
        self.assertIn("Published routes differ from flat source routes", "\n".join(check_site(self.repo)))

    def test_pending_site_publication_survives_a_successful_code_push(self):
        self.write("docs/content/example.md", "# Unpublished guide\n")
        previous = self.commit()
        self.env["BASE_SHA"] = previous
        self.write("cli/src/main.rs", "fn main() { println!(\"code-only push\"); }\n")
        self.env["HEAD_SHA"] = self.commit()
        self.env["DOCS_BASE_SHA"] = self.base
        self.assertTrue(self.script()["run_docs_site"])
        self.env["DOCS_BASE_SHA"] = previous
        self.assertFalse(self.script()["run_docs_site"])
        self.env.pop("DOCS_BASE_SHA")
        self.assertTrue(self.script()["run_docs_site"])
        self.env["DOCS_BASE_SHA"] = "invalid-baseline"
        self.script(success=False)

    def test_removed_and_renamed_site_inputs_select_a_build(self):
        self.write("docs/content/old.md", "# Guide\n")
        published = self.commit()
        self.env.update(BASE_SHA=published, DOCS_BASE_SHA=published)
        self.git("mv", "docs/content/old.md", "guide.md")
        self.env["HEAD_SHA"] = self.commit()
        self.assertTrue(self.script()["run_docs_site"])
        self.assertTrue(self.script()["run_docs_contracts"])

    def test_empty_manual_missing_and_invalid_ranges(self):
        self.assertFalse(self.script()["run_build_linux"])
        self.env["GITHUB_EVENT_NAME"] = "workflow_dispatch"
        self.assertTrue(self.script()["run_build_windows"])
        self.env["GITHUB_EVENT_NAME"] = "push"
        self.env.pop("BASE_SHA")
        self.assertTrue(self.script()["run_build_windows"])
        self.env["BASE_SHA"] = "invalid-comparison"
        self.script(success=False)

    def mock_release(self, tag="v2026.01.01-1", status=0):
        mock = self.root / "bin"
        mock.mkdir(exist_ok=True)
        gh = mock / "gh"
        gh.write_text(
            '#!/bin/sh\nset -eu\n'
            'printf "called\\n" >> "$FIXTURE_GH_LOG"\n'
            '[ "$#" -eq 12 ] && [ "$1" = release ] && [ "$2" = list ] &&\n'
            '[ "$3" = --repo ] && [ "$4" = fixture/repository ] &&\n'
            '[ "$5" = --exclude-drafts ] && [ "$6" = --exclude-pre-releases ] &&\n'
            '[ "$7" = --limit ] && [ "$8" = 1 ] && [ "$9" = --json ] &&\n'
            '[ "${10}" = tagName ] && [ "${11}" = --jq ] &&\n'
            '[ "${12}" = ".[0].tagName // empty" ] || exit 90\n'
            'printf "%s\\n" "$FIXTURE_RELEASE_TAG"\nexit "$FIXTURE_GH_STATUS"\n',
            encoding="utf-8",
        )
        gh.chmod(0o755)
        self.env.update(
            PATH=str(mock) + os.pathsep + self.env["PATH"],
            FIXTURE_RELEASE_TAG=tag, FIXTURE_GH_STATUS=str(status),
            FIXTURE_GH_LOG=str(self.root / "gh-calls"),
        )

    def test_release_uses_net_changes_since_an_annotated_published_tag(self):
        self.git("tag", "-a", "-m", "published fixture", "v2026.01.01-2", self.base)
        self.mock_release(tag="v2026.01.01-2")
        self.write("cli/src/main.rs", "fn main() { println!(\"unpublished\"); }\n")
        self.commit()
        self.write("cli/src/main.rs", "fn main() {}\n")
        self.write("README.md", "binary changes were reverted before publishing\n")
        self.env["HEAD_SHA"] = self.commit()
        self.assertFalse(self.script("classify-release.sh")["run_release"])

    def test_failed_empty_release_lookup_is_not_an_initial_release(self):
        self.mock_release(tag="", status=1)
        self.script("classify-release.sh", success=False)
        self.assertEqual((self.root / "gh-calls").read_text(), "called\n")

    def test_invalid_release_head_is_rejected_before_api_lookup(self):
        self.mock_release(tag="")
        self.env["HEAD_SHA"] = "invalid-head"
        self.script("classify-release.sh", success=False)
        self.assertFalse((self.root / "gh-calls").exists())

    def test_release_docs_skip_and_unpublished_binary_catch_up(self):
        self.mock_release()
        self.write("README.md", "docs\n")
        self.env["HEAD_SHA"] = self.commit()
        self.assertFalse(self.script("classify-release.sh")["run_release"])
        self.write("cli/src/main.rs", "fn main() { println!(\"updated\"); }\n")
        unpublished = self.commit()
        self.env["BASE_SHA"] = unpublished
        self.write("README.md", "more docs\n")
        self.env["HEAD_SHA"] = self.commit()
        self.assertFalse(self.script()["run_release"])
        self.assertTrue(self.script("classify-release.sh")["run_release"])
        self.git("tag", "v2026.01.01-2", unpublished)
        self.env["FIXTURE_RELEASE_TAG"] = "v2026.01.01-2"
        self.assertFalse(self.script("classify-release.sh")["run_release"])

    def test_initial_release_rerun_stale_and_api_failure(self):
        self.mock_release(tag="")
        self.assertTrue(self.script("classify-release.sh")["run_release"])
        self.env["FIXTURE_RELEASE_TAG"] = "v2026.01.01-1"
        self.assertFalse(self.script("classify-release.sh")["run_release"])
        self.write("cli/src/main.rs", "updated\n")
        newer = self.commit()
        self.git("tag", "v2026.01.01-2", newer)
        self.env["FIXTURE_RELEASE_TAG"] = "v2026.01.01-2"
        self.assertFalse(self.script("classify-release.sh")["run_release"])
        self.env["FIXTURE_GH_STATUS"] = "1"
        self.script("classify-release.sh", success=False)

    def test_missing_tag_and_divergent_history_fail(self):
        self.mock_release(tag="v2026.01.01-99")
        self.script("classify-release.sh", success=False)
        self.git("checkout", "--orphan", "unrelated")
        self.git("commit", "-qm", "unrelated")
        self.env["HEAD_SHA"] = self.git("rev-parse", "HEAD")
        self.env["FIXTURE_RELEASE_TAG"] = "v2026.01.01-1"
        self.script("classify-release.sh", success=False)
