#!/usr/bin/env python3
"""Regression fixtures for malformed, ambiguous, and mismatched skill metadata."""

from pathlib import Path
import runpy
import tempfile
import unittest

CHECKER = runpy.run_path(str(Path(__file__).with_name("check-skills.py")))
metadata = CHECKER["metadata"]
check = CHECKER["check"]
VALID = "---\nname: example-skill\ndescription: >\n  Use for example tasks.\n  Not for unrelated work.\n---\n\n# Example\n"


class MetadataTests(unittest.TestCase):
    def test_supported_yaml_blocks(self):
        for style in (">", ">-", ">+", "|", "|-", "|+"):
            with self.subTest(style=style):
                fields = metadata(VALID.replace("description: >", "description: " + style))
                self.assertEqual(fields["name"], "example-skill")
                self.assertIn("Not for unrelated work.", fields["description"])

    def test_rejects_invalid_or_unsupported_frontmatter(self):
        cases = {
            "opening": VALID.removeprefix("---\n"),
            "closing": VALID.replace("\n---\n\n#", "\n\n#"),
            "missing name": VALID.replace("name: example-skill\n", ""),
            "duplicate key": VALID.replace("name: example-skill", "name: example-skill\nname: other-skill"),
            "empty description": "---\nname: example-skill\ndescription: >\n  \n---\n",
            "missing description": "---\nname: example-skill\n---\n",
            "mapping description": VALID.replace("description: >", "description: {use: example}"),
            "unterminated quote": VALID.replace("name: example-skill", 'name: "example-skill'),
            "alias": VALID.replace("name: example-skill", "name: *example"),
            "uppercase": VALID.replace("example-skill", "Example-skill"),
            "boolean name": VALID.replace("example-skill", "true"),
            "numeric name": VALID.replace("example-skill", "0xff"),
            "unknown key": VALID.replace("name: example-skill", "other: example-skill"),
            "bad indentation": VALID.replace("  Use", " Use"),
            "tab indentation": VALID.replace("  Use", "\tUse"),
            "inconsistent indentation": VALID.replace("  Use", "    Use"),
        }
        for label, text in cases.items():
            with self.subTest(label=label), self.assertRaises(ValueError):
                metadata(text)

    def test_directory_identity_and_duplicate_names(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def skill(name, text):
                path = root / ".agents/skills" / name / "SKILL.md"
                path.parent.mkdir(parents=True)
                path.write_text(text, encoding="utf-8")
            skill("example-skill", VALID)
            self.assertEqual(check(root), [])
            skill("other-skill", VALID)
            errors = check(root)
            self.assertTrue(any("does not match directory" in error for error in errors), errors)
            self.assertTrue(any("duplicate skill name" in error for error in errors), errors)
            skill("broken-skill", "not frontmatter")
            self.assertTrue(any("broken-skill/SKILL.md" in error for error in check(root)))

    def test_missing_inventory_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertIn("no skill entry points", check(Path(directory))[0])


if __name__ == "__main__":
    unittest.main()
