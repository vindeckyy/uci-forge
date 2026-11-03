# Code generation

`uci-codegen` turns the parsed UCI schema into Rust. Generated output is produced
on demand and is not committed. CI generates the crate, compiles it, and runs its
round-trip tests.

## Input contract

The generator expects the public UCI XSD after it has passed through `uci-schema`.
It assumes:

- Fields are elements. UCI 2.6 has no XML attributes, `xs:any`, unions, or lists.
- Choices are direct children of a complex type or an extension. UCI 2.6 has no
  nested choice groups.
- Abstract types are declared with `abstract="true"` and have concrete subtypes.
- A field's type is a named complex type, a named simple type, or an XSD builtin.

If a field references an unknown type, generation stops with
`CodegenError::UnknownType`. The generator does not silently emit `String`.

## Naming

| UCI | Rust |
|---|---|
| `TaskCommandMT` | `TaskCommand` |
| `TaskCommandMDT` | `TaskCommandData` |
| `EntityID_Type` | `EntityId` |
| `AMTI_Capability` | `AmtiCapability` |

Names are sanitized to PascalCase. Collisions get a numeric suffix. Field names
are snake_case, reserved words get a trailing underscore, and the original wire
name stays on `#[serde(rename = "...")]`.

## Cardinality

| Schema | Rust |
|---|---|
| `minOccurs=1`, `maxOccurs=1` | `T` |
| `minOccurs=0`, `maxOccurs=1` | `Option<T>` |
| repeated, required | `Vec<T>` |
| repeated, optional | `Option<Vec<T>>` |

Optional fields use `skip_serializing_if = "Option::is_none"`. A round trip does
not add empty keys that were absent on the wire.

## Enums

Each enum keeps known values as variants and adds `Unknown(String)`. Serialization
and deserialization are implemented by hand so an unknown value survives. This is
the main forward-compatibility decision in the generator.

## Abstract types

A field declared as an abstract type becomes `serde_json::Value`. The message
carries `$type` as a string. This keeps the generated crate finite and avoids
guessing a full polymorphic type hierarchy. The limit is documented because it
loses compile-time checking for those fields.

## Recursion

The generator builds a graph of complex type references. A depth-first walk marks
back edges. Only those fields are boxed. Boxing every complex field would compile,
but it would make the crate harder to read and would add allocations that the
schema does not require.

## Failures

| Error | Meaning |
|---|---|
| `NoMessages` | No message names were supplied |
| `UnknownMessage` | A requested message is absent from the schema |
| `UnknownType` | A field references a type that is not declared and is not a builtin |

These are stable, testable errors. A schema change that introduces an unsupported
type fails loudly instead of producing a wrong binding.

## Add a message

1. Confirm the message root exists in `spec/002.6.0`.
2. Add it to `DEFAULT_MESSAGES` in `crates/uci-codegen/src/lib.rs`, or pass
   `--message` on the CLI.
3. Run `cargo test -p uci-codegen`. The real-schema suite generates the default
   set and asserts a non-trivial output.
4. Run `make generated` to generate the crate and execute its round-trip tests.

## Tests

- Naming and sanitization.
- Enum edge cases, including empty values, duplicates, and `Unknown`.
- Extension flattening.
- Back-edge boxing for mutual recursion.
- Unknown message and unknown type errors.
- A full generation pass against the real 2.6 schema.
- Two generated round-trip tests, 100 iterations each.
