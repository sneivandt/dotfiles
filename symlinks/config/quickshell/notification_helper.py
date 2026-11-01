"""Read Mako 1.11+ notification snapshots without changing daemon state."""

import json
import subprocess
import sys
from html.parser import HTMLParser


class PlainText(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.parts = []

    def handle_data(self, data):
        self.parts.append(data)

    def handle_starttag(self, tag, attrs):
        if tag in ("br", "p", "div"):
            self.parts.append("\n")


def body_text(value):
    parser = PlainText()
    parser.feed(value)
    return "".join(parser.parts).strip()


def read_notifications(command):
    result = subprocess.run(
        ["makoctl", command, "-j"], capture_output=True, text=True,
        timeout=3, check=True,
    )
    entries = json.loads(result.stdout)
    if not isinstance(entries, list) or any(not isinstance(e, dict) for e in entries):
        raise ValueError("Invalid notification list")
    return [
        {
            "app": str(entry.get("app_name") or "Notification"),
            "summary": str(entry.get("summary") or "Notification"),
            "body": body_text(str(entry.get("body") or "")),
            "critical": entry.get("urgency") == "critical",
        }
        for entry in entries[:100]
    ]


def snapshot():
    try:
        # Publish both lists together; a failed query must not look like an empty inbox.
        active = read_notifications("list")
        history = read_notifications("history")
        return {"ok": True, "active": active, "history": history}
    except (OSError, subprocess.SubprocessError, ValueError):
        # Never echo subprocess output: it can contain private notification text.
        return {"ok": False, "error": "Notifications unavailable. Check that Mako 1.11 or newer is running."}


if __name__ == "__main__":
    result = snapshot()
    print(json.dumps(result))
    sys.exit(0 if result["ok"] else 1)
