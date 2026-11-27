#!/usr/bin/env python3
"""Check published routes, anchors, and local assets in the built docs site."""

from html.parser import HTMLParser
from pathlib import Path
import re
import runpy
import sys
from urllib.parse import unquote, urljoin, urlsplit
from xml.etree import ElementTree

HEADING_ORDER = (
    ("task-reference", "catalog-overview", "scheduling-model"),
    ("configuration-basics", "files", "making-a-configuration-change"),
    ("contributing", "2-find-the-owning-files", "development-setup"),
    ("testing", "before-pushing", "specialized-checks"),
    ("agent-configuration", "deployment-targets", "validate-preview-and-apply"),
    ("agent-configuration", "what-a-preview-can-show", "configuration-fragments"),
    ("package-management", "logs-and-troubleshooting", "custom-arch-packages"),
    ("contributing", "contribution-workflow", "git-hooks"),
    ("commands-and-usage", "update-lifecycle", "select-tasks"),
    ("commands-and-usage", "control-what-advances", "move-a-checkout"),
    ("testing", "baseline-and-prerequisites", "local-npm-tls-failures"),
)

NO_SKILL_READING_GUIDES = frozenset(
    {
        "contributing",
        "adding-a-task",
        "application-settings",
        "testing",
        "writing-documentation",
    }
)

