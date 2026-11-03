#!/usr/bin/env python3
"""Check 95% changed-line coverage from an LCOV report."""

from __future__ import annotations

import os
import re
import subprocess
import sys
from collections import defaultdict

THRESHOLD = 95.0
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def changed_lines(ref: str) -> dict[str, set[int]]:
    result: dict[str, set[int]] = defaultdict(set)
    try:
        diff = subprocess.run(
            ["git", "diff", "-U0", "--no-color", ref, "--", "*.rs"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    except subprocess.CalledProcessError:
        return result

    current = None
    for line in diff.splitlines():
        if line.startswith("+++ b/"):
            current = line[6:]
        elif line.startswith("@@") and current:
            match = re.search(r"\+(\d+)(?:,(\d+))?", line)
            if match:
                start = int(match.group(1))
                count = int(match.group(2) or "1")
                result[current].update(range(start, start + count))

    untracked = subprocess.run(
        ["git", "ls-files", "--others", "--exclude-standard", "*.rs"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    ).stdout
    for name in untracked.splitlines():
        path = os.path.join(ROOT, name)
        if not os.path.isfile(path):
            continue
        with open(path, encoding="utf-8") as handle:
            result[name].update(range(1, len(handle.readlines()) + 1))
    return result


def lcov_data(path: str) -> dict[str, dict[int, int]]:
    data: dict[str, dict[int, int]] = defaultdict(dict)
    current = None
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if line.startswith("SF:"):
                current = os.path.relpath(line[3:], ROOT)
                if current.startswith(".."):
                    current = line[3:]
            elif line.startswith("DA:") and current:
                number, count = line[3:].split(",")[:2]
                data[current][int(number)] = int(count)
    return data


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: changed_coverage.py <lcov> <base-ref>", file=sys.stderr)
        return 2
    coverage = lcov_data(sys.argv[1])
    changes = changed_lines(sys.argv[2])

    covered = 0
    missed = 0
    missed_lines: list[tuple[str, int]] = []
    for name, lines in sorted(changes.items()):
        if name not in coverage:
            continue
        for number in sorted(lines):
            if number not in coverage[name]:
                continue
            if coverage[name][number] > 0:
                covered += 1
            else:
                missed += 1
                missed_lines.append((name, number))

    total = covered + missed
    if total == 0:
        print("changed-line coverage: no executable changed lines, gate satisfied")
        return 0

    percent = 100.0 * covered / total
    print(f"changed-line coverage: {percent:.2f}% ({covered}/{total})")
    if percent < THRESHOLD:
        for name, number in missed_lines[:40]:
            print(f"  uncovered {name}:{number}")
        print(f"FAIL: below {THRESHOLD:.0f}%")
        return 1
    print(f"PASS: at or above {THRESHOLD:.0f}%")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
