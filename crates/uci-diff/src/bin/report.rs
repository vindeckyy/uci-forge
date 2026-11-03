//! Build the 2.5 to 2.6 diff report.

use std::path::Path;

use uci_diff::diff;
use uci_schema::Schema;

fn main() -> anyhow::Result<()> {
    let old = Schema::load_dir(Path::new("spec/002.5.0"))?;
    let new = Schema::load_dir(Path::new("spec/002.6.0"))?;
    let report = diff(&old, &new);
    std::fs::create_dir_all("docs/diff")?;
    std::fs::write("docs/diff/2.5-to-2.6.md", report.to_markdown())?;
    std::fs::write(
        "docs/diff/2.5-to-2.6.json",
        serde_json::to_string_pretty(&report)?,
    )?;
    let (compatible, risky, breaking) = report.counts();
    println!(
        "from={} to={} compatible={} risky={} breaking={}",
        report.from, report.to, compatible, risky, breaking
    );
    Ok(())
}
