#!/usr/bin/env python3
"""Emit waybar JSON for the active Hyprland window; updates instantly via IPC."""

import json
import os
import socket
import subprocess
import sys


def emit() -> None:
    try:
        out = subprocess.check_output(["hyprctl", "activewindow", "-j"], text=True)
        title = json.loads(out).get("title", "") or ""
    except (subprocess.CalledProcessError, json.JSONDecodeError):
        title = ""

    if not title:
        payload = {"text": "", "class": "empty", "alt": "empty"}
    else:
        payload = {"text": title[:80], "class": "active", "alt": "active"}
    print(json.dumps(payload), flush=True)


def main() -> int:
    emit()

    sig = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if not sig or not runtime:
        return 0

    sock_path = f"{runtime}/hypr/{sig}/.socket2.sock"
    triggers = ("activewindow>>", "closewindow>>", "openwindow>>",
                "workspace>>", "focusedmon>>")

    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.connect(sock_path)
        buf = b""
        while True:
            data = sock.recv(4096)
            if not data:
                break
            buf += data
            while b"\n" in buf:
                line, buf = buf.split(b"\n", 1)
                if line.decode("utf-8", "replace").startswith(triggers):
                    emit()
    return 0


if __name__ == "__main__":
    sys.exit(main())