EDIT_LINK_PREFIX = "https://github.com/sneivandt/dotfiles/edit/main/docs/content/"
VOID_TAGS = frozenset(
    {
        "area",
        "base",
        "br",
        "col",
        "embed",
        "hr",
        "img",
        "input",
        "link",
        "meta",
        "param",
        "source",
        "track",
        "wbr",
    }
)


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__()
        self.ids = set()
        self.id_order = []
        self.duplicates = set()
        self.links = []
        self.assets = []
        self.h1_count = 0
        self.titles = []
        self.h2_ids = []
        self.nav_links = []
        self.outline_links = []
        self.edit_links = []
        self.boxed_code_links = []
        self.doc_skill_links = []
        self.doc_tables = []
        self.first_table_intro = None
        self.has_vp_doc = False
        self.site_title_links = []
        self.icons = []
        self.apple_touch_icons = []
        self.authors = []
        self.theme_colors = []
        self.robots = []
        self.canonicals = []
        self.descriptions = []
        self.og_types = []
        self.og_site_names = []
        self.og_locales = []
        self.og_urls = []
        self.og_images = []
        self.twitter_cards = []
        self.twitter_images = []
        self.in_title = False
        self.in_nav = False
        self.in_outline = False
        self.nav_link = None
        self.site_title_link = None
        self._depth = 0
        self._vp_doc_depth = None
        self._vp_doc_body_depth = None
        self._edit_link_depth = None
        self._doc_link_href = None
        self._doc_link_depth = None
        self._doc_link_has_code = False
        self._last_block = None
        self._active_p_text = None
        self._active_p_depth = None
        self._table = None
        self._table_depth = None
        self._in_tbody = False
        self._row = None
        self._row_code = None
        self._row_code_depth = None
        self.feed(text)

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        classes = set(attrs.get("class", "").split())
        is_void = tag in VOID_TAGS
        if not is_void:
            self._depth += 1
        depth = self._depth

        if identifier := attrs.get("id"):
            if identifier in self.ids:
                self.duplicates.add(identifier)
            self.ids.add(identifier)
            self.id_order.append(identifier)

        if self._vp_doc_depth is None and tag == "div" and "vp-doc" in classes:
            self.has_vp_doc = True
            self._vp_doc_depth = depth
        elif (
            self._vp_doc_depth is not None
            and self._vp_doc_body_depth is None
            and tag == "div"
            and depth == self._vp_doc_depth + 1
        ):
            self._vp_doc_body_depth = depth

        if self._edit_link_depth is None and tag == "div" and "edit-link" in classes:
            self._edit_link_depth = depth

        in_doc = self._vp_doc_depth is not None
        if in_doc and tag == "h2" and (identifier := attrs.get("id")):
            self.h2_ids.append(identifier)

        if (
            self._vp_doc_body_depth is not None
            and depth == self._vp_doc_body_depth + 1
            and self._table is None
        ):
            if tag == "p":
                self._active_p_text = []
                self._active_p_depth = depth
            elif tag == "table":
                if self.first_table_intro is None and not self.doc_tables:
                    if self._last_block and self._last_block[0] == "p":
                        self.first_table_intro = self._last_block[1]
                    else:
                        self.first_table_intro = ""
                self._last_block = ("table", "")
            else:
                self._last_block = (tag, "")

        if in_doc and tag == "table" and self._table is None:
            self._table = {"text": [], "rows": []}
            self._table_depth = depth
        elif self._table is not None:
            if tag == "tbody":
                self._in_tbody = True
            elif tag == "tr" and self._in_tbody and self._row is None:
                self._row = {"code": None, "href": None, "text": []}
            elif self._row is not None:
                if tag == "code" and self._row["code"] is None and self._row_code is None:
                    self._row_code = []
                    self._row_code_depth = depth
                elif tag == "a" and self._row["href"] is None and attrs.get("href"):
                    self._row["href"] = attrs["href"]

        if tag == "h1":
            self.h1_count += 1
            self.titles.append("")
            self.in_title = True
        if tag == "nav":
            if attrs.get("id") == "VPSidebarNav":
                self.in_nav = True
            if "VPDocAsideOutline" in classes:
                self.in_outline = True
        if tag == "a" and attrs.get("href"):
            href = attrs["href"]
            self.links.append(href)
            if self.in_nav:
                self.nav_link = [href, ""]
            if self.in_outline and "outline-link" in classes:
                self.outline_links.append(href)
            if self._edit_link_depth is not None:
                self.edit_links.append(href)
            if in_doc:
                if "/.agents/skills/" in href and "/SKILL.md" in href:
                    self.doc_skill_links.append(href)
                if self._doc_link_depth is None:
                    self._doc_link_href = href
                    self._doc_link_depth = depth
                    self._doc_link_has_code = False
            if "title" in classes:
                self.site_title_link = [href, ""]
        if tag == "code" and self._doc_link_depth is not None:
            self._doc_link_has_code = True
        if tag in {"script", "img"} and attrs.get("src"):
            self.assets.append(attrs["src"])
        if tag == "link" and attrs.get("href"):
            relations = attrs.get("rel", "").split()
            if "canonical" in relations:
                self.canonicals.append(attrs["href"])
            if "stylesheet" in relations or "icon" in relations or "apple-touch-icon" in relations:
                self.assets.append(attrs["href"])
            if "icon" in relations:
                self.icons.append(attrs["href"])
            if "apple-touch-icon" in relations:
                self.apple_touch_icons.append(attrs["href"])
        if tag == "meta":
            name = attrs.get("name", "").lower()
            prop = attrs.get("property", "").lower()
            content = attrs.get("content", "")
            if name in {"robots", "googlebot"}:
                self.robots.append(content)
            elif name == "author":
                self.authors.append(content)
            elif name == "theme-color":
                self.theme_colors.append(content)
            elif name == "description":
                self.descriptions.append(content)
            elif name == "twitter:card":
                self.twitter_cards.append(content)
            elif name == "twitter:image":
                self.twitter_images.append(content)
                if content:
                    self.assets.append(content)
            if prop == "og:type":
                self.og_types.append(content)
            elif prop == "og:site_name":
                self.og_site_names.append(content)
            elif prop == "og:locale":
                self.og_locales.append(content)
            elif prop == "og:url":
                self.og_urls.append(content)
            elif prop == "og:image":
                self.og_images.append(content)
                if content:
                    self.assets.append(content)

    def handle_data(self, text):
        if self.in_title:
            self.titles[-1] += text.replace("\u200b", "")
        if self.nav_link is not None:
            self.nav_link[1] += text
        if self.site_title_link is not None:
            self.site_title_link[1] += text
        if self._active_p_text is not None:
            self._active_p_text.append(text)
        if self._table is not None:
            self._table["text"].append(text)
            if self._row is not None:
                self._row["text"].append(text)
                if self._row_code is not None:
                    self._row_code.append(text)

    def handle_endtag(self, tag):
        if tag in VOID_TAGS:
            return
        depth = self._depth
        if tag == "h1":
            self.in_title = False
        if tag == "nav":
            self.in_nav = False
            self.in_outline = False
        if tag == "a" and self.nav_link is not None:
            href, label = self.nav_link
            self.nav_links.append((href, " ".join(label.split())))
            self.nav_link = None
        if tag == "a" and self.site_title_link is not None:
            href, label = self.site_title_link
            self.site_title_links.append((href, " ".join(label.split())))
            self.site_title_link = None
        if self._doc_link_depth == depth and tag == "a":
            if self._doc_link_has_code:
                self.boxed_code_links.append(self._doc_link_href)
            self._doc_link_href = None
            self._doc_link_depth = None
            self._doc_link_has_code = False
        if self._active_p_depth == depth and tag == "p":
            self._last_block = ("p", " ".join("".join(self._active_p_text).split()))
            self._active_p_text = None
            self._active_p_depth = None
        if self._row_code_depth == depth and tag == "code":
            self._row["code"] = "".join(self._row_code).strip()
            self._row_code = None
            self._row_code_depth = None
        if self._table is not None:
            if tag == "tr" and self._row is not None:
                self._row["text"] = " ".join("".join(self._row["text"]).split())
                self._table["rows"].append(self._row)
                self._row = None
            elif tag == "tbody":
                self._in_tbody = False
            elif self._table_depth == depth and tag == "table":
                self.doc_tables.append(
                    {
                        "text": " ".join("".join(self._table["text"]).split()),
                        "rows": self._table["rows"],
                    }
                )
                self._table = None
                self._table_depth = None
                self._in_tbody = False
        if self._edit_link_depth == depth and tag == "div":
            self._edit_link_depth = None
        if self._vp_doc_body_depth == depth and tag == "div":
            self._vp_doc_body_depth = None
        if self._vp_doc_depth == depth and tag == "div":
            self._vp_doc_depth = None
        self._depth = max(0, self._depth - 1)


