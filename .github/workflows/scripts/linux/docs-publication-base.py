#!/usr/bin/env python3
"""Find a verified Pages baseline; rebuild conservatively if it is unavailable."""

import json
import os
from pathlib import Path
import re
import subprocess


def api(endpoint):
    return json.loads(subprocess.check_output(["gh", "api", endpoint], text=True))


def publication_base(repository, head):
    deployments = api(f"repos/{repository}/deployments?environment=github-pages&per_page=100")
    for deployment in deployments:
        statuses = api(f"repos/{repository}/deployments/{int(deployment['id'])}/statuses?per_page=100")
        if not any(status["state"] == "success" for status in statuses):
            continue
        sha = deployment["sha"]
        if not re.fullmatch(r"[0-9a-f]{40}", sha):
            return ""
        ancestry = subprocess.run(
            ["git", "merge-base", "--is-ancestor", sha, head],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        # Do not use an older ancestor when a force push has left an unrelated
        # successful deployment live. Its site inputs may differ from both tips.
        return sha if ancestry.returncode == 0 else ""
    return ""


def main():
    try:
        base = publication_base(os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_SHA"])
    except (subprocess.SubprocessError, OSError, ValueError, KeyError, TypeError):
        # An unavailable deployment baseline is never permission to skip work.
        print("Pages baseline unavailable; selecting a documentation site build.")
        base = ""
    with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as stream:
        stream.write(f"base_sha={base}\n")
    print(f"Published documentation baseline: {base or 'none; rebuild required'}")


if __name__ == "__main__":
    main()
