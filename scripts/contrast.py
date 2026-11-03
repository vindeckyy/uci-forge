#!/usr/bin/env python3
"""WCAG contrast checks for the site palette."""

from __future__ import annotations


def channel(value: str) -> float:
    value = value.lstrip("#")
    parts = [int(value[i : i + 2], 16) / 255.0 for i in (0, 2, 4)]
    return sum(
        (c / 12.92) if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4
        for c in parts
    ) / 3.0


def ratio(a: str, b: str) -> float:
    la, lb = channel(a), channel(b)
    high, low = max(la, lb), min(la, lb)
    return (high + 0.05) / (low + 0.05)


PAIRS = {
    "light body": ("#1c1b18", "#f7f4ee"),
    "light muted": ("#5a564d", "#f7f4ee"),
    "light accent": ("#9a3b00", "#f7f4ee"),
    "light code": ("#1c1b18", "#efe9dc"),
    "dark body": ("#ece7db", "#14130f"),
    "dark muted": ("#b3ac9c", "#14130f"),
    "dark accent": ("#ffb27a", "#14130f"),
    "dark code": ("#ece7db", "#1e1c16"),
}


def main() -> int:
    failed = False
    for name, (fg, bg) in PAIRS.items():
        value = ratio(fg, bg)
        status = "PASS" if value >= 4.5 else "FAIL"
        if value < 4.5:
            failed = True
        print(f"{status} {name}: {value:.2f}:1")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
