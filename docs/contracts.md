# Contracts

These interfaces are frozen for parallel work. Changes need review from every affected owner.

## C1: schema API

```rust
pub fn load_dir(dir: &Path) -> Result<Schema, SchemaError>;

pub struct Schema {
    pub version: String,
    pub messages: BTreeMap<String, MessageInfo>,
    pub roots: BTreeMap<String, String>,
    pub types: BTreeMap<String, TypeInfo>,
}

pub struct MessageInfo {
    pub name: String,
    pub primitive: Option<String>,
    pub summary: String,
    pub docs: String,
    pub root_type: String,
}
```

## C2: validation error JSON

```json
{
  "path": "$.TaskCommand.MessageData.TaskID",
  "code": "MISSING_REQUIRED",
  "expected": "string",
  "found": null
}
```

Codes: `ROOT_SHAPE`, `UNKNOWN_MESSAGE`, `MISSING_REQUIRED`, `UNKNOWN_MEMBER`,
`TYPE_MISMATCH`, `OCCURRENCE`, `CHOICE`, `FACET`, `UNKNOWN_TYPE`,
`BAD_POLYMORPHIC_TYPE`.

## C3: generated crate

- Path: `examples/generated/uci-types/`
- Files: `Cargo.toml`, `src/lib.rs`, `src/types.rs`.
- Dependencies: `serde`, `serde_json`.
- Header: generated from UCI 2.6, Distribution A, not operational.

## C4: scenario format, TOML

```toml
broker = "ws://127.0.0.1:9100/"
schema = "002.6.0"

[[step]]
at = "0s"
service = "c2"
action = "publish"
topic = "mission.task"
message = "../../fixtures/messages/task_command.json"

[[step]]
at = "100ms"
service = "sensor"
action = "expect"
topic = "mission.task"
message = "TaskCommand"
timeout = "3s"
```

Relative payload paths resolve from the scenario file's directory.

## C5: transcript JSONL

```json
{"t_ms":0,"service":"c2","dir":"out","topic":"mission.task","bytes":833,"message":{}}
```

## C6: docs URL layout

- `/messages/task-command.html`
- `/types/task-command-data.html`
- `/diff/2.5-to-2.6.html`
- `/search-index.json`

## C7: diff report JSON

```json
{
  "from": "002.5.0",
  "to": "002.6.0",
  "changes": [
    {
      "kind": "FIELD_ADDED",
      "class": "breaking",
      "message": "TaskCommand",
      "path": "TaskCommand.NewField",
      "detail": "required field added"
    }
  ]
}
```

Exit 1 when a breaking change exists and `--fail-on-breaking` is set.

## C8: CLI exit codes

0 success, 1 validation or diff failure, 2 usage, 3 internal error.
