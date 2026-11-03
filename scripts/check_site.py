#!/usr/bin/env python3
"""Static accessibility and link checks for the generated site."""

from __future__ import annotations

import os
import sys
from html.parser import HTMLParser

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SITE = os.path.join(ROOT, "site")
COPY_PAGES = {
    "index.html",
    "messages/index.html",
    "types/index.html",
    "diff/2.5-to-2.6.html",
    "v2.5/index.html",
    "v2.5/messages/index.html",
    "v2.5/types/index.html",
}


class Checker(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.tags = []
        self.h1 = 0
        self.title = False
        self.lang = False
        self.main = False
        self.header = False
        self.nav = False
        self.footer = False
        self.viewport = ""
        self.scripts = []
        self.labels = 0
        self.inputs = 0
        self.nav_links = []
        self.in_nav = False

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        self.tags.append(tag)
        if tag == "html":
            self.lang = attrs.get("lang") == "en"
        elif tag == "title":
            self.title = True
        elif tag == "h1":
            self.h1 += 1
        elif tag == "main":
            self.main = True
        elif tag == "header":
            self.header = True
        elif tag == "nav":
            self.nav = True
            self.in_nav = True
        elif tag == "footer":
            self.footer = True
        elif tag == "meta" and attrs.get("name") == "viewport":
            self.viewport = attrs.get("content", "")
        elif tag == "script" and attrs.get("src"):
            self.scripts.append(attrs["src"])
        elif tag == "label":
            self.labels += 1
        elif tag == "input":
            self.inputs += 1
        elif tag == "a" and self.in_nav:
            href = attrs.get("href")
            if href:
                self.nav_links.append(href)

    def handle_endtag(self, tag):
        if tag == "nav":
            self.in_nav = False


def main() -> int:
    if not os.path.isdir(SITE):
        print("site/ is missing; run the docs build first", file=sys.stderr)
        return 2

    failures = []
    pages = 0
    for base, _, files in os.walk(SITE):
        for name in files:
            if not name.endswith(".html"):
                continue
            pages += 1
            path = os.path.join(base, name)
            rel = os.path.relpath(path, SITE)
            with open(path, encoding="utf-8") as handle:
                text = handle.read()
            checker = Checker()
            checker.feed(text)

            if not checker.lang:
                failures.append(f"{rel}: missing lang=en")
            if not checker.title:
                failures.append(f"{rel}: missing title")
            if not checker.main:
                failures.append(f"{rel}: missing main")
            if not checker.header or not checker.nav or not checker.footer:
                failures.append(f"{rel}: missing landmark")
            if checker.h1 != 1:
                failures.append(f"{rel}: expected one h1, found {checker.h1}")
            if "user-scalable=no" in checker.viewport or "maximum-scale=1" in checker.viewport:
                failures.append(f"{rel}: blocks zoom")
            if checker.labels < checker.inputs:
                failures.append(f"{rel}: input without label")
            for source in checker.scripts:
                if source.startswith("http://") or source.startswith("https://"):
                    failures.append(f"{rel}: third-party script {source}")
            for href in checker.nav_links:
                target = href.split("#", 1)[0]
                if not target:
                    continue
                resolved = os.path.normpath(os.path.join(os.path.dirname(path), target))
                if not os.path.exists(resolved):
                    failures.append(f"{rel}: dead nav link {href}")
            if rel in COPY_PAGES and "\u2014" in text:
                failures.append(f"{rel}: em dash in agent copy")

    if pages == 0:
        failures.append("no HTML pages found")

    print(f"checked {pages} pages")
    if failures:
        for failure in failures[:40]:
            print(f"FAIL {failure}")
        return 1
    print("PASS: landmarks, titles, headings, labels, links, zoom, scripts, copy")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
