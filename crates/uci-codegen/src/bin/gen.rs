//! Generate the checked-in example crate.

use std::path::Path;

use uci_codegen::{DEFAULT_MESSAGES, generate, write_files};
use uci_schema::Schema;

fn main() -> anyhow::Result<()> {
    let schema = Schema::load_dir(Path::new("spec/002.6.0"))?;
    let names: Vec<String> = DEFAULT_MESSAGES.iter().map(|s| (*s).to_owned()).collect();
    let files = generate(&schema, &names)?;
    write_files(Path::new("examples/generated/uci-types"), &files)?;
    println!(
        "generated {} files for {} messages",
        files.len(),
        names.len()
    );
    Ok(())
}