def sitemap_urls(output, origin, errors):
    index = output / "sitemap-index.xml"
    if not index.is_file():
        errors.append("Missing published sitemap-index.xml")
        return set()
    try:
        namespaces = {"s": "http://www.sitemaps.org/schemas/sitemap/0.9"}
        sitemap_files = [
            item.text for item in ElementTree.parse(index).findall(".//s:loc", namespaces)
        ]
        if not sitemap_files:
            errors.append("Published sitemap index contains no sitemaps")
        urls = []
        for item in sitemap_files:
            location = urlsplit(item or "")
            if not (item or "").startswith(origin + "/") or ".." in location.path.split("/"):
                errors.append(f"Invalid sitemap location {item!r}")
                continue
            child = output / location.path.lstrip("/")
            if not child.is_file():
                errors.append(f"Missing published sitemap {item}")
                continue
            urls.extend(
                tag.text for tag in ElementTree.parse(child).findall(".//s:loc", namespaces)
            )
        return set(urls)
    except ElementTree.ParseError as error:
        errors.append(f"Invalid published sitemap XML: {error}")
        return set()


def frontmatter_fields(text, filename, errors):
    header = re.match(r"\A---\r?\n(.*?)\r?\n---(?:\r?\n|$)", text, re.S)
    fields = {}
    for key in ("title", "description"):
        matches = re.findall(rf"^{key}:[ \t]*(.*)$", header[1], re.M) if header else []
        value = matches[0].strip() if len(matches) == 1 else ""
        if len(value) >= 2 and value[0] == value[-1] and value[0] in "'\"":
            value = value[1:-1].strip()
        if not value or value in {">", "|", ">-", "|-", ">+", "|+"}:
            errors.append(f"{filename}: expected one nonempty single-line frontmatter {key}")
        else:
            fields[key] = value
    return fields


def frontmatter_layout(text):
    header = re.match(r"\A---\r?\n(.*?)\r?\n---(?:\r?\n|$)", text, re.S)
    if not header:
        return None
    match = re.search(r"^layout:[ \t]*(.*?)[ \t]*$", header[1], re.M)
    return match[1].strip("'\"") if match else None


def public_url_path(route):
    return "/" if route == "/index.html" else route.removesuffix(".html")


def resolve_page_route(path):
    if path.endswith("/"):
        return path + "index.html"
    return path if path.endswith(".html") else path + ".html"


