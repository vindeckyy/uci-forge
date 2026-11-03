#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

BASE_REF="${1:-HEAD}"
cargo llvm-cov --workspace \
  --ignore-filename-regex '(src/bin/|src/main\.rs)' \
  --lcov --output-path target/lcov.info \
  --fail-under-lines 95
python3 scripts/changed_coverage.py target/lcov.info "$BASE_REF"
