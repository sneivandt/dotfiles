#!/usr/bin/env python3
"""Disable resize_on_border when the focused workspace's window fills it.

A window "fills" the workspace when it's fullscreen, or when it's the only
tiled window on the workspace (with no border/gaps via the f[1]/w[tv1]
workspace rules). In those cases the border-resize cursor is misleading
because there's nothing to resize against. Listens to Hyprland IPC and
toggles general:resize_on_border via hyprctl accordingly.
"""

import json
import os
import socket
import subprocess
import sys


def hyprctl_json(args: list[str]) -> object | None:
    try:
        out = subprocess.check_output(["hyprctl", "-j", *args], text=True)
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    try:
        return json.loads(out)
    except json.JSONDecodeError:
        return None


def set_resize_on_border(enabled: bool, state: dict) -> None:
    if state.get("enabled") == enabled:
        return
    value = "true" if enabled else "false"
    try:
        subprocess.run(
            ["hyprctl", "--batch",
             f"keyword general:resize_on_border {value} ; "
             f"keyword general:hover_icon_on_border {value}"],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except FileNotFoundError:
        return
    state["enabled"] = enabled


def workspace_fills(state: dict) -> bool:
    ws = hyprctl_json(["activeworkspace"])
    if not isinstance(ws, dict):
        return False
    ws_id = ws.get("id")
    clients = hyprctl_json(["clients"])
    if not isinstance(clients, list):
        return False

    tiled = 0
    for c in clients:
        if not isinstance(c, dict):
            continue
        c_ws = c.get("workspace") or {}
        if c_ws.get("id") != ws_id:
            continue
        if c.get("hidden"):
            continue
        # fullscreen: 0 = none, 1 = maximize, 2 = fullscreen
        if c.get("fullscreen", 0) >= 2:
            return True
        if not c.get("floating"):
            tiled += 1

    return tiled <= 1


def update(state: dict) -> None:
    set_resize_on_border(not workspace_fills(state), state)


def main() -> int:
    sig = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if not sig or not runtime:
        return 1

    sock_path = f"{runtime}/hypr/{sig}/.socket2.sock"
    state: dict = {"enabled": None}

    triggers = (
        "openwindow>>",
        "closewindow>>",
        "movewindow>>",
        "movewindowv2>>",
        "changefloatingmode>>",
        "fullscreen>>",
        "workspace>>",
        "workspacev2>>",
        "focusedmon>>",
        "activewindow>>",
        "activewindowv2>>",
    )

    update(state)

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
                line = raw.decode("utf-8", "replace")
                if line.startswith(triggers):
                    update(state)
    return 0


if __name__ == "__main__":
    sys.exit(main())
