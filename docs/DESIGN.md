# Design decisions

Each decision records what problem it solved, what else was considered, and what
the choice costs. The goal is to make the reasoning reviewable, not to defend every
line.

## ADR-001: Parse the XSD directly

**Context.** The public UCI package ships an XSD with 4,636 complex types. A JSON
Schema or hand-written model would drift from the standard.

**Decision.** Parse the XSD into an in-memory model with `roxmltree`, then generate
from that model.

**Alternatives.** Hand-write the 20 message types. Faster for one message, wrong for
a living standard.

**Consequences.** The parser owns the schema quirks. A malformed or unsupported XSD
fails at parse time. The model can feed codegen, validation, docs, and diff from one
source.

## ADR-002: Preserve unknown enum values

**Context.** UCI is a living standard. A deployed consumer can receive a value added
after it was built.

**Decision.** Generate `Unknown(String)` and hand-written `Serialize` and
`Deserialize` implementations.

**Alternatives.** Strict enums that reject unknown values. Simple, but it turns a
standard update into an outage.

**Consequences.** A new value round-trips without code changes. The type is no
longer a closed set, so exhaustive matches need an `Unknown` arm.

## ADR-003: Omit absent optional fields

**Context.** Generated types are used to build and inspect wire messages. Adding
keys that were not present changes the payload.

**Decision.** Optional fields use `Option<T>` with `skip_serializing_if`.

**Alternatives.** Always serialize `null`. Simpler, but it changes the wire shape.

**Consequences.** Round trips preserve absence. Code that wants a default must apply
it explicitly.

## ADR-004: Box only graph back edges

**Context.** Some UCI types are recursive. Rust needs indirection to make the type
finite.

**Decision.** Build a type graph, find back edges with a depth-first walk, and box
only those fields.

**Alternatives.** Box every complex field. Works, but adds noise and allocations to
types that do not need them.

**Consequences.** Generated types stay readable. The graph walk must be tested,
which the mutual-recursion test does.

## ADR-005: Abstract types become `Value`

**Context.** UCI has 70 abstract types. A full polymorphic Rust enum for every
subtype would be large and fragile.

**Decision.** A field declared as an abstract type is `serde_json::Value` with a
`$type` string. Concrete subtypes are generated for direct use.

**Alternatives.** Generate enums of all subtypes. More type safety, but a large
amount of generated code and a harder upgrade path when the hierarchy changes.

**Consequences.** Abstract fields lose compile-time checking. The limit is explicit
in the README, the codegen docs, and the tests.

## ADR-006: Build the OWP client instead of vendoring Sleet's

**Context.** Sleet has a client crate, but it is not published on crates.io and its
workspace is external.

**Decision.** Implement the small OWP text protocol in `uci-sim`: `INIT`, `PUB`,
`SUB`, `UNSUB`, and `MSG`.

**Alternatives.** Copy Sleet's client and types. Less code to write, but it couples
the project to an external workspace and its release cadence.

**Consequences.** The client can be tested against an in-process broker and against
Sleet. Protocol changes are our responsibility.

## ADR-007: Static docs instead of a web app

**Context.** The reference has 725 messages and 4,636 types. The reader wants to
search a field name, not use a dashboard.

**Decision.** Generate static HTML, a JSON search index, and a small script. No
framework, no server, no CDN.

**Alternatives.** A single-page app. More interactive, but adds a build stack and a
runtime dependency for a reference manual.

**Consequences.** 10,702 pages, fast hosting, and offline-friendly output. Search is
client-side and the empty state is explicit.

## ADR-008: Start with 20 message roots

**Context.** Generating all 725 roots pulls in nearly the whole schema. That is a
large crate with a long compile time and little extra proof.

**Decision.** Generate 20 roots that cover a task and report flow. Document all 725
messages.

**Alternatives.** Generate all 725. More coverage, much more output to review.

**Consequences.** The codegen is proven on a representative slice. The docs remain
complete. Expanding the list is a one-line change.

## ADR-009: Exclude glue from the coverage gate

**Context.** A `main` function and one-off binaries are hard to unit test and add
little signal.

**Decision.** Measure library code at 95% lines. Exclude `src/bin/` and `src/main.rs`.

**Alternatives.** Count everything, then write tests for `main`. Busywork.

**Consequences.** The gate measures the code that carries logic. The exclusions are
named in the coverage script and the README.

## ADR-010: Do not commit generated output

**Context.** The generated crate is 131,911 lines. Committing it makes the repo look
mostly generated and hides the hand-written code.

**Decision.** Generate the crate on demand. CI generates it, compiles it, and runs
its round-trip tests.

**Alternatives.** Commit the output as an example. Easy to browse, bad signal.

**Consequences.** The repo stays small. The generation path is exercised on every
push. A reader can reproduce it with `make generated`.
