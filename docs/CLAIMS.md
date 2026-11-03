# Claims and limits

Every claim in the README maps to a reproducible check or a named limit. If a check
does not exist, the claim is removed.

| Claim | Evidence | Limit |
|---|---|---|
| Parses UCI 2.6 | `forge schema-stats` reports 725 messages, 4,636 complex types, 935 simple types, 7,767 enum values | Public Distribution A schema only |
| Generates Rust types | `cargo test -p uci-codegen` generates the default set and asserts the output | 20 roots, not all 725 |
| Validates messages | Validator tests cover required fields, types, choices, cardinality, enums, and polymorphism | Structural validation, not mission semantics |
| Diffs 2.5 to 2.6 | `forge diff` reports 0 breaking, 5 risky, 3 compatible | Schema-level compatibility, not program impact |
| Runs against Sleet | One scripted task and signal flow, 4 events | Not a conformance or certification test |
| Documents the schema | 10,702 pages, static checks pass | Generated reference, not a tutorial |
| Has tests | 151 workspace tests, plus 2 generated round-trip tests at 100 iterations | Test count is not a quality score |
| Meets a coverage gate | 98.18% lines, 97.37% regions, 96.75% functions, 95% changed-line gate | Excludes `src/bin/` and `src/main.rs` |
| Fuzzes the parser | 10,000 libFuzzer runs, no crash | Bounded run, not a proof |
| Works in CI | Green on Ubuntu, macOS, and Windows | Does not test hardware or classified networks |
| Has a release | v0.1.0 with Linux, macOS, and Windows binaries | Prebuilt binaries are convenience artifacts, not a support contract |
| Is accessible | 8 contrast pairs pass WCAG AA, keyboard focus, light and dark themes | Not a full WCAG audit |
| Is ready for operations | **No claim.** This is developer tooling | Not certified, not approved, not operational |

## What this is not

- Not affiliated with or endorsed by the U.S. Air Force or the Department of War.
- Not a compliance or certification artifact.
- Not approved for operational use.
- Not a substitute for a program office's integration and test process.

## Reporting a wrong claim

Open an issue with the claim, the command you ran, and the output you saw. A claim
that cannot be reproduced is a bug.
