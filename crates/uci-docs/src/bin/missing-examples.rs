use std::path::Path;
use uci_docs::example_message;
use uci_schema::Schema;

fn main() -> anyhow::Result<()> {
    let schema = Schema::load_dir(Path::new("spec/002.6.0"))?;
    for name in schema.messages.keys() {
        if example_message(&schema, name).is_none() {
            println!("{name}");
        }
    }
    Ok(())
}
