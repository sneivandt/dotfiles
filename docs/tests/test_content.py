"""Isolated source-link and page metadata regressions; no network or browser."""

from pathlib import Path
import runpy
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[2] / ".github/workflows/scripts/linux"
LINKS = runpy.run_path(str(SCRIPTS / "check-doc-links.py"))
SITE = runpy.run_path(str(SCRIPTS / "check-doc-site.py"))


class SourceLinkTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="docs-links-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "docs").mkdir()
        (self.root / "docs/target with spaces.md").write_text("# Target\n## Details\n", encoding="utf-8")
        (self.root / "source.rs").write_text("first\nsecond\n", encoding="utf-8")

    def check_link(self, target):
        (self.root / "guide.md").write_text(f"[Source](<{target}>)\n", encoding="utf-8")
        return LINKS["check"](self.root)

    def test_repository_files_directories_anchors_and_line_ranges(self):
        prefix = "https://github.com/sneivandt/dotfiles/"
        for target in (
            "blob/main/docs/target%20with%20spaces.md#details",
            "blob/main/docs/target%20with%20spaces.md#L1-L2",
            "tree/main/docs",
            "blob/main/source.rs#L1-L2",
        ):
            with self.subTest(target=target):
                self.assertEqual(self.check_link(prefix + target), [])
        for target, expected in (
            ("blob/main/moved.rs", "broken repository source"),
            ("blob/main/docs", "broken repository source"),
            ("tree/main/source.rs", "broken repository source"),
            ("blob/main/docs/target%20with%20spaces.md#missing", "broken repository anchor"),
            ("blob/main/docs/target%20with%20spaces.md#L3", "broken repository line range"),
            ("blob/main/source.rs#L0", "broken repository line range"),
            ("blob/main/source.rs#L3", "broken repository line range"),
            ("blob/main/source.rs#L2-L1", "broken repository line range"),
            ("blob/main/%2e%2e/outside.md", "invalid repository source"),
        ):
            with self.subTest(target=target):
                errors = self.check_link(prefix + target)
                self.assertEqual(len(errors), 1, errors)
                self.assertIn(expected, errors[0])

    def test_other_repositories_revisions_and_upstream_urls_are_not_local(self):
        for target in (
            "https://github.com/another/repository/blob/main/missing.md",
            "https://github.com/sneivandt/dotfiles/blob/v1/missing.md",
            "https://example.invalid/missing",
            "mailto:",
        ):
            with self.subTest(target=target):
                self.assertEqual(self.check_link(target), [])

    def test_source_rename_is_detected_without_network(self):
        target = "https://github.com/sneivandt/dotfiles/blob/main/source.rs"
        self.assertEqual(self.check_link(target), [])
        (self.root / "source.rs").rename(self.root / "renamed.rs")
        self.assertIn("broken repository source", self.check_link(target)[0])


class FrontmatterTests(unittest.TestCase):
    def fields(self, header):
        errors = []
        fields = SITE["frontmatter_fields"](header + "\n# Guide\n", "guide.md", errors)
        return fields, errors

    def test_required_fields_are_nonempty_unique_single_line_scalars(self):
        fields, errors = self.fields("---\ntitle: Guide\ndescription: Read & act.\n---")
        self.assertEqual(errors, [])
        self.assertEqual(fields, {"title": "Guide", "description": "Read & act."})
        for header, key in (
            ("# No frontmatter", "title"),
            ("---\ntitle: Guide\n---", "description"),
            ("---\ntitle: Guide\ndescription: ''\n---", "description"),
            ("---\ntitle: Guide\ndescription: |\n  Multiline.\n---", "description"),
            ("---\ntitle: Guide\ndescription: First\ndescription: Second\n---", "description"),
            ("---\ntitle: \ndescription: Read.\n---", "title"),
        ):
            with self.subTest(header=header):
                _, failures = self.fields(header)
                self.assertTrue(any(f"frontmatter {key}" in error for error in failures), failures)

    def test_homepage_and_404_keep_metadata_without_requiring_matching_routes(self):
        for name, title in (("index.md", "About this project"), ("404.md", "Page not found")):
            errors = []
            fields = SITE["frontmatter_fields"](
                f"---\ntitle: {title}\ndescription: A useful description.\n---\n",
                name, errors,
            )
            self.assertEqual(errors, [])
            self.assertEqual(fields["title"], title)

    def test_rendered_metadata_decodes_html_entities(self):
        page = SITE["Page"]('<meta name="description" content="Read &amp; act.">')
        self.assertEqual(page.descriptions, ["Read & act."])


if __name__ == "__main__":
    unittest.main()
