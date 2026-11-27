"""Shared module loaders and constants for CI regression tests."""

from pathlib import Path
import runpy

SCRIPTS = Path(__file__).resolve().parents[1]
REPO_ROOT = SCRIPTS.parents[2]
POLICY = runpy.run_path(str(SCRIPTS / "classify-ci-changes.py"))
CONTRACT = runpy.run_path(str(SCRIPTS / "check-ci-contract.py"))
PRE_PUSH = runpy.run_path(str(SCRIPTS / "pre-push.py"))
MUTATION = runpy.run_path(str(SCRIPTS / "select-mutations.py"))
MUTATION_RUN = runpy.run_path(str(SCRIPTS / "run-mutations.py"))
MUTATION_REPORT = runpy.run_path(str(SCRIPTS / "report-mutations.py"))
classify = POLICY["classify"]
SCRIPTS_PREFIX = POLICY["SCRIPTS"]
LINUX = POLICY["LINUX"]
WINDOWS = POLICY["WINDOWS"]
