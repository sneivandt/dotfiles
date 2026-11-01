#!/usr/bin/env python3
"""Check published routes, anchors, and local assets in the built docs site."""

from html.parser import HTMLParser
from pathlib import Path
import re
import runpy
import sys
from urllib.parse import unquote, urljoin, urlsplit
from xml.etree import ElementTree


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__()
        self.ids = set()
        self.duplicates = set()
        self.links = []
        self.assets = []
        self.h1_count = 0
        self.titles = []
        self.nav_links = []
        self.site_title_links = []
        self.icons = []
        self.robots = []
        self.canonicals = []
        self.in_title = False
        self.in_nav = False
        self.nav_link = None
        self.site_title_link = None
        self.feed(text)

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        if identifier := attrs.get("id"):
            if identifier in self.ids:
                self.duplicates.add(identifier)
            self.ids.add(identifier)
        if tag == "h1":
            self.h1_count += 1
            self.titles.append("")
            self.in_title = True
        if tag == "nav" and attrs.get("aria-label") == "Main":
            self.in_nav = True
        if tag == "a" and attrs.get("href"):
            self.links.append(attrs["href"])
            if self.in_nav:
                self.nav_link = [attrs["href"], ""]
            if "site-title" in attrs.get("class", "").split():
                self.site_title_link = [attrs["href"], ""]
        if tag in {"script", "img"} and attrs.get("src"):
            self.assets.append(attrs["src"])
        if tag == "link" and attrs.get("href"):
            relations = attrs.get("rel", "").split()
            if "canonical" in relations:
                self.canonicals.append(attrs["href"])
            if "stylesheet" in relations or "icon" in relations:
                self.assets.append(attrs["href"])
            if "icon" in relations:
                self.icons.append(attrs["href"])
        if tag == "meta" and attrs.get("name", "").lower() in {"robots", "googlebot"}:
            self.robots.append(attrs.get("content", ""))

    def handle_data(self, text):
        if self.in_title:
            self.titles[-1] += text
        if self.nav_link is not None:
            self.nav_link[1] += text
        if self.site_title_link is not None:
            self.site_title_link[1] += text

    def handle_endtag(self, tag):
        if tag == "h1":
            self.in_title = False
        if tag == "nav":
            self.in_nav = False
        if tag == "a" and self.nav_link is not None:
            href, label = self.nav_link
            self.nav_links.append((href, " ".join(label.split())))
            self.nav_link = None
        if tag == "a" and self.site_title_link is not None:
            href, label = self.site_title_link
            self.site_title_links.append((href, " ".join(label.split())))
            self.site_title_link = None


def sitemap_urls(output, origin, errors):
    index = output / "sitemap-index.xml"
    if not index.is_file():
        errors.append("Missing published sitemap-index.xml")
        return set()
    try:
        namespaces = {"s": "http://www.sitemaps.org/schemas/sitemap/0.9"}
        sitemap_files = [item.text for item in ElementTree.parse(index).findall(".//s:loc", namespaces)]
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
            urls.extend(tag.text for tag in ElementTree.parse(child).findall(".//s:loc", namespaces))
        return set(urls)
    except ElementTree.ParseError as error:
        errors.append(f"Invalid published sitemap XML: {error}")
        return set()


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
        urlsplit(urljoin(origin + "/index.html", href)).path
        for href, _ in pages["/index.html"].nav_links
    }
    homepage = pages["/index.html"]
    homepage_title = homepage.titles[0].strip() if homepage.titles else ""
    if homepage.site_title_links != [("/index.html", homepage_title)]:
        errors.append("Homepage header must link to the introduction with its title")
    if "/index.html" in sidebar_routes:
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
    heading_anchors = runpy.run_path(str(Path(__file__).with_name("check-doc-links.py")))["markdown_anchors"]
    for source in sorted((root / "docs/content").glob("*.md")):
        route = "/" + source.stem + ".html"
        if source.stem not in {"404", "index"} and route not in sidebar_routes:
            errors.append(f"{source.name}: missing from homepage navigation")
        if route not in pages:
            errors.append(f"{source.name}: missing published route {route}")
            continue
        page = pages[route]
        source_text = source.read_text(encoding="utf-8")
        missing = heading_anchors(source_text) - page.ids
        if missing:
            errors.append(f"{route}: missing heading anchors {sorted(missing)}")
        if page.h1_count != 1:
            errors.append(f"{route}: expected one page title, found {page.h1_count}")
        elif source.stem != "404":
            title = page.titles[0].strip()
            slug = re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")
            if source.stem != "index" and source.stem != slug:
                errors.append(f"{route}: filename and URL do not match title {title!r}")
            source_title = re.search(r"^# (.+)$", source_text, re.MULTILINE)
            if source_title is None or source_title[1] != title:
                errors.append(f"{source.name}: Markdown title does not match page title {title!r}")
        if "/favicon.svg" not in page.icons:
            errors.append(f"{route}: missing site favicon")
        noindex = any(
            re.search(r"(?:^|[,\s])(?:noindex|none)(?:$|[,\s])", value, re.I)
            for value in page.robots
        )
        if source.stem == "404":
            if not noindex:
                errors.append(f"{route}: missing noindex robots meta tag")
        else:
            canonical = origin + ("/index.html" if source.stem == "index" else route)
            if page.canonicals != [canonical]:
                errors.append(f"{route}: expected canonical URL {canonical}, found {page.canonicals}")
            expected_urls.add(canonical)
            if noindex:
                errors.append(f"{route}: indexable guide has a noindex robots meta tag")
    if indexed_urls != expected_urls:
        errors.append(
            f"Sitemap URLs differ from canonical guide URLs: "
            f"missing={sorted(expected_urls - indexed_urls)}, extra={sorted(indexed_urls - expected_urls)}"
        )
    for route, page in pages.items():
        if page.duplicates:
            errors.append(f"{route}: duplicate IDs {sorted(page.duplicates)}")
        for href, label in page.nav_links:
            target = pages.get(urlsplit(urljoin(origin + route, href)).path)
            if target is not None and target.titles != [label]:
                errors.append(f"{route}: navigation label {label!r} does not match {href}")
        for target in page.links + page.assets:
            url = urlsplit(urljoin(origin + route, target))
            if url.scheme not in {"http", "https"} or url.netloc != domain:
                continue
            path = unquote(url.path)
            if path.endswith("/"):
                path += "index.html"
            destination = output / path.lstrip("/")
            if not destination.is_file():
                errors.append(f"{route}: broken destination {target}")
            elif url.fragment and path in pages and unquote(url.fragment) not in pages[path].ids:
                errors.append(f"{route}: broken anchor {target}")
    published_cname = output / "CNAME"
    if not published_cname.is_file() or published_cname.read_text(encoding="utf-8").strip() != domain:
        errors.append("The published CNAME does not match docs/public/CNAME")
    return errors


if __name__ == "__main__":
    failures = check_site(Path(sys.argv[1]).resolve())
    for failure in failures:
        print(failure)
    if not failures:
        print("Published documentation routes, anchors, and assets are valid")
    sys.exit(bool(failures))