def check_doc_structure(root, sources, pages, errors):
    source_texts = {source.stem: source.read_text(encoding="utf-8") for source in sources}
    for source in sources:
        route = "/" + source.stem + ".html"
        page = pages.get(route)
        if page is None:
            continue
        if page.boxed_code_links:
            errors.append(
                f"{route}: guide links must not wrap inline code ({page.boxed_code_links})"
            )
        if source.stem in NO_SKILL_READING_GUIDES and page.doc_skill_links:
            errors.append(
                f"{route}: guide must not assign repository skills as reading ({page.doc_skill_links})"
            )
        if source.stem not in {"404", "index"} and page.has_vp_doc:
            rel = source.relative_to(root / "docs/content").as_posix()
            expected_edit = EDIT_LINK_PREFIX + rel
            if page.edit_links != [expected_edit]:
                errors.append(
                    f"{route}: expected edit link {expected_edit!r}, found {page.edit_links}"
                )

    for guide, earlier, later in HEADING_ORDER:
        route = f"/{guide}.html"
        page = pages.get(route)
        if page is None or not page.has_vp_doc:
            continue
        if earlier not in page.id_order or later not in page.id_order:
            errors.append(f"{route}: missing ordered headings {earlier!r}, {later!r}")
        elif page.id_order.index(earlier) >= page.id_order.index(later):
            errors.append(f"{route}: expected #{earlier} before #{later}")

    task_reference = pages.get("/task-reference.html")
    if task_reference is None or not task_reference.has_vp_doc:
        return
    catalog = next(
        (table for table in task_reference.doc_tables if "agent-packages" in table["text"]), None
    )
    if catalog is None:
        errors.append("/task-reference.html: missing update-task catalog table")
        return
    expected_update_tasks = sorted(
        stem
        for stem, text in source_texts.items()
        if stem not in {"task-reference", "overlay-scripts"}
        and re.search(r"^\| Selector \|", text, re.M)
        and re.search(r"^\| Commands \|.*`update`", text, re.M)
    )
    catalog_tasks = []
    for row in catalog["rows"]:
        href = row.get("href") or ""
        selector = row.get("code") or ""
        stem = href.rstrip("/").split("/")[-1].removesuffix(".html")
        catalog_tasks.append((stem, selector))
    actual_update_tasks = sorted(stem for stem, _ in catalog_tasks)
    if actual_update_tasks != expected_update_tasks:
        errors.append(
            f"/task-reference.html: update-task catalog {actual_update_tasks} "
            f"does not match task docs {expected_update_tasks}"
        )
    for stem, selector in catalog_tasks:
        route = f"/{stem}.html"
        page = pages.get(route)
        if page is None:
            continue
        if not (page.first_table_intro and page.first_table_intro.strip()):
            errors.append(
                f"{route}: expected introductory paragraph immediately before first table"
            )
        if not page.doc_tables or not selector or selector not in page.doc_tables[0]["text"]:
            errors.append(f"{route}: first table must mention selector {selector!r}")
        for anchor in ("what-it-does", "preview-and-apply", "removal"):
            if page.h2_ids.count(anchor) != 1:
                errors.append(f"{route}: expected one h2#{anchor}")
            if page.outline_links.count(f"#{anchor}") != 1:
                errors.append(f"{route}: expected one outline link to #{anchor}")
        if page.outline_links.count("#configuration") != page.h2_ids.count("configuration"):
            errors.append(
                f"{route}: outline link count for #configuration does not match h2#configuration"
            )


