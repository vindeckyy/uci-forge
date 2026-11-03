#!/usr/bin/env python3
"""Fetch and verify the public UCI 2.5 and 2.6 schemas."""

from __future__ import annotations

import hashlib
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPEC = ROOT / "spec"
CHECKSUMS = ROOT / "spec-checksums.txt"
REPO = "https://gitlab.com/open-arsenal/uci/standard.git"


def clone(tag: str, target: Path) -> None:
    subprocess.run(
        ["git", "clone", "-q", "--depth", "1", "--branch", tag, REPO, str(target)],
        check=True,
    )


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    with tempfile.TemporaryDirectory() as temp:
        temp_path = Path(temp)
        print("fetching UCI v2.5")
        clone("v2.5", temp_path / "v25")
        print("fetching UCI v2.6")
        clone("v2.6", temp_path / "v26")

        for version in ("002.5.0", "002.6.0"):
            target = SPEC / version
            if target.exists():
                shutil.rmtree(target)
            target.mkdir(parents=True)

        source25 = temp_path / "v25" / "03_OAC-STD-002_RevE_UCI_Schema_v2_5"
        for path in sorted(source25.glob("*.xsd")):
            shutil.copy2(path, SPEC / "002.5.0" / path.name)

        archive = (
            temp_path
            / "v26"
            / "UCI Release Documentation - UCI Schema"
            / "03_UCI-STD-002_Rev6_UCI_Schema_v2_6-CDRL.zip"
        )
        with zipfile.ZipFile(archive) as bundle:
            bundle.extractall(SPEC / "002.6.0")

    lines = []
    for path in sorted(SPEC.rglob("*.xsd")):
        lines.append(f"{sha256(path)}  {path.relative_to(ROOT)}")
    CHECKSUMS.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {CHECKSUMS.name}")

    failed = False
    for line in lines:
        digest, name = line.split("  ", 1)
        actual = sha256(ROOT / name)
        status = "OK" if actual == digest else "FAILED"
        if actual != digest:
            failed = True
        print(f"{name}: {status}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
