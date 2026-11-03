# Examples

These commands run against the public schema and the checked-in fixtures. Start
with `make fetch-spec` once.

## Smoke test

```bash
./examples/smoke.sh
```

It prints schema counts, validates a real `TaskCommand`, runs a four-event scenario
against the in-process broker, and checks the transcript exists.

## Generate the typed crate

```bash
cargo run -p uci-cli -- gen --out examples/generated/uci-types
cargo test --manifest-path examples/generated/uci-types/Cargo.toml
```

The generated crate is not committed. It has two round-trip tests at 100 iterations
each.

## Validate a message

```bash
cargo run -p uci-cli -- validate --file fixtures/messages/task_command.json
```

A bad message prints a JSON path and a stable code:

```bash
cargo run -p uci-cli -- validate --file fixtures/messages/bad.json
```

## Run a scenario without Sleet

```bash
cargo run -p uci-cli -- sim examples/scenarios/tasking.toml --embedded
```

Expected output: `4 events`.

## Run the same scenario against Sleet

```bash
sleet fixtures/sleet/server.toml
cargo run -p uci-cli -- sim examples/scenarios/tasking.toml \
  --broker ws://127.0.0.1:9100/ --transcript transcripts/tasking.jsonl
```

The transcript records a `TaskCommand` from `c2` to `sensor`, then a `SignalReport`
back to `c2`.
