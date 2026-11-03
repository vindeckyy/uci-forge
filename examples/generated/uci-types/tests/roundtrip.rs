// Generated round-trip checks.

#[test]
fn round_trips_primitives() {
    for _ in 0..100 {
        let time = uci_types::DateTime("2026-11-02T12:00:00Z".to_owned());
        let text = serde_json::to_string(&time).unwrap();
        let again: uci_types::DateTime = serde_json::from_str(&text).unwrap();
        assert_eq!(time, again);
        let bytes = uci_types::HexBytes(vec![0, 15, 255]);
        let text = serde_json::to_string(&bytes).unwrap();
        let again: uci_types::HexBytes = serde_json::from_str(&text).unwrap();
        assert_eq!(bytes, again);
    }
}

#[test]
fn round_trips_task_command() {
    let text = include_str!("../../../../fixtures/messages/task_command.json");
    for _ in 0..100 {
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        let inner = &value["TaskCommand"];
        let typed: uci_types::TaskCommand = serde_json::from_value(inner.clone()).unwrap();
        let encoded = serde_json::to_value(&typed).unwrap();
        let again: uci_types::TaskCommand = serde_json::from_value(encoded).unwrap();
        assert_eq!(typed, again);
    }
}
