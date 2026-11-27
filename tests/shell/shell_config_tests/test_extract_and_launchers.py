"""Archive extraction, wget, Windows git editor, and launcher tests."""

import bz2
import gzip
import hashlib
import http.server
import io
import lzma
import shlex
import shutil
import struct
import subprocess
import tarfile
import threading
import time

from shell_config_tests.base import ROOT, ShellConfigTestCase


class ExtractAndLaunchersTests(ShellConfigTestCase):
    def write_rpm(self, members):
        payload = bytearray()
        for name, data in [*members, ("TRAILER!!!", b"")]:
            encoded_name = name.encode() + b"\0"
            fields = (
                1,
                0o100600,
                0,
                0,
                1,
                int(time.time()) + 3600,
                len(data),
                0,
                0,
                0,
                0,
                len(encoded_name),
                0,
            )
            entry = b"070701" + "".join(f"{value:08x}" for value in fields).encode()
            entry += encoded_name
            entry += b"\0" * (-len(entry) % 4)
            entry += data + b"\0" * (-len(data) % 4)
            payload.extend(entry)
        # Minimal RPM lead and empty signature/header sections around a cpio payload.
        lead = struct.pack(
            ">4sBBHH66sHH16s",
            b"\xed\xab\xee\xdb",
            3,
            0,
            0,
            1,
            b"synthetic-security-fixture",
            1,
            5,
            b"",
        )
        header = struct.pack(">4s4sII", b"\x8e\xad\xe8\x01", b"", 0, 0)
        archive = self.fixture / "fixture with spaces.rpm"
        archive.write_bytes(lead + header + header + gzip.compress(payload))
        return archive

    def run_extract(self, archive, expected=0, options=()):
        self.env["ARCHIVE"] = str(archive)
        return self.run_shell(
            'cd "$HOME"\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            "autoload -Uz extract\n"
            f'extract {shlex.join(options)} "$ARCHIVE"\n'
            "local result=$?\n"
            '[[ "$PWD" == "$HOME" ]] || exit 99\n'
            'exit "$result"\n',
            expected=expected,
        )

    def test_extract_keep_preserves_standalone_inputs(self):
        compressors = {
            ".gz": gzip.compress,
            ".bz2": bz2.compress,
            ".xz": lzma.compress,
            ".zst": lambda data: (
                subprocess.run(
                    ["zstd", "-q", "-c"],
                    input=data,
                    capture_output=True,
                    check=True,
                ).stdout
            ),
        }
        for suffix, compress in compressors.items():
            with self.subTest(suffix=suffix):
                archive = self.fixture / "inputs" / ("sample with spaces" + suffix)
                archive.parent.mkdir(exist_ok=True)
                payload = compress(b"fixture\n")
                archive.write_bytes(payload)
                self.run_extract(archive, options=("--keep",))
                self.assertEqual(archive.read_bytes(), payload)
                self.assertEqual(archive.with_suffix("").read_bytes(), b"fixture\n")
                archive.with_suffix("").unlink()
                archive.unlink()
                self.assertEqual(list(archive.parent.iterdir()), [])
                self.assertEqual(list(self.home.iterdir()), [])

    def test_extract_standalone_destination_preserves_default_removal_and_keep(self):
        for keep in (False, True):
            with self.subTest(keep=keep):
                archive = self.home / f"-sample {keep}.gz"
                payload = gzip.compress(b"fixture\n")
                archive.write_bytes(payload)
                destination = f"output {keep}/nested directory"
                options = ("--destination", destination) + (("--keep",) if keep else ())
                self.run_extract(archive.name, options=options)
                directory = self.home / destination
                self.assertEqual((directory / archive.stem).read_bytes(), b"fixture\n")
                self.assertEqual(list(directory.iterdir()), [directory / archive.stem])
                self.assertEqual(archive.exists(), keep)
                if keep:
                    self.assertEqual(archive.read_bytes(), payload)
                self.assertFalse((self.home / archive.stem).exists())

    def test_extract_container_destination_preserves_archive_and_handles_option_names(self):
        archive = self.home / "--sample with spaces.tar.gz"
        with tarfile.open(archive, "w:gz") as output:
            member = tarfile.TarInfo("nested/file with spaces")
            member.size = len(b"fixture\n")
            output.addfile(member, io.BytesIO(b"fixture\n"))
        original = archive.read_bytes()
        destination = self.home / "output with spaces"
        self.run_extract(
            archive.name,
            options=("--keep", f"--destination={destination}", "--"),
        )
        self.assertEqual((destination / "nested/file with spaces").read_bytes(), b"fixture\n")
        self.assertEqual(archive.read_bytes(), original)
        self.assertFalse((self.home / "nested").exists())

    def test_extract_streaming_refuses_existing_output_paths(self):
        original = gzip.compress(b"replacement\n")
        archive = self.home / "sample.gz"
        archive.write_bytes(original)
        outside = self.write(self.fixture / "outside", "preserve\n")
        for kind in ("file", "directory", "symlink", "dangling-link"):
            with self.subTest(kind=kind):
                destination = self.home / kind
                destination.mkdir()
                target = destination / "sample"
                if kind == "file":
                    target.write_text("preserve\n")
                elif kind == "directory":
                    target.mkdir()
                else:
                    target.symlink_to(
                        outside if kind == "symlink" else outside.with_name("missing")
                    )
                result = self.run_extract(
                    archive, expected=1, options=("--destination", str(destination))
                )
                self.assertTrue(result.stderr)
                self.assertEqual(archive.read_bytes(), original)
                self.assertEqual(list(destination.iterdir()), [target])
                self.assertEqual(outside.read_text(), "preserve\n")
                if kind == "file":
                    self.assertEqual(target.read_text(), "preserve\n")
                elif kind == "directory":
                    self.assertEqual(list(target.iterdir()), [])
                else:
                    self.assertTrue(target.is_symlink())

    def test_extract_failed_stream_preserves_input_and_cleans_partial_output(self):
        tools = self.fixture / "tools"
        self.write(tools / "gunzip", "#!/bin/sh\nprintf partial\nexit 23\n", executable=True)
        self.env["PATH"] = str(tools) + ":/usr/bin:/bin"
        archive = self.write(self.home / "sample.gz", "preserve\n")
        destination = self.home / "output"
        self.run_extract(
            archive, expected=23, options=("--keep", "--destination", str(destination))
        )
        self.assertEqual(archive.read_text(), "preserve\n")
        self.assertEqual(list(destination.iterdir()), [])

    def test_extract_keep_preserves_caller_noclobber_option(self):
        archive = self.home / "sample.gz"
        original = gzip.compress(b"fixture\n")
        archive.write_bytes(original)
        self.run_shell(
            'cd "$HOME"\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            "autoload -Uz extract\n"
            "setopt noclobber\n"
            "extract --keep sample.gz || exit $?\n"
            "[[ -o noclobber ]]\n",
        )
        self.assertEqual(archive.read_bytes(), original)
        self.assertEqual((self.home / "sample").read_bytes(), b"fixture\n")

    def test_extract_invalid_options_and_destination_leave_inputs_unchanged(self):
        archive = self.home / "sample.gz"
        original = gzip.compress(b"fixture\n")
        archive.write_bytes(original)
        blocked = self.write(self.home / "not a directory", "preserve\n")
        for options in (
            ("--destination",),
            ("--destination=",),
            ("--destination", ""),
            ("--unknown",),
            ("--destination", str(blocked)),
        ):
            with self.subTest(options=options):
                result = self.run_extract(archive, expected=1, options=options)
                self.assertTrue(result.stderr)
                self.assertEqual(archive.read_bytes(), original)
                self.assertEqual(blocked.read_text(), "preserve\n")
                self.assertEqual(set(self.home.iterdir()), {archive, blocked})

    def test_extract_rpm_reads_payload(self):
        archive = self.write_rpm([("nested/file with spaces", b"fixture\n")])
        self.run_extract(archive)
        self.assertEqual((self.home / "nested/file with spaces").read_bytes(), b"fixture\n")

    def test_extract_missing_tool_names_the_requirement_and_preserves_archive(self):
        tools = self.fixture / "empty tools"
        tools.mkdir()
        (tools / "zsh").symlink_to(shutil.which("zsh"))
        self.env["PATH"] = str(tools)
        for suffix, tool in (
            (".tar.gz", "tar"),
            (".bz2", "bunzip2"),
            (".gz", "gunzip"),
            (".rar", "unrar"),
            (".zip", "unzip"),
            (".7z", "7z"),
            (".Z", "uncompress"),
            (".xz", "unxz"),
            (".zst", "unzstd"),
            (".deb", "ar"),
            (".rpm", "bsdtar"),
        ):
            with self.subTest(suffix=suffix):
                archive = self.write(self.home / ("-archive with spaces" + suffix), "keep\n")
                result = self.run_extract(archive.name, expected=127)
                self.assertEqual(
                    result.stderr, f"extract: {tool} is required to extract {archive.name}\n"
                )
                self.assertEqual(archive.read_bytes(), b"keep\n")
                self.assertEqual(list(self.home.iterdir()), [archive])
                archive.unlink()

    def test_extract_optional_tools_receive_literal_arguments_and_propagate_failure(self):
        tools = self.fixture / "tools"
        for suffix, tool, options, operation in (
            (".rar", "unrar", (), "x"),
            (".7z", "7z", (), "x"),
            (".Z", "uncompress", ("--keep",), "-c"),
        ):
            with self.subTest(suffix=suffix):
                self.write(
                    tools / tool,
                    '#!/bin/sh\nprintf "%s\\n" "$@" > "$HOME/arguments"\nexit 23\n',
                    executable=True,
                )
                self.env["PATH"] = str(tools) + ":/usr/bin:/bin"
                archive = self.write(self.home / ("-archive $(false)" + suffix), "keep\n")
                self.run_extract(archive.name, expected=23, options=options)
                self.assertEqual(
                    (self.home / "arguments").read_text(), f"{operation}\n./{archive.name}\n"
                )
                self.assertEqual(archive.read_bytes(), b"keep\n")

    def test_extract_option_like_relative_archive_name(self):
        archive = self.home / "-sample with spaces.gz"
        archive.write_bytes(gzip.compress(b"fixture\n"))
        self.run_extract(archive.name)
        self.assertEqual((self.home / "-sample with spaces").read_bytes(), b"fixture\n")

    def test_extract_standalone_zstandard_archive(self):
        self.assertIsNotNone(shutil.which("zstd"), "zstd is required for extraction coverage")
        archive = self.home / "-sample with spaces.zst"
        compressed = subprocess.run(
            ["zstd", "-q", "-c"],
            input=b"fixture\n",
            capture_output=True,
            check=True,
        ).stdout
        archive.write_bytes(compressed)
        self.run_extract(archive.name)
        self.assertEqual((self.home / "-sample with spaces").read_bytes(), b"fixture\n")
        self.assertFalse(archive.exists())

    def test_extract_rejects_multiple_archives_before_changing_any_files(self):
        archives = [self.home / name for name in ("first.gz", "second.gz")]
        payload = gzip.compress(b"fixture\n")
        for archive in archives:
            archive.write_bytes(payload)
        result = self.run_shell(
            'cd "$HOME"\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            "autoload -Uz extract\n"
            "extract first.gz second.gz\n",
            expected=1,
        )
        self.assertIn("Usage: extract", result.stderr)
        self.assertEqual(
            sorted(path.name for path in self.home.iterdir()), ["first.gz", "second.gz"]
        )
        for archive in archives:
            self.assertEqual(archive.read_bytes(), payload)

    def test_extract_rpm_confines_member_paths(self):
        target = self.write(self.fixture / "outside", "original\n")
        (self.home / "escape").symlink_to(self.fixture, target_is_directory=True)
        for member, expected in (
            ("../outside", 1),
            (str(target), 0),
            ("escape/outside", 1),
        ):
            with self.subTest(member=member):
                archive = self.write_rpm([(member, b"replaced\n")])
                result = self.run_extract(archive, expected=expected)
                self.assertEqual(target.read_text(), "original\n")
                self.assertTrue(result.stderr, "unsafe paths must not be handled silently")

    def test_extract_rpm_propagates_invalid_archive_failure(self):
        archive = self.write(self.fixture / "invalid.rpm", "not an archive\n")
        result = self.run_extract(archive, expected=1)
        self.assertIn("Error", result.stderr)
        self.assertEqual(list(self.home.iterdir()), [])

    def test_windows_git_editor_is_scoped_to_desktop(self):
        config = self.home / ".config/git"
        self.write(config / "windows", (ROOT / "symlinks/config/git/windows").read_text())
        for desktop in (False, True):
            with self.subTest(desktop=desktop):
                if desktop:
                    self.write(
                        config / "windows-desktop",
                        (ROOT / "symlinks/config/git/windows-desktop").read_text(),
                    )
                result = subprocess.run(
                    [
                        "git",
                        "config",
                        "--file",
                        str(config / "windows"),
                        "--includes",
                        "--get",
                        "core.editor",
                    ],
                    cwd=self.fixture,
                    env=self.env,
                    text=True,
                    capture_output=True,
                )
                self.assertEqual(result.returncode, 0 if desktop else 1, result.stderr)
                self.assertEqual(result.stdout.strip(), "code-insiders --wait" if desktop else "")

    def test_wget_preserves_url_names_and_reuses_unchanged_downloads(self):
        requests = []

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_HEAD(self):
                self.do_GET()

            def do_GET(self):
                requests.append((self.path, self.headers.get("If-Modified-Since")))
                if self.path == "/redirect":
                    self.send_response(302)
                    self.send_header("Location", "/renamed")
                elif self.headers.get("If-Modified-Since"):
                    self.send_response(304)
                else:
                    self.send_response(200)
                    self.send_header("Content-Type", "text/html")
                    self.send_header("Content-Disposition", 'attachment; filename="header.html"')
                    self.send_header("Last-Modified", "Mon, 05 Oct 2026 00:00:00 GMT")
                    self.send_header("Content-Length", "7")
                self.end_headers()
                if (
                    self.command != "HEAD"
                    and self.path != "/redirect"
                    and not self.headers.get("If-Modified-Since")
                ):
                    self.wfile.write(b"fixture")

            def log_message(self, *_args):
                pass

        server = http.server.ThreadingHTTPServer(("localhost", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            self.env["WGETRC"] = str(ROOT / "symlinks/config/wgetrc")
            url = f"http://localhost:{server.server_port}"

            def download(path, *options):
                subprocess.run(
                    ["wget", "-q", *options, url + path],
                    cwd=self.fixture,
                    env=self.env,
                    check=True,
                    timeout=10,
                )

            download("/page")
            download("/page")
            self.assertEqual((self.fixture / "page").read_bytes(), b"fixture")
            self.assertTrue(any(path == "/page" and date for path, date in requests))
            download("/redirect")
            self.assertTrue((self.fixture / "redirect").is_file())
            self.assertFalse((self.fixture / "renamed").exists())
            self.assertFalse((self.fixture / "header.html").exists())
            download("/extension", "--adjust-extension")
            self.assertTrue((self.fixture / "extension.html").is_file())
            download("/redirect", "--trust-server-names")
            self.assertTrue((self.fixture / "renamed").is_file())
            download("/header", "--content-disposition")
            self.assertTrue((self.fixture / "header.html").is_file())
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_wget_does_not_append_changed_content_without_modification_date(self):
        payload = b"NEW-CONTENT-LONGER\n"
        ranges = []

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_HEAD(self):
                self.respond(False)

            def do_GET(self):
                self.respond(True)

            def respond(self, body):
                requested = self.headers.get("Range")
                ranges.append(requested)
                start = int(requested.split("=")[1].split("-")[0]) if requested else 0
                content = payload[start:]
                self.send_response(206 if requested else 200)
                self.send_header("Content-Length", str(len(content)))
                if requested:
                    self.send_header(
                        "Content-Range", f"bytes {start}-{len(payload) - 1}/{len(payload)}"
                    )
                self.end_headers()
                if body:
                    self.wfile.write(content)

            def log_message(self, *args):
                pass

        server = http.server.HTTPServer(("localhost", 0), Handler)
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        try:
            self.write(self.fixture / "release.bin", "OLD\n")
            self.env["WGETRC"] = str(ROOT / "symlinks/config/wgetrc")
            subprocess.run(
                ["wget", "-q", f"http://localhost:{server.server_port}/release.bin"],
                cwd=self.fixture,
                env=self.env,
                check=True,
                timeout=10,
            )
            downloads = [path.read_bytes() for path in self.fixture.glob("release.bin*")]
            self.assertIn(payload, downloads)
            self.assertTrue(all(content in (b"OLD\n", payload) for content in downloads))
            self.assertTrue(ranges)
            self.assertTrue(all(value is None for value in ranges))
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_desktop_code_alias_requires_usable_code_insiders(self):
        legacy_shim = (
            b"#!/usr/bin/env python3\n"
            b"\n"
            b"import os\n"
            b"import sys\n"
            b"\n"
            b'REAL_BINARY = "/usr/bin/code-insiders"\n'
            b'PASSWORD_STORE = "--password-store=gnome-libsecret"\n'
            b"\n"
            b"\n"
            b"def normalized_args(arguments: list[str]) -> list[str]:\n"
            b"    forwarded: list[str] = []\n"
            b"    index = 0\n"
            b"\n"
            b"    while index < len(arguments):\n"
            b"        argument = arguments[index]\n"
            b'        if argument == "--":\n'
            b"            forwarded.extend(arguments[index:])\n"
            b"            break\n"
            b'        if argument == "--password-store":\n'
            b"            index += 1\n"
            b'            if index < len(arguments) and not arguments[index].startswith("-"):\n'
            b"                index += 1\n"
            b"            continue\n"
            b'        if argument.startswith("--password-store="):\n'
            b"            index += 1\n"
            b"            continue\n"
            b"\n"
            b"        forwarded.append(argument)\n"
            b"        index += 1\n"
            b"\n"
            b"    return [PASSWORD_STORE, *forwarded]\n"
            b"\n"
            b"\n"
            b"def main() -> None:\n"
            b"    os.execv(REAL_BINARY, [REAL_BINARY, *normalized_args(sys.argv[1:])])\n"
            b"\n"
            b"\n"
            b'if __name__ == "__main__":\n'
            b"    main()\n"
        )
        self.assertEqual(
            hashlib.sha256(legacy_shim).hexdigest(),
            "abbafb0e03fa9c8534a5a74a46cbfc4357e73a34497da0966f96cddd6017eec9",
        )
        shim_source = ROOT / "symlinks/local/bin/code-insiders"
        helper_target = self.home / ".config/shell/code-insiders.sh"
        helper_target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / "symlinks/config/shell/code-insiders.sh", helper_target)
        shim_target = self.home / ".local/bin/code-insiders"
        other_bin = self.fixture / "other-bin"
        base_bin = self.fixture / "base-bin"
        base_bin.mkdir()
        for tool in ("bash", "zsh", "sha256sum"):
            resolved = shutil.which(tool, path=self.env["PATH"])
            if resolved:
                (base_bin / tool).symlink_to(resolved)

        cases = (
            ("missing", None, False, False),
            ("shim-link-no-backend", "link", False, False),
            ("shim-copy-no-backend", "copy", False, False),
            ("modified-marked-shim-no-backend", "modified-marked", False, False),
            ("legacy-shim-copy-no-backend", "legacy-copy", False, False),
            ("shim-with-backend", "link", True, True),
            ("custom-at-shim-path-no-backend", "custom", False, True),
            ("other-path-no-backend", "other", False, True),
        )
        for label, mode, backend_exists, expect_alias in cases:
            if shim_target.exists() or shim_target.is_symlink():
                shim_target.unlink()
            if (other_bin / "code-insiders").exists():
                (other_bin / "code-insiders").unlink()
            path_parts = [str(self.home / ".local/bin"), str(base_bin)]
            if mode == "link":
                shim_target.parent.mkdir(parents=True, exist_ok=True)
                shim_target.symlink_to(shim_source)
            elif mode == "copy":
                shim_target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(shim_source, shim_target)
                shim_target.chmod(0o755)
            elif mode == "modified-marked":
                shim_target.parent.mkdir(parents=True, exist_ok=True)
                shim_target.write_text(
                    shim_source.read_text(encoding="utf-8") + "\n# future shim edit\n",
                    encoding="utf-8",
                )
                shim_target.chmod(0o755)
            elif mode == "legacy-copy":
                shim_target.parent.mkdir(parents=True, exist_ok=True)
                shim_target.write_bytes(legacy_shim)
                shim_target.chmod(0o755)
            elif mode == "custom":
                self.write(shim_target, "#!/bin/sh\nexit 0\n", executable=True)
            elif mode == "other":
                self.write(other_bin / "code-insiders", "#!/bin/sh\nexit 0\n", executable=True)
                path_parts.insert(0, str(other_bin))

            self.env["PATH"] = ":".join(path_parts)
            backend_rc = "0" if backend_exists else "1"
            guard_hash = (
                "sha256sum() { exit 99; }\n"
                if backend_exists or mode in ("link", "copy", "modified-marked")
                else ""
            )
            for shell in ("bash", "zsh"):
                with self.subTest(case=label, shell=shell):
                    script = (
                        "set -u\n"
                        f"{guard_hash}"
                        "[() {\n"
                        '  if builtin [ "$#" -eq 3 ] && builtin [ "$1" = "-x" ] && '
                        'builtin [ "$2" = "/usr/bin/code-insiders" ]; then\n'
                        f"    return {backend_rc}\n"
                        "  fi\n"
                        '  builtin [ "$@"\n'
                        "}\n"
                        '. "$ROOT/symlinks/config/shell/aliases-desktop.sh"\n'
                        '[ "${_code_insiders+x}" != x ] || exit 2\n'
                        '[ "${_code_insiders_digest+x}" != x ] || exit 3\n'
                        "if alias code >/dev/null 2>&1; then\n"
                        "  alias code\n"
                        "else\n"
                        '  printf "none\\n"\n'
                        "fi\n"
                    )
                    result = self.run_shell(script, shell=shell)
                    output = result.stdout.strip()
                    if expect_alias:
                        self.assertIn("code-insiders", output)
                    else:
                        self.assertEqual(output, "none")

    def test_code_insiders_shim_reports_missing_backend_cleanly(self):
        source = (ROOT / "symlinks/local/bin/code-insiders").read_text(encoding="utf-8")
        missing = self.fixture / "missing-code-insiders"
        patched = source.replace(
            'REAL_BINARY = "/usr/bin/code-insiders"',
            f"REAL_BINARY = {str(missing)!r}",
        )
        self.assertNotEqual(patched, source)
        shim = self.write(self.fixture / "code-insiders-shim", patched, executable=True)
        result = subprocess.run(
            ["python3", str(shim), "--version"],
            cwd=self.fixture,
            env=self.env,
            text=True,
            capture_output=True,
            timeout=10,
        )
        self.assertEqual(result.returncode, 127, result.stdout + result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertIn(str(missing), result.stderr)
        self.assertNotIn("Traceback", result.stderr)
