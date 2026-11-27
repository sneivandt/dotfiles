"""Build-mode, argument-forwarding, runtime-context, and Cargo artifact tests for dotfiles.sh."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import tempfile
import unittest

from .helpers import REPO_ROOT, copy_wrapper, isolated_mode, run, write_executable

_FAKE_CARGO_SIMPLE = """#!/bin/sh
printf '{"reason":"compiler-artifact","target":{"name":"dotfiles"},"executable":"%s/cli/target/dev-opt/dotfiles"}\\n' "$DOTFILES_ROOT"
exit 0
"""


class WrapperArgsAndBuildTests(unittest.TestCase):
    def test_wrapper_build_mode(self) -> None:
        if isolated_mode():
            self.skipTest("skipped in --isolated mode")
        binary = os.environ.get("BINARY_PATH", "")
        if binary and os.access(binary, os.X_OK):
            self.skipTest("pre-built binary available; build tested separately")
        if shutil.which("cargo") is None:
            self.skipTest("cargo not installed")

        res = run([str(REPO_ROOT / "dotfiles.sh"), "--build", "--version"], cwd=REPO_ROOT)
        combined = res.stdout + res.stderr
        self.assertIn("dotfiles", combined, f"--build mode failed:\n{combined}")

    def test_wrapper_uses_local_binary(self) -> None:
        binary = os.environ.get("BINARY_PATH", "")
        if not binary:
            self.skipTest("BINARY_PATH not set")
        binary_path = Path(binary)
        self.assertTrue(binary_path.is_file(), f"Binary not found at {binary}")
        res = run([str(binary_path), "--version"], cwd=REPO_ROOT)
        self.assertEqual(res.returncode, 0, f"Binary cannot execute --version:\n{res.stderr}")

    def test_wrapper_forwarded_args(self) -> None:
        binary = os.environ.get("BINARY_PATH", "")
        if not binary:
            self.skipTest("BINARY_PATH not set")
        with tempfile.TemporaryDirectory(prefix="wrapper-fwd-") as tmp_str:
            tmp = Path(tmp_str)
            wrapper = copy_wrapper(tmp)
            bin_dir = tmp / "bin"
            bin_dir.mkdir()
            dst = bin_dir / "dotfiles"
            shutil.copy2(binary, dst)
            dst.chmod(0o755)

            res = run([str(wrapper), "--version"], cwd=tmp)
            self.assertEqual(res.returncode, 0, f"Argument forwarding failed:\n{res.stderr}")

    def test_wrapper_build_mode_consumes_build_flag_and_forwards_cli_args(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wrapper-build-args-") as tmp_str:
            tmp = Path(tmp_str)
            wrapper = copy_wrapper(tmp)
            write_executable(tmp / "fake-bin" / "cargo", _FAKE_CARGO_SIMPLE)
            write_executable(
                tmp / "cli" / "target" / "dev-opt" / "dotfiles",
                '#!/bin/sh\nprintf \'%s\\n\' "$@" > "$DOTFILES_ROOT/forwarded-args.txt"\n',
            )
            env = dict(os.environ, PATH=f"{tmp / 'fake-bin'}:{os.environ.get('PATH', '')}")
            res = run(
                [str(wrapper), "--build", "update", "-p", "desktop", "-n", "-v"], cwd=tmp, env=env
            )
            self.assertEqual(res.returncode, 0, res.stderr)
            self.assertEqual(
                (tmp / "forwarded-args.txt").read_text(encoding="utf-8").splitlines(),
                ["update", "-p", "desktop", "-n", "-v"],
            )

    def test_wrapper_forwards_advanced_flags(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wrapper-adv-args-") as tmp_str:
            tmp = Path(tmp_str)
            wrapper = copy_wrapper(tmp)
            write_executable(tmp / "fake-bin" / "cargo", _FAKE_CARGO_SIMPLE)
            write_executable(
                tmp / "cli" / "target" / "dev-opt" / "dotfiles",
                '#!/bin/sh\nprintf \'%s\\n\' "$@" > "$DOTFILES_ROOT/forwarded-advanced-args.txt"\n',
            )
            env = dict(os.environ, PATH=f"{tmp / 'fake-bin'}:{os.environ.get('PATH', '')}")
            res = run(
                [
                    str(wrapper),
                    "--build",
                    "update",
                    "--skip",
                    "symlinks",
                    "--only",
                    "packages",
                    "--no-parallel",
                ],
                cwd=tmp,
                env=env,
            )
            self.assertEqual(res.returncode, 0, res.stderr)
            self.assertEqual(
                (tmp / "forwarded-advanced-args.txt").read_text(encoding="utf-8").splitlines(),
                ["update", "--skip", "symlinks", "--only", "packages", "--no-parallel"],
            )

    def test_wrapper_preserves_runtime_context(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wrapper-context-") as tmp_str:
            tmp = Path(tmp_str).resolve()
            copy_wrapper(tmp)
            caller_dir = tmp / "caller dir"
            caller_dir.mkdir()

            write_executable(
                tmp / "fake-bin" / "cargo",
                "#!/bin/sh\n"
                'printf \'%s\\n\' "$PWD" > "$DOTFILES_ROOT/cargo-cwd"\n'
                'printf \'{"reason":"compiler-artifact","target":{"name":"dotfiles"},"executable":"%s/cli/target/dev-opt/dotfiles"}\\n\' "$DOTFILES_ROOT"\n'
                'exit "${WRAPPER_CARGO_EXIT:-0}"\n',
            )
            child_script = (
                "#!/bin/sh\n"
                'printf \'%s\\n\' "$PWD" > "$DOTFILES_ROOT/child-cwd"\n'
                'printf \'%s\\n\' "$DOTFILES_ROOT" "$DOTFILES_WRAPPER" "$DOTFILES_REEXEC_GUARD" > "$DOTFILES_ROOT/child-context"\n'
                'printf \'%s\\n\' "$#" > "$DOTFILES_ROOT/child-argc"\n'
                ': > "$DOTFILES_ROOT/child-args"\n'
                "for argument do\n"
                '  printf \'%s\\0\' "$argument" >> "$DOTFILES_ROOT/child-args"\n'
                "done\n"
                "printf 'child stdout\\n'\n"
                "printf 'child stderr\\n' >&2\n"
                "exit 7\n"
            )
            write_executable(tmp / "bin" / "dotfiles", child_script)
            write_executable(tmp / "cli" / "target" / "dev-opt" / "dotfiles", child_script)

            wrapper_link = caller_dir / "wrapper link"
            wrapper_link.symlink_to("../dotfiles.sh")

            env = dict(
                os.environ,
                PATH=f"{tmp / 'fake-bin'}:{os.environ.get('PATH', '')}",
                DOTFILES_REEXEC_GUARD="1",
            )
            complex_args = [
                "update",
                "--dry-run",
                "--root",
                ".",
                "--overlay",
                "../overlay dir",
                "",
                "--BUILD",
                "--",
                "literal value",
                "--build",
                "*?[literal]",
                "quote\"and apostrophe'",
                "semi;colon",
                "back\\slash",
                "line\nbreak\tvalue",
                "",
                "",
            ]

            for mode in ("cached", "build"):
                for arg_kind, args in (("empty", []), ("complex", complex_args)):
                    with self.subTest(mode=mode, arguments=arg_kind):
                        argv = ["./wrapper link"]
                        if mode == "build":
                            argv.extend(["--build", "--build"])
                        argv.extend(args)
                        for stale in (tmp / "child-cwd", tmp / "child-args"):
                            stale.unlink(missing_ok=True)
                        res = run(argv, cwd=caller_dir, env=env)
                        self.assertEqual(res.returncode, 7, f"{mode}/{arg_kind} lost exit code")
                        self.assertEqual(
                            (tmp / "child-cwd").read_text(encoding="utf-8").strip(),
                            str(caller_dir),
                        )
                        self.assertEqual(
                            (tmp / "child-context").read_text(encoding="utf-8").splitlines(),
                            [str(tmp), "sh", "1"],
                        )
                        self.assertEqual(
                            int((tmp / "child-argc").read_text(encoding="utf-8").strip()),
                            len(args),
                        )
                        expected_bytes = b"".join(a.encode("utf-8") + b"\0" for a in args)
                        self.assertEqual((tmp / "child-args").read_bytes(), expected_bytes)
                        self.assertEqual(res.stdout, "child stdout\n")
                        self.assertEqual(res.stderr, "child stderr\n")

            self.assertEqual(
                (tmp / "cargo-cwd").read_text(encoding="utf-8").strip(),
                str(tmp / "cli"),
            )
            (tmp / "child-cwd").unlink()
            fail_env = dict(env, WRAPPER_CARGO_EXIT="23")
            fail_res = run(
                [str(tmp / "dotfiles.sh"), "--build", "--version"], cwd=caller_dir, env=fail_env
            )
            self.assertEqual(fail_res.returncode, 23)
            self.assertFalse((tmp / "child-cwd").exists())

    def test_wrapper_uses_cargo_artifact(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wrapper-artifact-") as tmp_str:
            fixture = Path(tmp_str).resolve()
            wrapper = copy_wrapper(fixture)
            (fixture / "cli" / ".cargo").mkdir(parents=True)
            caller_dir = fixture / "caller dir"
            caller_dir.mkdir()

            write_executable(
                fixture / "cli" / "target" / "dev-opt" / "dotfiles",
                '#!/bin/sh\necho "ERROR: stale default-target binary executed" >&2\nexit 91\n',
            )
            write_executable(
                fixture / "fake-bin" / "cargo",
                "#!/bin/sh\n"
                "set -eu\n"
                "[ \"$*\" = 'build --profile dev-opt --bin dotfiles --message-format=json-render-diagnostics' ]\n"
                "target=${CARGO_TARGET_DIR:-$(sed -n 's/^target-dir = \"\\(.*\\)\"$/\\1/p' .cargo/config.toml)}\n"
                'case "$target" in /*) ;; *) target="$PWD/$target" ;; esac\n'
                'executable="$target/custom-triple/dev-opt/dotfiles"\n'
                'mkdir -p "$(dirname "$executable")"\n'
                "cat > \"$executable\" <<'BIN'\n"
                "#!/bin/sh\n"
                'printf \'%s\\n\' "$PWD" > "$DOTFILES_ROOT/child-cwd"\n'
                'printf \'%s\\n\' "$@" > "$DOTFILES_ROOT/child-args"\n'
                "exit 7\n"
                "BIN\n"
                'chmod +x "$executable"\n'
                'if [ "${WRAPPER_ARTIFACT_MODE:-valid}" != missing ]; then\n'
                "  escaped=$(printf '%s' \"$executable\" | sed 's/\\\\/\\\\\\\\/g; s/\"/\\\\\"/g; s,/,\\\\u002f,g; s/ /\\\\u0020/g')\n"
                '  printf \'{"reason":"compiler-artifact","target":{"name":"another-binary"},"executable":"not-the-dotfiles-artifact"}\\n\'\n'
                '  printf \'{"reason":"compiler-artifact","target":{"name":"dotfiles"},"executable":"%s"}\\n\' "$escaped"\n'
                '  if [ "${WRAPPER_ARTIFACT_MODE:-valid}" = duplicate ]; then\n'
                '    printf \'{"reason":"compiler-artifact","target":{"name":"dotfiles"},"executable":"%s"}\\n\' "$escaped"\n'
                "  fi\n"
                "fi\n"
                'printf \'{"reason":"build-finished","success":true}\\n\'\n',
            )

            base_env = dict(os.environ, PATH=f"{fixture / 'fake-bin'}:{os.environ.get('PATH', '')}")
            base_env.pop("CARGO_TARGET_DIR", None)

            for mode in ("environment", "config"):
                with self.subTest(mode=mode):
                    env = dict(base_env)
                    if mode == "environment":
                        env["CARGO_TARGET_DIR"] = str(fixture / "environment output")
                    else:
                        (fixture / "cli" / ".cargo" / "config.toml").write_text(
                            '[build]\ntarget-dir = "configured output"\n',
                            encoding="utf-8",
                        )
                    res = run(
                        [str(wrapper), "--build", "--version", "space value", ""],
                        cwd=caller_dir,
                        env=env,
                    )
                    self.assertEqual(res.returncode, 7, f"{mode} stderr:\n{res.stderr}")
                    self.assertEqual(
                        (fixture / "child-cwd").read_text(encoding="utf-8").strip(),
                        str(caller_dir),
                    )
                    self.assertEqual(
                        (fixture / "child-args").read_text(encoding="utf-8"),
                        "--version\nspace value\n\n",
                    )
                    self.assertEqual(res.stdout, "", f"{mode} leaked Cargo JSON to stdout")

            (fixture / "child-cwd").unlink()
            for artifact_mode in ("missing", "duplicate"):
                with self.subTest(artifact_mode=artifact_mode):
                    env = dict(base_env, WRAPPER_ARTIFACT_MODE=artifact_mode)
                    res = run([str(wrapper), "--build", "--version"], cwd=caller_dir, env=env)
                    self.assertNotEqual(res.returncode, 0, f"{artifact_mode} did not fail")
                    self.assertFalse((fixture / "child-cwd").exists())
                    self.assertEqual(res.stdout, "")
                    self.assertIn("Cargo did not report a unique dotfiles executable", res.stderr)
