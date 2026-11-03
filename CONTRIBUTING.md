# Contributing

Issues and pull requests are welcome.

## Before a pull request

```bash
make check
```

This runs formatting, clippy, tests, generated round-trip tests, coverage, docs
building, site checks, and contrast checks. The coverage gate is 95% for total lines
and 95% for changed lines.

## Rules

- Keep the change to one concern.
- Add or update a test for behavior changes.
- Do not add a dependency for a few lines.
- Do not include CUI, classified data, or restricted standards.
- Do not claim compliance, certification, or operational use.
- Keep comments for the reason, the constraint, or the non-obvious behavior. Do not
  narrate the code.

## Commits

Conventional Commit prefixes are preferred: `feat:`, `fix:`, `docs:`, `test:`,
`chore:`.

## License

By contributing, you agree that your contribution is licensed under Apache-2.0.
