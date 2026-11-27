#!/usr/bin/env python3
"""Exercise Quickshell output and lock lifecycles in a disposable Wayland session."""

import argparse
import base64
import importlib.util
import json
import os
from pathlib import Path
import re
import resource
import select
import subprocess
import sys
import tempfile
import time

SOURCE = Path(__file__).resolve().parents[3]
# Minimal 1x1 RGB PNG so hyprpaper's configured wallpaper path resolves in CI.
_TINY_PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4"
    "nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg=="
)


def _load_workflow_yaml():
    name = "workflow_yaml"
    if name in sys.modules:
        return sys.modules[name]
    path = SOURCE / ".github/workflows/scripts/workflow_yaml.py"
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot load {name} from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _yaml_mapping_paths(text: str) -> set[str]:
    """Collect dotted mapping key paths outside YAML sequence items."""
    stack: list[tuple[int, str]] = []
    paths: set[str] = set()
    seq_indent: int | None = None
    for line in text.splitlines():
        stripped = line.lstrip(" ")
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(stripped)
        if seq_indent is not None and indent <= seq_indent:
            seq_indent = None
        if stripped.startswith("-"):
            if seq_indent is None:
                seq_indent = indent
            continue
        if seq_indent is not None:
            continue
        match = re.match(r"^([A-Za-z0-9_]+)\s*:", stripped)
        if not match:
            continue
        while stack and stack[-1][0] >= indent:
            stack.pop()
        stack.append((indent, match.group(1)))
        paths.add(".".join(key for _, key in stack))
    return paths


def check_hypr_configuration(env):
    config = SOURCE / "symlinks/config/hypr"
    subprocess.run(
        ["Hyprland", "--verify-config", "--config", str(config / "hyprland.lua")],
        env=env,
        check=True,
        timeout=25,
    )
    # Hyprlock reads its configuration before attempting the Wayland connection.
    result = subprocess.run(
        [
            "hyprlock",
            "--config",
            str(config / "hyprlock.conf"),
            "--display",
            "dotfiles-config-test",
        ],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=25,
    )
    if (
        result.returncode == 0
        or "Couldn't connect to a wayland compositor" not in result.stdout
        or "Config has errors" in result.stdout
    ):
        raise RuntimeError(
            f"Hyprlock configuration check failed ({result.returncode}):\n{result.stdout}"
        )

    # Hypridle also parses its configuration before connecting to Wayland.
    idle = subprocess.run(
        ["hypridle", "--config", str(config / "hypridle.conf")],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=25,
    )
    if (
        idle.returncode == 0
        or "Couldn't connect to a wayland compositor" not in idle.stdout
        or "Config has errors" in idle.stdout
        or "Registered timeout rule for 600s" not in idle.stdout
        or "Registered timeout rule for 900s" not in idle.stdout
    ):
        raise RuntimeError(
            f"Hypridle configuration check failed ({idle.returncode}):\n{idle.stdout}"
        )


