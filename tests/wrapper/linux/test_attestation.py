"""Build provenance attestation verification tests for dotfiles.sh."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import tempfile
import unittest

from .helpers import copy_wrapper, run, symlink_system_tools, write_executable

_SYSTEM_TOOLS = (
    "awk",
    "cat",
    "chmod",
    "dirname",
    "flock",
    "mkdir",
    "mktemp",
    "mv",
    "readlink",
    "rm",
    "rmdir",
    "sha256sum",
    "uname",
    "grep",
)


class WrapperAttestationTests(unittest.TestCase):
    def test_wrapper_attestation_verification(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wrapper-attest-") as tmp_str:
            tmpdir = Path(tmp_str).resolve()
            wrapper = copy_wrapper(tmpdir)
            fake_bin = tmpdir / "fake-bin"

            write_executable(
                fake_bin / "curl",
                "#!/bin/sh\n"
                "set -o errexit\n"
                "set -o nounset\n"
                'out=""\n'
                'url=""\n'
                'while [ "$#" -gt 0 ]; do\n'
                '  case "$1" in\n'
                '    -o) shift; out="$1" ;;\n'
                '    http*) url="$1" ;;\n'
                "  esac\n"
                "  shift\n"
                "done\n"
                'case "$url" in\n'
                '  */releases/latest) printf \'{"tag_name":"v9.9.9"}\\n\' ;;\n'
                "  */releases/download/v9.9.9/checksums.sha256)\n"
                '    sum=$(sha256sum "$(cat "$DOTFILES_ROOT/download-path")" | awk \'{print $1}\')\n'
                '    printf \'%s  dotfiles-linux-x86_64\\n\' "$sum" > "$out"\n'
                "    ;;\n"
                "  */releases/download/v9.9.9/dotfiles-linux-x86_64)\n"
                '    mkdir -p "$(dirname "$out")"\n'
                '    printf \'%s\\n\' "$out" > "$DOTFILES_ROOT/download-path"\n'
                "    cat > \"$out\" <<'BIN'\n"
                "#!/bin/sh\n"
                'printf \'%s\\n\' "$GH_PROMPT_DISABLED" "$DBUS_SESSION_BUS_ADDRESS" > "$DOTFILES_ROOT/child-env"\n'
                '[ "${WRAPPER_BAD_BINARY:-0}" = 0 ] || exit 42\n'
                "printf 'dotfiles v9.9.9\\n'\n"
                "BIN\n"
                "    ;;\n"
                '  *) echo "unexpected URL: $url" >&2; exit 1 ;;\n'
                "esac\n",
            )
            write_executable(
                fake_bin / "gh",
                "#!/bin/sh\n"
                'printf \'%s\\n\' "$GH_PROMPT_DISABLED" "$DBUS_SESSION_BUS_ADDRESS" > "$DOTFILES_ROOT/gh-env"\n'
                'exit "${WRAPPER_GH_EXIT:-1}"\n',
            )

            base_env = dict(
                os.environ,
                PATH=f"{fake_bin}:{os.environ.get('PATH', '')}",
                DOTFILES_RELEASE_REPO="fixture/releases",
                GH_PROMPT_DISABLED="caller",
                DBUS_SESSION_BUS_ADDRESS="caller",
            )

            # Required (default): unverified provenance is fatal and removes the download.
            res = run(
                [str(wrapper), "--version"],
                cwd=tmpdir,
                env=dict(base_env, DOTFILES_SKIP_ATTESTATION="0"),
            )
            self.assertNotEqual(res.returncode, 0, "Default policy accepted an unverified binary")
            self.assertFalse((tmpdir / "bin" / "dotfiles").exists())
            self.assertEqual(
                (tmpdir / "gh-env").read_text(encoding="utf-8").splitlines(),
                ["1", "disabled:"],
            )
            self.assertIn("GH_TOKEN/GITHUB_TOKEN", res.stderr)

            # Verified binary that fails its startup smoke test must not be published.
            res = run(
                [str(wrapper), "--version"],
                cwd=tmpdir,
                env=dict(
                    base_env,
                    DOTFILES_SKIP_ATTESTATION="0",
                    WRAPPER_GH_EXIT="0",
                    WRAPPER_BAD_BINARY="1",
                ),
            )
            self.assertNotEqual(res.returncode, 0)
            self.assertFalse((tmpdir / "bin" / "dotfiles").exists())
            self.assertIn("Downloaded CLI did not start", res.stderr)

            # Verified healthy binary keeps caller environment intact.
            res = run(
                [str(wrapper), "--version"],
                cwd=tmpdir,
                env=dict(base_env, DOTFILES_SKIP_ATTESTATION="0", WRAPPER_GH_EXIT="0"),
            )
            self.assertEqual(res.returncode, 0, res.stderr)
            self.assertEqual(
                (tmpdir / "child-env").read_text(encoding="utf-8").splitlines(),
                ["caller", "caller"],
            )

            # Skip: verification is bypassed without invoking gh.
            shutil.rmtree(tmpdir / "bin")
            (tmpdir / "gh-env").unlink(missing_ok=True)
            res = run(
                [str(wrapper), "--version"],
                cwd=tmpdir,
                env=dict(base_env, DOTFILES_SKIP_ATTESTATION="1"),
            )
            self.assertEqual(res.returncode, 0, res.stderr)
            self.assertTrue(os.access(tmpdir / "bin" / "dotfiles", os.X_OK))
            self.assertFalse((tmpdir / "gh-env").exists())

            # Missing gh: fail closed unless explicitly bypassed.
            shutil.rmtree(tmpdir / "bin")
            (fake_bin / "gh").unlink()
            symlink_system_tools(fake_bin, _SYSTEM_TOOLS)
            minimal_env = dict(base_env, PATH=str(fake_bin))

            res = run(
                [str(wrapper), "--version"],
                cwd=tmpdir,
                env=dict(minimal_env, DOTFILES_SKIP_ATTESTATION="0"),
            )
            self.assertNotEqual(res.returncode, 0)
            self.assertFalse((tmpdir / "bin" / "dotfiles").exists())
            self.assertIn("ERROR: gh not found", res.stderr)

            res = run(
                [str(wrapper), "--version"],
                cwd=tmpdir,
                env=dict(minimal_env, DOTFILES_SKIP_ATTESTATION="1"),
            )
            self.assertEqual(res.returncode, 0, res.stderr)
            self.assertTrue(os.access(tmpdir / "bin" / "dotfiles", os.X_OK))
