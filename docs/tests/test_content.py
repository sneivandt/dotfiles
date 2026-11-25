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
        for name, title in (("index.md", "About these dotfiles"), ("404.md", "Page not found")):
            errors = []
            fields = SITE["frontmatter_fields"](
                f"---\ntitle: {title}\ndescription: A useful description.\n---\n",
                name, errors,
            )
            self.assertEqual(errors, [])
            self.assertEqual(fields["title"], title)

    def test_rendered_metadata_decodes_html_entities(self):
        page = SITE["Page"](
            '<meta name="author" content="Stuart Neivandt">'
            '<meta name="theme-color" content="#1a1b26">'
            '<meta name="robots" content="noindex, follow">'
            '<meta name="description" content="Read &amp; act.">'
            '<meta property="og:type" content="article">'
            '<meta property="og:image" content="https://dotfiles.stuartneivandt.com/assets/og-image.png">'
            '<meta name="twitter:card" content="summary_large_image">'
            '<meta name="twitter:image" content="https://dotfiles.stuartneivandt.com/assets/og-image.png">'
            '<link rel="apple-touch-icon" href="/apple-touch-icon.png">'
        )
        self.assertEqual(page.authors, ["Stuart Neivandt"])
        self.assertEqual(page.theme_colors, ["#1a1b26"])
        self.assertEqual(page.robots, ["noindex, follow"])
        self.assertEqual(page.descriptions, ["Read & act."])
        self.assertEqual(page.og_types, ["article"])
        self.assertEqual(page.og_images, ["https://dotfiles.stuartneivandt.com/assets/og-image.png"])
        self.assertEqual(page.twitter_cards, ["summary_large_image"])
        self.assertEqual(page.twitter_images, ["https://dotfiles.stuartneivandt.com/assets/og-image.png"])
        self.assertEqual(page.apple_touch_icons, ["/apple-touch-icon.png"])
        self.assertIn("/apple-touch-icon.png", page.assets)
        self.assertIn("https://dotfiles.stuartneivandt.com/assets/og-image.png", page.assets)

    def test_static_doc_structure_checks_detect_boxed_code_edit_links_and_task_sections(self):
        with tempfile.TemporaryDirectory(prefix="docs-structure-") as temp:
            root = Path(temp)
            (root / "docs/content/guides").mkdir(parents=True)
            (root / "docs/content/reference").mkdir(parents=True)
            guide = root / "docs/content/guides/contributing.md"
            guide.write_text("# Contributing\n", encoding="utf-8")
            task_ref = root / "docs/content/reference/task-reference.md"
            task_ref.write_text("# Task reference\n", encoding="utf-8")
            task_doc = root / "docs/content/reference/agent-packages.md"
            task_doc.write_text(
                "---\ntitle: Agent packages\ndescription: Manage.\n---\n"
                "| Selector | `agent-packages` |\n| Commands | `update` |\n",
                encoding="utf-8",
            )
            pages = {
                "/contributing.html": SITE["Page"](
                    '<div class="vp-doc"><div>'
                    '<a href="/guide"><code>boxed</code></a>'
                    '<a href="https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/x/SKILL.md">skill</a>'
                    '<h2 id="development-setup">Setup</h2>'
                    '<h2 id="2-find-the-owning-files">Find</h2>'
                    '</div></div>'
                    '<div class="edit-link"><a href="https://example.invalid/wrong">Edit</a></div>'
                ),
                "/task-reference.html": SITE["Page"](
                    '<div class="vp-doc"><div>'
                    '<table><tbody><tr><td><code>agent-packages</code></td>'
                    '<td><a href="./agent-packages">Agent packages</a></td></tr></tbody></table>'
                    '</div></div>'
                    '<div class="edit-link"><a href="https://github.com/sneivandt/dotfiles/edit/main/docs/content/reference/task-reference.md">Edit</a></div>'
                ),
                "/agent-packages.html": SITE["Page"](
                    '<nav class="VPDocAsideOutline"><a class="outline-link" href="#what-it-does">What</a></nav>'
                    '<div class="vp-doc"><div>'
                    '<table><tbody><tr><td>wrong</td></tr></tbody></table>'
                    '<h2 id="what-it-does">What</h2>'
                    '</div></div>'
                    '<div class="edit-link"><a href="https://github.com/sneivandt/dotfiles/edit/main/docs/content/reference/agent-packages.md">Edit</a></div>'
                ),
            }
            errors = []
            SITE["check_doc_structure"](root, [guide, task_ref, task_doc], pages, errors)
            joined = "\n".join(errors)
            self.assertIn("must not wrap inline code", joined)
            self.assertIn("must not assign repository skills", joined)
            self.assertIn("expected edit link", joined)
            self.assertIn("expected #2-find-the-owning-files before #development-setup", joined)
            self.assertIn("expected introductory paragraph immediately before first table", joined)
            self.assertIn("first table must mention selector 'agent-packages'", joined)
            self.assertIn("expected one h2#preview-and-apply", joined)
            self.assertIn("expected one outline link to #removal", joined)


if __name__ == "__main__":
    unittest.main()
