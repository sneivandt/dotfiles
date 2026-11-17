#!/usr/bin/env python3
"""Audit docs dependencies with one temporary, advisory-specific exception."""

from datetime import datetime, timezone
import json
from pathlib import Path
import shutil
import subprocess
import sys


ADVISORY = "https://github.com/advisories/GHSA-fc5q-39xc-vjpf"
EXPIRES = datetime(2026, 11, 24, 19, tzinfo=timezone.utc)


def audit_findings(report, returncode, now):
    """Return accepted and blocking package names; reject incomplete reports."""
    if returncode not in (0, 1):
        raise ValueError(f"npm audit failed with exit code {returncode}")
    if not isinstance(report, dict) or "error" in report or report.get("auditReportVersion") != 2:
        raise ValueError("npm audit did not return a supported, successful report")
    vulnerabilities = report.get("vulnerabilities")
    metadata = report.get("metadata")
    if not isinstance(vulnerabilities, dict) or not isinstance(metadata, dict):
        raise ValueError("npm audit report is missing vulnerability data")
    totals = metadata.get("vulnerabilities")
    if (not isinstance(totals, dict) or type(totals.get("total")) is not int
            or totals["total"] != len(vulnerabilities)
            or returncode != int(bool(vulnerabilities))):
        raise ValueError("npm audit status and vulnerability counts disagree")
    for name, finding in vulnerabilities.items():
        if (not isinstance(finding, dict) or finding.get("name") != name
                or not isinstance(finding.get("via"), list) or not finding["via"]):
            raise ValueError(f"npm audit returned an incomplete finding for {name}")

    def excepted(name, ancestors):
        if name in ancestors or name not in vulnerabilities:
            return False
        for cause in vulnerabilities[name]["via"]:
            if isinstance(cause, str):
                if not excepted(cause, ancestors | {name}):
                    return False
            elif isinstance(cause, dict):
                if not (
                    name == cause.get("name") == cause.get("dependency") == "picomatch"
                    and cause.get("url") == ADVISORY
                    and cause.get("range") == "<=4.0.7"
                    and cause.get("severity") == "high"
                ):
                    return False
            else:
                return False
        return True

    accepted = []
    blocked = []
    for name in sorted(vulnerabilities):
        if now < EXPIRES and excepted(name, set()):
            accepted.append(name)
        else:
            blocked.append(name)
    return accepted, blocked


def main():
    root = Path(__file__).resolve().parents[4]
    npm = shutil.which("npm.cmd" if sys.platform == "win32" else "npm")
    if npm is None:
        raise RuntimeError("npm is unavailable; install the Node version in docs/.node-version")
    result = subprocess.run(
        [npm, "audit", "--package-lock-only", "--json"],
        cwd=root / "docs", capture_output=True, text=True, timeout=120, check=False,
    )
    print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    accepted, blocked = audit_findings(
        json.loads(result.stdout), result.returncode, datetime.now(timezone.utc),
    )
    if accepted:
        print(
            f"WARNING: temporarily accepting {ADVISORY} until {EXPIRES.isoformat()} "
            f"for {', '.join(accepted)}. The weekly security audit remains strict.",
            file=sys.stderr,
        )
    if blocked:
        raise RuntimeError(
            "Unaccepted or expired dependency findings: " + ", ".join(blocked)
        )
    print("Documentation dependency audit passed" + (" with the temporary exception." if accepted else "."))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"Documentation dependency audit failed: {error}", file=sys.stderr)
        sys.exit(1)
