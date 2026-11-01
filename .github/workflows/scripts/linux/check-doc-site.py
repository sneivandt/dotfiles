#!/usr/bin/env python3
"""Check published routes, anchors, and local assets in the built docs site."""

from html.parser import HTMLParser
from pathlib import Path
import re
import runpy
import sys
from urllib.parse import unquote, urljoin, urlsplit


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
        self.icons = []
        self.in_title = False
        self.in_nav = False
        self.nav_link = None
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
        if tag in {"script", "img"} and attrs.get("src"):
            self.assets.append(attrs["src"])
        if tag == "link" and attrs.get("href"):
            relations = attrs.get("rel", "").split()
            if "stylesheet" in relations or "icon" in relations:
                self.assets.append(attrs["href"])
            if "icon" in relations:
                self.icons.append(attrs["href"])

    def handle_data(self, text):
        if self.in_title:
            self.titles[-1] += text
        if self.nav_link is not None:
            self.nav_link[1] += text

    def handle_endtag(self, tag):
        if tag == "h1":
            self.in_title = False
        if tag == "nav":
            self.in_nav = False
        if tag == "a" and self.nav_link is not None:
            href, label = self.nav_link
            self.nav_links.append((href, " ".join(label.split())))
            self.nav_link = None


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
    heading_anchors = runpy.run_path(str(Path(__file__).with_name("check-doc-links.py")))["markdown_anchors"]
    for source in sorted((root / "docs/content").glob("*.md")):
        route = "/" + source.stem + ".html"
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
            if source.stem != slug:
                errors.append(f"{route}: filename and URL do not match title {title!r}")
            source_title = re.search(r"^# (.+)$", source_text, re.MULTILINE)
            if source_title is None or source_title[1] != title:
                errors.append(f"{source.name}: Markdown title does not match page title {title!r}")
        if "/favicon.svg" not in page.icons:
            errors.append(f"{route}: missing site favicon")
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
