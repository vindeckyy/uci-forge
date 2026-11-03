#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

if [ ! -f spec/002.6.0/UCI_MessageDefinitions_v2_6_0.xsd ]; then
  echo "missing schema; run make fetch-spec first" >&2
  exit 1
fi

stats="$(cargo run -q -p uci-cli -- schema-stats)"
printf '%s\n' "$stats"
printf '%s\n' "$stats" | grep -q 'messages 725'

cargo run -q -p uci-cli -- validate --file fixtures/messages/task_command.json
cargo run -q -p uci-cli -- sim examples/scenarios/tasking.toml --embedded \
  --transcript target/example-transcript.jsonl

test -s target/example-transcript.jsonl
lines="$(wc -l < target/example-transcript.jsonl | tr -d ' ')"
test "$lines" = "4"

echo "smoke: pass"
