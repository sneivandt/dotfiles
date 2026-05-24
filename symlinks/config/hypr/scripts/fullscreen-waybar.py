#!/usr/bin/env python3
"""Hide waybar on fullscreen, show otherwise. Listens to Hyprland IPC."""

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
    if current["hidden"] == hidden:
        return
    for pid in waybar_pids():
        try:
            os.kill(pid, signal.SIGUSR1)
        except ProcessLookupError:
            pass
    current["hidden"] = hidden


def main() -> int:
    sig = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if not sig or not runtime:
        return 0

    sock_path = f"{runtime}/hypr/{sig}/.socket2.sock"
    state = {"hidden": False}

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
                    line = raw.decode("utf-8", "replace")
                    if line.startswith("fullscreen>>"):
                        set_hidden(line.endswith(">>1"), state)
                    elif line.startswith(("workspace>>", "focusedmon>>")):
                        set_hidden(False, state)
    except OSError:
        return 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
