//! Build the checked-in documentation site for 2.6 and 2.5.

use std::path::Path;

use uci_diff::diff;
use uci_docs::{Options, build, write_diff_page};
use uci_schema::Schema;

fn main() -> anyhow::Result<()> {
    let old = Schema::load_dir(Path::new("spec/002.5.0"))?;
    let schema = Schema::load_dir(Path::new("spec/002.6.0"))?;
    let report = diff(&old, &schema);
    let counts = report.counts();

    let built = build(
        &schema,
        Path::new("site"),
        Options {
            include_diff: true,
            changes: Some(&report),
            version: Some("2.6"),
            switch: true,
        },
    )?;

    let _ = std::fs::remove_dir_all("site/v2.5");
    let old_built = build(
        &old,
        Path::new("site/v2.5"),
        Options {
            include_diff: false,
            changes: None,
            version: Some("2.5"),
            switch: true,
        },
    )?;

    write_diff_page(&report, Path::new("site"))?;
    println!(
        "2.6 messages={} types={} examples={} missing_examples={}",
        built.messages, built.types, built.examples, built.missing_examples
    );
    println!(
        "2.5 messages={} types={} examples={} missing_examples={}",
        old_built.messages, old_built.types, old_built.examples, old_built.missing_examples
    );
    println!(
        "diff compatible={} risky={} breaking={}",
        counts.0, counts.1, counts.2
    );
    Ok(())
}
