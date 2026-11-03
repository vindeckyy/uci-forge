//! Write example payloads used by the Sleet integration scenario.

use std::path::Path;

use uci_docs::example_message;
use uci_schema::Schema;

fn main() -> anyhow::Result<()> {
    let schema = Schema::load_dir(Path::new("spec/002.6.0"))?;
    std::fs::create_dir_all("fixtures/messages")?;
    for (message, file) in [
        ("TaskCommand", "fixtures/messages/task_command.json"),
        ("SignalReport", "fixtures/messages/signal_report.json"),
        ("PositionReport", "fixtures/messages/position_report.json"),
    ] {
        let value = example_message(&schema, message)
            .ok_or_else(|| anyhow::anyhow!("no example for {message}"))?;
        std::fs::write(file, serde_json::to_string_pretty(&value)?)?;
        println!("wrote {file}");
    }
    Ok(())
}
