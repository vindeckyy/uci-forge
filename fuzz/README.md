# Parser fuzzing

The target parses arbitrary UTF-8 input as an XSD. It never resolves includes and
never touches the network.

```bash
rustup toolchain install nightly
cargo install cargo-fuzz --locked
cargo +nightly fuzz run parse_xsd -- -runs=10000
```

Corpus and artifacts are ignored. A crash is a parser bug, not a schema bug.
