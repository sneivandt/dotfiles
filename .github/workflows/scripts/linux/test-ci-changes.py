#!/usr/bin/env python3
"""Isolated regressions for CI selection, result gates, and release catch-up."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ci_changes_tests.test_classification import ClassificationTests
from ci_changes_tests.test_gate import GateTests
from ci_changes_tests.test_git_range import GitRangeTests
from ci_changes_tests.test_mutations_and_docs import (
    DocumentationPublicationTests,
    MutationSelectionTests,
    ReleaseTagTests,
)
from ci_changes_tests.test_pre_push import (
    LocalCheckTests,
    PrePushCoordinatorTests,
    PrePushPlanTests,
)

__all__ = [
    "ClassificationTests",
    "DocumentationPublicationTests",
    "GateTests",
    "GitRangeTests",
    "LocalCheckTests",
    "MutationSelectionTests",
    "PrePushCoordinatorTests",
    "PrePushPlanTests",
    "ReleaseTagTests",
]

if __name__ == "__main__":
    unittest.main()