def check_desktop_app_configurations(env):
    configs = SOURCE / "symlinks/config"

    subprocess.run(
        ["fuzzel", f"--config={configs / 'fuzzel/fuzzel.ini'}", "--check-config"],
        env=env,
        check=True,
        timeout=25,
    )

    gammastep = subprocess.run(
        ["gammastep", "-c", str(configs / "gammastep/config.ini"), "-p"],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=25,
    )
    if (
        gammastep.returncode != 0
        or "Location:" not in gammastep.stdout
        or "Color temperature:" not in gammastep.stdout
    ):
        raise RuntimeError(
            f"Gammastep configuration check failed ({gammastep.returncode}):\n{gammastep.stdout}"
        )

    # Mako parses its configuration before attempting to connect to Wayland.
    mako_env = dict(env)
    mako_env.pop("WAYLAND_DISPLAY", None)
    mako_cmd = ["mako", "-c", str(configs / "mako/config")]
    if not mako_env.get("DBUS_SESSION_BUS_ADDRESS"):
        mako_cmd = ["dbus-run-session", "--", *mako_cmd]
    mako = subprocess.run(
        mako_cmd,
        env=mako_env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=25,
    )
    if "Failed to parse" in mako.stdout or "failed to create display" not in mako.stdout:
        raise RuntimeError(f"Mako configuration check failed ({mako.returncode}):\n{mako.stdout}")

    # Alacritty migrate validates TOML and surfaces any renamed/deprecated keys.
    alacritty_cfg = configs / "alacritty/alacritty.toml"
    alacritty = subprocess.run(
        ["alacritty", "migrate", "--config-file", str(alacritty_cfg), "--dry-run"],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=25,
    )
    if alacritty.returncode != 0 or "Successfully migrated" not in alacritty.stdout:
        raise RuntimeError(
            f"Alacritty configuration check failed ({alacritty.returncode}):\n{alacritty.stdout}"
        )
    lines = alacritty.stdout.splitlines()
    start = next(
        (i for i, line in enumerate(lines) if line.startswith("v-----Start TOML")),
        None,
    )
    end = next(
        (i for i, line in enumerate(lines) if line.startswith("^-----End TOML")),
        None,
    )
    if start is None or end is None or end <= start:
        raise RuntimeError(f"Unexpected alacritty migrate output:\n{alacritty.stdout}")
    migrated = "\n".join(lines[start + 1 : end]).strip()
    if migrated != alacritty_cfg.read_text(encoding="utf-8").strip():
        raise RuntimeError(
            "alacritty.toml contains deprecated or migratable keys; run `alacritty migrate`"
        )

    # Verify every configured btop key exists in the installed binary's default schema.
    btop_default = subprocess.run(
        ["btop", "--default-config"],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=True,
        timeout=25,
    ).stdout
    known_btop_keys = set(re.findall(r"^(?:#\s*)?([A-Za-z0-9_]+)\s*=", btop_default, re.MULTILINE))
    btop_cfg = (configs / "btop/btop.conf").read_text(encoding="utf-8")
    configured_btop_keys = set(re.findall(r"^([A-Za-z0-9_]+)\s*=", btop_cfg, re.MULTILINE))
    unknown_btop = sorted(configured_btop_keys - known_btop_keys)
    if not configured_btop_keys or unknown_btop:
        raise RuntimeError(f"Unknown or missing btop config keys: {unknown_btop}")

    # Verify every configured lazygit key path exists in `lazygit --config` and
    # diffRenderers entries are well-formed.
    lg_default = subprocess.run(
        ["lazygit", "--config"],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=True,
        timeout=25,
    ).stdout
    lg_cfg_text = (configs / "lazygit/config.yml").read_text(encoding="utf-8")
    unknown_lg = sorted(_yaml_mapping_paths(lg_cfg_text) - _yaml_mapping_paths(lg_default))
    if unknown_lg:
        raise RuntimeError(f"Unknown lazygit config key paths: {unknown_lg}")
    parsed_lg = _load_workflow_yaml().parse_workflow(
        lg_cfg_text, "symlinks/config/lazygit/config.yml"
    )
    renderers = parsed_lg.get("git", {}).get("diffRenderers")
    if not isinstance(renderers, list) or not renderers:
        raise RuntimeError("lazygit config must define git.diffRenderers")
    for entry in renderers:
        if not isinstance(entry, dict) or not entry.get("name"):
            raise RuntimeError(f"Invalid lazygit diffRenderer entry: {entry!r}")


