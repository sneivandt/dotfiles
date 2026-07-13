#!/usr/bin/env python3
"""Hide waybar on fullscreen, show otherwise. Listens to Hyprland IPC."""

import json
import os
import signal
import socket
import subprocess
import sys


def waybar_pids() -> list[int]:
    try:
        out = subprocess.check_output(["pidof", "waybar"], text=True).strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return []
    return [int(p) for p in out.split() if p.isdigit()]


def set_hidden(hidden: bool, current: dict) -> None:
    pids = set(waybar_pids())
    previous_pids = current["pids"]
    targets = {
        pid
        for pid in pids
        if (current["hidden"] if pid in previous_pids else False) != hidden
    }
    for pid in targets:
        try:
            os.kill(pid, signal.SIGUSR1)
        except ProcessLookupError:
            pass
    current["hidden"] = hidden
    current["pids"] = pids


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
        return 0

    sock_path = f"{runtime}/hypr/{sig}/.socket2.sock"
    state = {"hidden": False, "pids": set()}
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
            sock.settimeout(1.0)
            buf = b""
            while True:
                try:
                    data = sock.recv(4096)
                except socket.timeout:
                    # A restarted Waybar starts visible; hide newly seen PIDs.
                    set_hidden(state["hidden"], state)
                    continue
                if not data:
                    break
                buf += data
                while b"\n" in buf:
                    raw, buf = buf.split(b"\n", 1)
                    line = raw.decode("utf-8", "replace")
                    if line.startswith(triggers):
                        update(state)
    except OSError:
        return 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
