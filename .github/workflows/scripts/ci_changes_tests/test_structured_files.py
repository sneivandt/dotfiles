"""Tests for zero-dependency structured file validation (JSON/JSONC/TOML/YAML)."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import unittest

from ci_changes_tests.helpers import SCRIPTS


def _load_validator():
    path = SCRIPTS / "check-structured-files.py"
    spec = importlib.util.spec_from_file_location("check_structured_files", path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


VALIDATOR = _load_validator()


class StructuredFileValidationTests(unittest.TestCase):
    def test_strict_json_rejects_comments_trailing_commas_and_duplicate_keys(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            valid = root / "data.json"
            valid.write_text('{"a": 1, "url": "https://example.com//path"}\n', encoding="utf-8")
            self.assertIsNone(VALIDATOR.validate_file(valid, "data.json"))

            commented = root / "commented.json"
            commented.write_text(
                '{\n  // not allowed in strict json\n  "a": 1\n}\n', encoding="utf-8"
            )
            self.assertIsNotNone(VALIDATOR.validate_file(commented, "commented.json"))

            trailing = root / "trailing.json"
            trailing.write_text('{"a": 1,}\n', encoding="utf-8")
            self.assertIsNotNone(VALIDATOR.validate_file(trailing, "trailing.json"))

            duplicate = root / "dup.json"
            duplicate.write_text('{"a": 1, "a": 2}\n', encoding="utf-8")
            error = VALIDATOR.validate_file(duplicate, "dup.json")
            self.assertIsNotNone(error)
            self.assertIn("duplicate JSON key", error)

    def test_allowlisted_jsonc_permits_comments_and_trailing_commas(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            settings = root / ".vscode" / "settings.json"
            settings.parent.mkdir(parents=True)
            settings.write_text(
                '{\n  // comment\n  "url": "https://example.com//not-a-comment",\n'
                '  /* block\n     comment */\n  "list": [1, 2,],\n}\n',
                encoding="utf-8",
            )
            self.assertIsNone(VALIDATOR.validate_file(settings, ".vscode/settings.json"))

            settings.write_text('{\n  "k": 1,\n  "k": 2\n}\n', encoding="utf-8")
            self.assertIsNotNone(VALIDATOR.validate_file(settings, ".vscode/settings.json"))

    def test_toml_and_yaml_validation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            toml_file = root / "conf.toml"
            toml_file.write_text("[section]\nkey = 1\n", encoding="utf-8")
            self.assertIsNone(VALIDATOR.validate_file(toml_file, "conf.toml"))
            toml_file.write_text("[section\nkey = 1\n", encoding="utf-8")
            self.assertIsNotNone(VALIDATOR.validate_file(toml_file, "conf.toml"))

            yaml_file = root / "config.yml"
            yaml_file.write_text("name: test\nitems:\n  - one\n", encoding="utf-8")
            self.assertIsNone(VALIDATOR.validate_file(yaml_file, "config.yml"))
            yaml_file.write_text("name: one\nname: two\n", encoding="utf-8")
            self.assertIsNotNone(VALIDATOR.validate_file(yaml_file, "config.yml"))

    def test_xml_validation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fonts = root / "symlinks" / "config" / "fontconfig" / "fonts.conf"
            fonts.parent.mkdir(parents=True)
            fonts.write_text(
                '<?xml version="1.0"?>\n'
                '<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">\n'
                "<fontconfig><alias><family>monospace</family></alias></fontconfig>\n",
                encoding="utf-8",
            )
            self.assertIsNone(
                VALIDATOR.validate_file(fonts, "symlinks/config/fontconfig/fonts.conf")
            )

            nuget = root / "symlinks" / "config" / "nuget" / "nuget.config"
            nuget.parent.mkdir(parents=True)
            nuget.write_text(
                "<configuration><packageSources /></configuration>\n", encoding="utf-8"
            )
            self.assertIsNone(VALIDATOR.validate_file(nuget, "symlinks/config/nuget/nuget.config"))
            self.assertEqual(VALIDATOR.main(["--root", str(root)]), 0)

            nuget.write_text("<configuration><unclosed></configuration>\n", encoding="utf-8")
            self.assertIsNotNone(
                VALIDATOR.validate_file(nuget, "symlinks/config/nuget/nuget.config")
            )
            self.assertEqual(VALIDATOR.main(["--root", str(root)]), 1)

            generic_xml = root / "sample.xml"
            generic_xml.write_text('<root a="1" a="2" />\n', encoding="utf-8")
            self.assertIsNotNone(VALIDATOR.validate_file(generic_xml, "sample.xml"))

    def test_root_discovery_skips_generated_build_trees(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "valid.json").write_text('{"ok": true}\n', encoding="utf-8")
            ignored = root / "cli" / "target" / "broken.json"
            ignored.parent.mkdir(parents=True)
            ignored.write_text("{broken", encoding="utf-8")
            self.assertEqual(VALIDATOR.main(["--root", str(root)]), 0)