def check_hyprpaper_configuration(env, root):
    wallpaper = Path(env["HOME"]) / ".config/wallpaper/default.png"
    wallpaper.parent.mkdir(parents=True, exist_ok=True)
    if not wallpaper.exists():
        wallpaper.write_bytes(_TINY_PNG)
    log_path = root / "hyprpaper.log"
    with log_path.open("w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            ["hyprpaper", "--config", str(SOURCE / "symlinks/config/hypr/hyprpaper.conf")],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        time.sleep(1.0)
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)
    output = log_path.read_text(encoding="utf-8", errors="replace")
    if (
        "Config error" in output
        or "config option" in output
        or "wl_display_connect failed" in output
    ):
        raise RuntimeError(f"Hyprpaper configuration check failed:\n{output}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--native-hyprland",
        action="store_true",
        help="require nested Hyprland and the tracked Hyprlock config; needs GPU-backed buffer allocation",
    )
    parser.add_argument(
        "--configs-only",
        action="store_true",
        help="validate desktop and TUI configurations without starting a headless Wayland session",
    )
    args = parser.parse_args()
    native = args.native_hyprland
    if not args.configs_only and (not Path("/.dockerenv").exists() or os.geteuid() == 0):
        raise RuntimeError("run as the fixture user inside the desktop CI container")
    # The headless Hyprlock/Hypridle parser checks deliberately stop at the missing display.
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    with tempfile.TemporaryDirectory(prefix="q") as temporary:
        root = Path(temporary)
        runtime = root / "r"
        runtime.mkdir(mode=0o700)
        env = dict(os.environ)
        for key in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "HYPRLAND_INSTANCE_SIGNATURE"):
            env.pop(key, None)
        if args.configs_only:
            env.pop("DBUS_SESSION_BUS_ADDRESS", None)
        env.update(
            XDG_RUNTIME_DIR=str(runtime),
            XDG_CONFIG_HOME=str(root / "config"),
            XDG_CACHE_HOME=str(root / "cache"),
            WLR_BACKENDS="headless",
            WLR_RENDERER="gles2" if native else "pixman",
            WLR_LIBINPUT_NO_DEVICES="1",
            LIBGL_ALWAYS_SOFTWARE="0" if native else "1",
            QT_QUICK_BACKEND="software",
            HYPRLAND_TRACE="1",
            HYPRLAND_LOG_WLR="1",
            HYPRLAND_NO_SD_NOTIFY="1",
            HYPRLAND_NO_SD_VARS="1",
        )
        check_hypr_configuration(env)
        check_desktop_app_configurations(env)
        if args.configs_only:
            print("Desktop and TUI configuration checks passed")
            return
        units = SOURCE / "symlinks/config/systemd/user"
        subprocess.run(
            [
                "systemd-analyze",
                "--user",
                "--man=no",
                "--recursive-errors=no",
                "verify",
                *sorted(units.glob("*.service")),
                *sorted(units.glob("*.timer")),
            ],
            env=env,
            check=True,
            timeout=25,
        )
        processes = []
        logs = []

        def start(name, args):
            path = root / (name + ".log")
            log = path.open("w")
            logs.append((path, log))
            process = subprocess.Popen(
                args, env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT
            )
            processes.append(process)
            return process

        def wait_for(description, predicate):
            deadline = time.monotonic() + 25
            while time.monotonic() < deadline:
                for process in processes:
                    if process.poll() is not None:
                        raise RuntimeError(f"child {process.args} exited {process.returncode}")
                if predicate():
                    return
                time.sleep(0.2)
            raise RuntimeError("timed out: " + description)

        def ctl(*args):
            return subprocess.check_output(
                ["hyprctl", *args], env=env, text=True, timeout=5
            ).strip()

        def query(name):
            return json.loads(ctl("-j", name))

        def bar_on(name):
            return "dotfiles-bar" in json.dumps(query("layers").get(name, {}))

        try:
            sway_config = root / "sway.conf"
            sway_config.write_text("output * mode 1280x720\n")
            start("sway", ["sway", "-c", str(sway_config)])
            wait_for("parent compositor", lambda: (runtime / "wayland-1").is_socket())
            env["WAYLAND_DISPLAY"] = "wayland-1"
            check_hyprpaper_configuration(env, root)
            if native:
                hypr_config = root / "hyprland.lua"
                hypr_config.write_text(
                    'hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 1 })\nhl.config({ debug = { enable_stdout_logs = true, disable_logs = false } })\n'
                )
                start("hyprland", ["Hyprland", "--config", str(hypr_config)])
                wait_for(
                    "nested compositor IPC", lambda: bool(list(runtime.glob("hypr/*/.socket.sock")))
                )
                socket = next(runtime.glob("hypr/*/.socket.sock"))
                env["HYPRLAND_INSTANCE_SIGNATURE"] = socket.parent.name
                wait_for(
                    "nested Wayland socket",
                    lambda: any(
                        path.is_socket() and path.name != "wayland-1"
                        for path in runtime.glob("wayland-*")
                    ),
                )
                env["WAYLAND_DISPLAY"] = next(
                    path.name
                    for path in runtime.glob("wayland-*")
                    if path.is_socket() and path.name != "wayland-1"
                )
                assert not ctl("configerrors"), "fixture compositor configuration must be valid"
                original = query("monitors")[0]["name"]
                start(
                    "quickshell",
                    [
                        "quickshell",
                        "--no-duplicate",
                        "--path",
                        str(SOURCE / "symlinks/config/quickshell"),
                    ],
                )
                wait_for("real bar surface", lambda: bar_on(original))
                # Use the same native dispatcher expression as Bar.qml.
                ctl("dispatch", "hl.dsp.focus({ workspace = 2 })")
                wait_for("workspace activation", lambda: query("activeworkspace")["id"] == 2)
                ctl("output", "create", "headless", "TEST-OUTPUT")
                wait_for("bar on added output", lambda: bar_on("TEST-OUTPUT"))
                ctl(
                    "eval",
                    'hl.monitor({ output = "TEST-OUTPUT", mode = "1024x768@60", position = "auto", scale = 1 })',
                )
                wait_for(
                    "output resize",
                    lambda: any(
                        m["name"] == "TEST-OUTPUT" and m["width"] == 1024 for m in query("monitors")
                    ),
                )
                ctl("output", "remove", "TEST-OUTPUT")
                wait_for(
                    "output removal",
                    lambda: all(m["name"] != "TEST-OUTPUT" for m in query("monitors")),
                )
                wait_for("surviving bar after hotplug", lambda: bar_on(original))
                # The fixture has no systemd user manager. Wrapper/service behavior
                # is covered separately; here the real locker must acquire a lock.
                start(
                    "hyprlock",
                    [
                        "hyprlock",
                        "--no-fade-in",
                        "--config",
                        str(SOURCE / "symlinks/config/hypr/hyprlock.conf"),
                    ],
                )
                wait_for("native session lock", lambda: query("status")["locked"])
                print(
                    "Native desktop acceptance passed: bar, workspace, output add/resize/remove, session lock"
                )
            else:
                env["SWAYSOCK"] = str(next(runtime.glob("sway-ipc.*.sock")))

                def sway(*args):
                    return json.loads(
                        subprocess.check_output(
                            ["swaymsg", "-r", *args], env=env, text=True, timeout=5
                        )
                    )

                def active_outputs():
                    return [output for output in sway("-t", "get_outputs") if output["active"]]

                def reserved_bar(name):
                    output = next(
                        (output for output in active_outputs() if output["name"] == name), None
                    )
                    return output is not None and any(
                        workspace["output"] == name and workspace["rect"]["y"] > output["rect"]["y"]
                        for workspace in sway("-t", "get_workspaces")
                    )

                original = active_outputs()[0]["name"]
                start(
                    "quickshell",
                    [
                        "quickshell",
                        "--no-duplicate",
                        "--path",
                        str(SOURCE / "symlinks/config/quickshell"),
                    ],
                )
                wait_for("real bar reservation", lambda: reserved_bar(original))
                assert all(result["success"] for result in sway("workspace", "2"))
                wait_for(
                    "workspace activation",
                    lambda: any(
                        workspace["name"] == "2" and workspace["focused"]
                        for workspace in sway("-t", "get_workspaces")
                    ),
                )
                assert all(result["success"] for result in sway("create_output"))
                wait_for("second output", lambda: len(active_outputs()) == 2)
                added = next(
                    output["name"] for output in active_outputs() if output["name"] != original
                )
                wait_for("bar reservation on new output", lambda: reserved_bar(added))
                assert all(
                    result["success"] for result in sway("output", added, "mode", "1024x768")
                )
                wait_for(
                    "resized output",
                    lambda: any(
                        output["name"] == added and output["rect"]["width"] == 1024
                        for output in active_outputs()
                    ),
                )
                assert all(result["success"] for result in sway("output", added, "disable"))
                wait_for(
                    "disabled output removed from active model", lambda: len(active_outputs()) == 1
                )
                wait_for("surviving bar after output changes", lambda: reserved_bar(original))
                ready, writer = os.pipe()
                try:
                    log_path = root / "swaylock.log"
                    log = log_path.open("w")
                    logs.append((log_path, log))
                    locker = subprocess.Popen(
                        ["swaylock", "--ready-fd", str(writer), "--color", "000000"],
                        env=env,
                        stdin=subprocess.DEVNULL,
                        stdout=log,
                        stderr=subprocess.STDOUT,
                        pass_fds=(writer,),
                    )
                    processes.append(locker)
                    os.close(writer)
                    writer = None
                    assert select.select([ready], [], [], 20)[0], (
                        "locker must acknowledge acquisition"
                    )
                    assert os.read(ready, 1) == b"\n", "locker must acquire the native session lock"
                    assert locker.poll() is None, "locker must stay alive after acquisition"
                finally:
                    os.close(ready)
                    if writer is not None:
                        os.close(writer)
                print(
                    "Software Wayland acceptance passed: bar reservations, workspace, output add/resize/disable, session lock"
                )
        finally:
            for process in reversed(processes):
                if process.poll() is None:
                    process.terminate()

            def stop_process(process):
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)

            for process in reversed(processes):
                stop_process(process)
            for path, log in logs:
                log.close()
                print(f"--- {path.name} ---\n{path.read_text(errors='replace')}")
            for path in root.rglob("hyprland.log"):
                print(path.read_text(errors="replace"))


if __name__ == "__main__":
    main()
