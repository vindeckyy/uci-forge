//! Run a scenario from the command line.

use std::path::{Path, PathBuf};

use uci_sim::{RunOptions, Scenario, run};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .unwrap_or_else(|| "examples/scenarios/tasking.toml".to_owned());
    let broker = args.next();
    let scenario = Scenario::load(Path::new(&path))?;
    let events = run(
        &scenario,
        &RunOptions {
            broker,
            transcript: Some(PathBuf::from("transcripts/tasking.jsonl")),
        },
    )
    .await?;
    println!("{} events", events.len());
    Ok(())
}
