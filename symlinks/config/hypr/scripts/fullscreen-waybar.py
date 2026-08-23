#!/usr/bin/env python3
"""Hide waybar on fullscreen, show otherwise. Listens to Hyprland IPC."""

import json
import os
import socket
import subprocess
import sys


def toggle_waybar() -> bool:
    try:
        subprocess.run(
            [
                "systemctl",
                "--user",
                "kill",
                "--signal=SIGUSR1",
                "--kill-whom=main",
                "waybar.service",
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except (subprocess.CalledProcessError, FileNotFoundError):
        return False
    return True


def set_hidden(hidden: bool, current: dict) -> None:
    if current["hidden"] != hidden and toggle_waybar():
        current["hidden"] = hidden


def active_workspace_fullscreen() -> bool | None:
    try:
        out = subprocess.check_output(
            ["hyprctl", "-j", "activeworkspace"], text=True
        )
        workspace = json.loads(out)
    except (
        subprocess.CalledProcessError,
        FileNotFoundError,
        json.JSONDecodeError,
    ):
        return None
    if not isinstance(workspace, dict):
        return None
    fullscreen = workspace.get("hasfullscreen")
    return fullscreen if isinstance(fullscreen, bool) else None


def update(state: dict) -> None:
    fullscreen = active_workspace_fullscreen()
    if fullscreen is not None:
        set_hidden(fullscreen, state)


def main() -> int:
    sig = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if not sig or not runtime:
        print(
            "fullscreen-waybar: Hyprland IPC environment is unavailable",
            file=sys.stderr,
        )
        return 1

    sock_path = f"{runtime}/hypr/{sig}/.socket2.sock"
    state = {"hidden": False}
    triggers = (
        "fullscreen>>",
        "workspace>>",
        "workspacev2>>",
        "focusedmon>>",
        "openwindow>>",
        "closewindow>>",
    )

    update(state)

    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.connect(sock_path)
            buf = b""
            while True:
                data = sock.recv(4096)
                if not data:
                    print(
                        "fullscreen-waybar: Hyprland IPC stream closed",
                        file=sys.stderr,
                    )
                    return 1
                buf += data
                while b"\n" in buf:
                    raw, buf = buf.split(b"\n", 1)
                    line = raw.decode("utf-8", "replace")
                    if line.startswith(triggers):
                        update(state)
    except OSError as error:
        print(f"fullscreen-waybar: Hyprland IPC failed: {error}", file=sys.stderr)
        return 1
    finally:
        set_hidden(False, state)


if __name__ == "__main__":
    sys.exit(main())
