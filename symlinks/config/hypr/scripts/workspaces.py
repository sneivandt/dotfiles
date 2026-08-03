#!/usr/bin/env python3
"""Waybar workspace dots for Hyprland.

Waybar 0.15's hyprland/workspaces module switches workspaces by sending the
legacy "dispatch workspace N" request over the Hyprland IPC socket. Hyprland
0.56 replaced that with a Lua dispatch protocol and rejects the old form, but
Waybar never inspects the reply, so clicks are swallowed with no visible error.
Rendering the dots as per-workspace custom modules lets each one dispatch the
Lua equivalent from its own on-click instead.

Usage:
  workspaces.py <n>   Emit waybar JSON for workspace n.
  workspaces.py --watch
                      Signal waybar (SIGRTMIN+1) whenever workspaces change.
"""

import json
import os
import signal
import socket
import subprocess
import sys

ACTIVE_ICON = "\u25cf"  # ●
OCCUPIED_ICON = "\u25cb"  # ○
# Matches the "signal" of the custom/wsN modules in ~/.config/waybar/config.
WAYBAR_SIGNAL_ID = 1
WAYBAR_SIGNAL = signal.SIGRTMIN + WAYBAR_SIGNAL_ID
LAST_WORKSPACE = 9


def hyprctl(*args: str):
    try:
        out = subprocess.check_output(["hyprctl", "-j", *args], text=True)
        return json.loads(out)
    except (
        subprocess.CalledProcessError,
        FileNotFoundError,
        json.JSONDecodeError,
    ):
        return None


def active_id() -> int | None:
    workspace = hyprctl("activeworkspace")
    if not isinstance(workspace, dict):
        return None
    ident = workspace.get("id")
    return ident if isinstance(ident, int) else None


def occupied_ids() -> set[int]:
    workspaces = hyprctl("workspaces")
    if not isinstance(workspaces, list):
        return set()
    return {
        ws["id"]
        for ws in workspaces
        if isinstance(ws, dict) and isinstance(ws.get("id"), int)
    }


def emit(workspace: int) -> None:
    if workspace == active_id():
        payload = {"text": ACTIVE_ICON, "class": "active", "alt": "active"}
    elif workspace in occupied_ids():
        payload = {"text": OCCUPIED_ICON, "class": "occupied", "alt": "occupied"}
    else:
        # Waybar hides a module whose text is empty, matching the old
        # hyprland/workspaces behaviour of only showing live workspaces.
        payload = {"text": "", "class": "empty", "alt": "empty"}
    print(json.dumps(payload), flush=True)


def waybar_pids() -> list[int]:
    try:
        out = subprocess.check_output(["pidof", "waybar"], text=True).strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return []
    return [int(pid) for pid in out.split() if pid.isdigit()]


def notify_waybar() -> None:
    for pid in waybar_pids():
        try:
            os.kill(pid, WAYBAR_SIGNAL)
        except ProcessLookupError:
            pass


def watch() -> int:
    sig = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if not sig or not runtime:
        return 0

    sock_path = f"{runtime}/hypr/{sig}/.socket2.sock"
    triggers = (
        "workspace>>",
        "workspacev2>>",
        "createworkspace>>",
        "createworkspacev2>>",
        "destroyworkspace>>",
        "destroyworkspacev2>>",
        "moveworkspace>>",
        "moveworkspacev2>>",
        "focusedmon>>",
        "openwindow>>",
        "closewindow>>",
        "movewindow>>",
        "movewindowv2>>",
    )

    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.connect(sock_path)
            buf = b""
            while True:
                data = sock.recv(4096)
                if not data:
                    break
                buf += data
                while b"\n" in buf:
                    raw, buf = buf.split(b"\n", 1)
                    if raw.decode("utf-8", "replace").startswith(triggers):
                        notify_waybar()
    except OSError:
        return 0
    return 0


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    if argv[0] == "--watch":
        return watch()
    if argv[0].isdigit() and 1 <= int(argv[0]) <= LAST_WORKSPACE:
        emit(int(argv[0]))
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
