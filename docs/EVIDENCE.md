# Evidence

All results below were produced on 2026-11-02 on macOS arm64 with rustc 1.99.1.

## Schema fetch

```bash
./scripts/fetch-spec.sh
```

Fetched tags `v2.5` and `v2.6` from the public Open Arsenal repository. Verified all six XSD files against `spec-checksums.txt`.

## Schema statistics

```bash
cargo run -p uci-cli -- schema-stats
```

```text
version 002.6.0
messages 725
complex_types 4636
simple_types 935
enum_values 7767
```

## Tests

```bash
cargo test --workspace
```

```text
151 passed, 0 failed
```

Coverage:

```bash
./scripts/coverage.sh
```

```text
TOTAL regions 97.37%, functions 96.75%, lines 98.18%
changed-line gate: 95% minimum
baseline commit: 98.30% (4,341/4,416)
current change: 98.29% (172/175)
```

Instrumented lines exclude `src/bin/` and `src/main.rs` as glue.

## Generated crate

```bash
cargo run -p uci-cli -- gen --out examples/generated/uci-types
cargo test --manifest-path examples/generated/uci-types/Cargo.toml
```

```text
4,535 structs
710 enums with Unknown(String)
131,911 lines
cargo test: 2 round-trip tests, 100 iterations each
```

## Release

Tag `v0.1.0` published three binaries:

```text
forge-aarch64-apple-darwin
forge-x86_64-unknown-linux-gnu
forge-x86_64-pc-windows-msvc.exe
```

## Parser fuzzing

```bash
cargo +nightly fuzz run parse_xsd -- -runs=10000
```

```text
Done 10000 runs
no crash, no artifact
```

## Diff

```bash
cargo run -p uci-cli -- diff --old spec/002.5.0 --new spec/002.6.0
```

```text
0 breaking, 5 risky, 3 compatible
3 new messages: SystemSchedule, SystemScheduleDataRequest, SystemScheduleDataRequestStatus
5 new enum values, including SYSTEM_SCHEDULE and SUBORBITAL
```

## Reference CAL integration

Sleet was built from the public repository and run with `fixtures/sleet/server.toml`.
The scenario published a `TaskCommand` from `c2`, received it at `sensor`, published
a `SignalReport` back, and received it at `c2`.

```bash
cargo run -p uci-cli -- sim examples/scenarios/tasking.toml \
  --broker ws://127.0.0.1:9100/ --transcript transcripts/tasking.jsonl
```

```text
4 events
0    c2      out  mission.task    TaskCommand
100  sensor  in   mission.task    TaskCommand
200  sensor  out  mission.signal  SignalReport
300  c2      in   mission.signal  SignalReport
```

## Documentation site

```bash
cargo run -p uci-docs --bin build-site
python3 scripts/check_site.py
python3 scripts/contrast.py
```

```text
10,702 HTML pages across 2.6 and 2.5
725 of 725 messages have a schema-valid generated example
PASS: landmarks, titles, headings, labels, links, zoom, scripts, copy
8 contrast pairs pass WCAG AA (lowest 5.49:1 light, 8.00:1 dark)
```

## Browser click-through

Playwright Chromium, 30 checks, all pass:

- 725 message rows, filter to one, clear restores 725.
- Version switch 2.6 to 2.5 and back.
- Per-message change badge on a new 2.6 message.
- Theme toggles to dark and back, `aria-pressed` updates, dark background applied.
- Message page heading, 833-character example, required markers.
- Types index 4,636 rows.
- Diff page 8 rows and 8 badges.
- Keyboard focus reaches a link with a 3 px visible outline.
- No horizontal overflow at 390 px on index, messages, message, types, diff, and a 2.5 message.
- No console errors.

## Determinism

Ten embedded runs of `examples/scenarios/tasking.toml` produced one distinct
transcript hash:

```text
7fc10a46892edee16d17bbf544645ee502846a28dcddf346d07bf892ed2da3f3
```

## External actions

Not performed because they need the owner's accounts and identity:

- GitLab account creation and merge requests.
- Email to the AFLCMC inbox.
- Prometheus Flame submission.
- Job applications.

External actions are tracked outside this repository.
