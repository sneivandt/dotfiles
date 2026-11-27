#!/usr/bin/env python3
"""Isolated regressions for CI selection, result gates, and release catch-up."""

from pathlib import Path
import sys
import unittest

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
from ci_changes_tests.test_structured_files import StructuredFileValidationTests

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
    "StructuredFileValidationTests",
]

if __name__ == "__main__":
    unittest.main()