def check_site(root):
    output = root / "docs/dist"
    domain = (root / "docs/public/CNAME").read_text(encoding="utf-8").strip()
    origin = f"https://{domain}"
    pages = {
        "/" + path.relative_to(output).as_posix(): Page(path.read_text(encoding="utf-8"))
        for path in output.rglob("*.html")
    }
    errors = []
    if "/index.html" not in pages:
        return ["Missing built homepage; run npm --prefix docs run build first"]
    if not (root / "docs/content/index.md").is_file():
        errors.append("Missing Markdown homepage docs/content/index.md")
    sidebar_routes = {
        urlsplit(urljoin(origin + public_url_path(route), href)).path
        for route, page in pages.items()
        for href, _ in page.nav_links
    }
    homepage = pages["/index.html"]
    if homepage.site_title_links != [("/", "Dotfiles")]:
        errors.append("Homepage header must link to the introduction with the site title")
    if "/" in sidebar_routes or "/index.html" in sidebar_routes:
        errors.append("Homepage must not appear in sidebar navigation")
    indexed_urls = sitemap_urls(output, origin, errors)
    robots = output / "robots.txt"
    if not robots.is_file():
        errors.append("Missing published robots.txt")
    elif not re.search(
        rf"(?im)^sitemap:\s*{re.escape(origin)}/sitemap-index\.xml\s*$",
        robots.read_text(encoding="utf-8"),
    ):
        errors.append("Published robots.txt does not declare the canonical sitemap")
    expected_urls = set()
    expected_nav_titles = {}
    heading_anchors = runpy.run_path(str(Path(__file__).with_name("check-doc-links.py")))[
        "markdown_anchors"
    ]
    sources = sorted((root / "docs/content").rglob("*.md"))
    if len({source.stem for source in sources}) != len(sources):
        errors.append("Documentation source filenames must be unique across sections")
    expected_routes = {"/" + source.stem + ".html" for source in sources}
    if set(pages) != expected_routes:
        errors.append(
            f"Published routes differ from flat source routes: "
            f"missing={sorted(expected_routes - set(pages))}, extra={sorted(set(pages) - expected_routes)}"
        )
    for source in sources:
        route = "/" + source.stem + ".html"
        public_route = public_url_path(route)
        if source.stem not in {"404", "index"} and public_route not in sidebar_routes:
            errors.append(f"{source.name}: missing from sidebar navigation")
        if route not in pages:
            errors.append(f"{source.name}: missing published route {route}")
            continue
        page = pages[route]
        source_text = source.read_text(encoding="utf-8")
        fields = frontmatter_fields(source_text, source.name, errors)
        home_layout = source.stem == "index" and frontmatter_layout(source_text) == "home"
        if (
            source.stem != "404"
            and "description" in fields
            and page.descriptions != [fields["description"]]
        ):
            errors.append(f"{source.name}: rendered description does not match frontmatter")
        missing = (heading_anchors(source_text) - page.ids) if source.stem != "404" else set()
        if missing:
            errors.append(f"{route}: missing heading anchors {sorted(missing)}")
        if source.stem != "404" and page.h1_count != 1:
            errors.append(f"{route}: expected one page title, found {page.h1_count}")
        elif source.stem != "404":
            title = page.titles[0].strip().rstrip("#").strip()
            slug = re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")
            if source.stem != "index" and source.stem != slug:
                errors.append(f"{route}: filename and URL do not match title {title!r}")
            source_title = re.search(r"^# (.+)$", source_text, re.MULTILINE)
            if home_layout:
                if not title:
                    errors.append(f"{route}: home layout hero title is empty")
                if source_title is not None:
                    errors.append(
                        f"{source.name}: home layout should not declare a second Markdown H1"
                    )
                if "title" in fields:
                    expected_nav_titles[route] = [fields["title"]]
            else:
                if source_title is None or source_title[1] != title:
                    errors.append(
                        f"{source.name}: Markdown title does not match page title {title!r}"
                    )
                if "title" in fields and fields["title"] != title:
                    errors.append(
                        f"{source.name}: frontmatter title does not match page title {title!r}"
                    )
                expected_nav_titles[route] = [t.strip().rstrip("#").strip() for t in page.titles]
        errors.extend(
            f"{route}: missing site icon {icon}"
            for icon in ("/favicon.svg", "/favicon.png", "/favicon.ico")
            if icon not in page.icons
        )
        if "/apple-touch-icon.png" not in page.apple_touch_icons:
            errors.append(f"{route}: missing apple-touch-icon")
        if page.authors != ["Stuart Neivandt"]:
            errors.append(f"{route}: expected author 'Stuart Neivandt', found {page.authors}")
        if page.theme_colors != ["#1a1b26"]:
            errors.append(f"{route}: expected theme-color '#1a1b26', found {page.theme_colors}")
        if page.og_site_names != ["Dotfiles"]:
            errors.append(f"{route}: expected og:site_name 'Dotfiles', found {page.og_site_names}")
        robots_directives = {
            token.strip().lower()
            for value in page.robots
            for token in re.split(r"[,\s]+", value)
            if token.strip()
        }
        noindex = bool(robots_directives & {"noindex", "none"})
        nofollow = bool(robots_directives & {"nofollow", "none"})
        if source.stem == "404":
            if not (noindex and nofollow):
                errors.append(
                    f"{route}: expected noindex, nofollow robots meta tag, found {page.robots}"
                )
        else:
            canonical = origin + public_route
            expected_image = f"{origin}/assets/og-image.png"
            expected_type = "website" if source.stem == "index" else "article"
            if page.canonicals != [canonical]:
                errors.append(
                    f"{route}: expected canonical URL {canonical}, found {page.canonicals}"
                )
            if page.og_types != [expected_type]:
                errors.append(f"{route}: expected og:type {expected_type!r}, found {page.og_types}")
            if page.og_locales != ["en_US"]:
                errors.append(f"{route}: expected og:locale 'en_US', found {page.og_locales}")
            if page.og_urls != [canonical]:
                errors.append(f"{route}: expected og:url {canonical}, found {page.og_urls}")
            if page.og_images != [expected_image]:
                errors.append(
                    f"{route}: expected og:image {expected_image}, found {page.og_images}"
                )
            if page.twitter_cards != ["summary_large_image"]:
                errors.append(
                    f"{route}: expected twitter:card 'summary_large_image', found {page.twitter_cards}"
                )
            if page.twitter_images != [expected_image]:
                errors.append(
                    f"{route}: expected twitter:image {expected_image}, found {page.twitter_images}"
                )
            if source.stem == "index":
                expected_urls.add(canonical)
                if (
                    noindex
                    or nofollow
                    or "index" not in robots_directives
                    or "follow" not in robots_directives
                ):
                    errors.append(
                        f"{route}: landing page must allow indexing and following, found {page.robots}"
                    )
            else:
                if not noindex or nofollow or "follow" not in robots_directives:
                    errors.append(
                        f"{route}: non-home guide must emit noindex, follow, found {page.robots}"
                    )
    if indexed_urls != expected_urls:
        errors.append(
            f"Sitemap URLs differ from indexable landing URL: "
            f"missing={sorted(expected_urls - indexed_urls)}, extra={sorted(indexed_urls - expected_urls)}"
        )
    for route, page in pages.items():
        base_url = origin + public_url_path(route)
        if page.duplicates:
            errors.append(f"{route}: duplicate IDs {sorted(page.duplicates)}")
        for href, label in page.nav_links:
            target_route = resolve_page_route(urlsplit(urljoin(base_url, href)).path)
            target_titles = expected_nav_titles.get(target_route)
            if target_titles is not None and target_titles != [label]:
                errors.append(f"{route}: navigation label {label!r} does not match {href}")
        for target in page.links + page.assets:
            url = urlsplit(urljoin(base_url, target))
            if url.scheme not in {"http", "https"} or url.netloc != domain:
                continue
            path = unquote(url.path)
            if path.endswith(".html"):
                errors.append(f"{route}: expected clean URL without .html, found {target}")
                continue
            page_route = resolve_page_route(path)
            if path.endswith("/"):
                destination = output / page_route.lstrip("/")
            else:
                destination = output / path.lstrip("/")
                if not destination.is_file():
                    destination = output / page_route.lstrip("/")
            if not destination.is_file():
                errors.append(f"{route}: broken destination {target}")
            elif (
                url.fragment
                and page_route in pages
                and unquote(url.fragment) not in pages[page_route].ids
            ):
                errors.append(f"{route}: broken anchor {target}")
    check_doc_structure(root, sources, pages, errors)
    published_cname = output / "CNAME"
    if (
        not published_cname.is_file()
        or published_cname.read_text(encoding="utf-8").strip() != domain
    ):
        errors.append("The published CNAME does not match docs/public/CNAME")
    return errors


if __name__ == "__main__":
    failures = check_site(Path(sys.argv[1]).resolve())
    for failure in failures:
        print(failure)
    if not failures:
        print("Published documentation routes, anchors, and assets are valid")
    sys.exit(bool(failures))
