//! Command line interface for UCI Forge.

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use uci_schema::Schema;

/// UCI Forge command line.
#[derive(Debug, Parser)]
#[command(name = "forge", version, about = "Tooling for the public UCI schema")]
pub struct Cli {
    /// Subcommand.
    #[command(subcommand)]
    pub command: Command,
}

/// Available commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print schema statistics.
    SchemaStats {
        /// Schema directory.
        #[arg(long, default_value = "spec/002.6.0")]
        dir: PathBuf,
    },
    /// Export the parsed schema as JSON.
    SchemaExport {
        /// Schema directory.
        #[arg(long, default_value = "spec/002.6.0")]
        dir: PathBuf,
        /// Output file.
        #[arg(long)]
        out: PathBuf,
    },
    /// Generate typed Rust bindings.
    Gen {
        /// Schema directory.
        #[arg(long, default_value = "spec/002.6.0")]
        dir: PathBuf,
        /// Output crate root.
        #[arg(long, default_value = "examples/generated/uci-types")]
        out: PathBuf,
        /// Messages to generate. Defaults to the curated set.
        #[arg(long)]
        message: Vec<String>,
    },
    /// Validate a message or a JSONL session.
    Validate {
        /// Schema directory.
        #[arg(long, default_value = "spec/002.6.0")]
        dir: PathBuf,
        /// JSON file.
        #[arg(long)]
        file: Option<PathBuf>,
        /// Read JSONL from stdin.
        #[arg(long)]
        session: bool,
        /// Emit JSON errors.
        #[arg(long)]
        json: bool,
    },
    /// Run a scripted mission thread.
    Sim {
        /// Scenario file.
        scenario: PathBuf,
        /// Broker URL override.
        #[arg(long)]
        broker: Option<String>,
        /// Transcript path.
        #[arg(long)]
        transcript: Option<PathBuf>,
        /// Start an in-process broker.
        #[arg(long)]
        embedded: bool,
    },
    /// Compare two schema versions.
    Diff {
        /// Old schema directory.
        #[arg(long, default_value = "spec/002.5.0")]
        old: PathBuf,
        /// New schema directory.
        #[arg(long, default_value = "spec/002.6.0")]
        new: PathBuf,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
        /// Exit 1 when a breaking change exists.
        #[arg(long)]
        fail_on_breaking: bool,
    },
    /// Build the documentation site.
    Docs {
        /// Schema directory.
        #[arg(long, default_value = "spec/002.6.0")]
        dir: PathBuf,
        /// Output directory.
        #[arg(long, default_value = "site")]
        out: PathBuf,
        /// Old schema directory, for a diff page.
        #[arg(long)]
        diff_old: Option<PathBuf>,
    },
}

/// Run a parsed command. Returns the process exit code.
pub async fn run(cli: Cli) -> anyhow::Result<i32> {
    match cli.command {
        Command::SchemaStats { dir } => {
            let schema = Schema::load_dir(&dir)?;
            println!("version {}", schema.version);
            println!("messages {}", schema.message_count());
            println!("complex_types {}", schema.complex_count());
            println!("simple_types {}", schema.simple_count());
            println!("enum_values {}", schema.enum_value_count());
            Ok(0)
        }
        Command::SchemaExport { dir, out } => {
            let schema = Schema::load_dir(&dir)?;
            std::fs::write(&out, schema.to_json()?)
                .with_context(|| format!("write {}", out.display()))?;
            println!("wrote {}", out.display());
            Ok(0)
        }
        Command::Gen { dir, out, message } => {
            let schema = Schema::load_dir(&dir)?;
            let names: Vec<String> = if message.is_empty() {
                uci_codegen::DEFAULT_MESSAGES
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect()
            } else {
                message
            };
            let files = uci_codegen::generate(&schema, &names)?;
            uci_codegen::write_files(&out, &files)?;
            println!(
                "generated {} files for {} messages",
                files.len(),
                names.len()
            );
            Ok(0)
        }
        Command::Validate {
            dir,
            file,
            session,
            json,
        } => {
            let schema = Schema::load_dir(&dir)?;
            let text = if session {
                let mut text = String::new();
                use std::io::Read as _;
                std::io::stdin().read_to_string(&mut text)?;
                text
            } else {
                let file = file.context("--file or --session is required")?;
                std::fs::read_to_string(&file)
                    .with_context(|| format!("read {}", file.display()))?
            };
            validate_text(&schema, &text, json)
        }
        Command::Sim {
            scenario,
            broker,
            transcript,
            embedded,
        } => {
            let scenario = uci_sim::Scenario::load(&scenario)?;
            let (url, handle) = if embedded {
                let (url, handle) = uci_sim::mock::spawn().await?;
                (Some(url), Some(handle))
            } else {
                (broker, None)
            };
            let events = uci_sim::run(
                &scenario,
                &uci_sim::RunOptions {
                    broker: url,
                    transcript,
                },
            )
            .await?;
            if let Some(handle) = handle {
                handle.abort();
            }
            println!("{} events", events.len());
            Ok(0)
        }
        Command::Diff {
            old,
            new,
            json,
            fail_on_breaking,
        } => {
            let old = Schema::load_dir(&old)?;
            let new = Schema::load_dir(&new)?;
            let report = uci_diff::diff(&old, &new);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.to_markdown());
            }
            Ok(if fail_on_breaking && report.has_breaking() {
                1
            } else {
                0
            })
        }
        Command::Docs { dir, out, diff_old } => {
            let schema = Schema::load_dir(&dir)?;
            if let Some(old_path) = diff_old {
                let old = Schema::load_dir(&old_path)?;
                let report = uci_diff::diff(&old, &schema);
                let built = uci_docs::build(
                    &schema,
                    &out,
                    uci_docs::Options {
                        include_diff: true,
                        changes: Some(&report),
                        version: Some("2.6"),
                        switch: true,
                    },
                )?;
                let old_built = uci_docs::build(
                    &old,
                    &out.join("v2.5"),
                    uci_docs::Options {
                        include_diff: false,
                        changes: None,
                        version: Some("2.5"),
                        switch: true,
                    },
                )?;
                uci_docs::write_diff_page(&report, &out)?;
                println!(
                    "2.6 messages={} types={} examples={} missing_examples={}",
                    built.messages, built.types, built.examples, built.missing_examples
                );
                println!(
                    "2.5 messages={} types={} examples={} missing_examples={}",
                    old_built.messages,
                    old_built.types,
                    old_built.examples,
                    old_built.missing_examples
                );
            } else {
                let built = uci_docs::build(
                    &schema,
                    &out,
                    uci_docs::Options {
                        include_diff: false,
                        changes: None,
                        version: Some("2.6"),
                        switch: false,
                    },
                )?;
                println!(
                    "messages={} types={} examples={} missing_examples={}",
                    built.messages, built.types, built.examples, built.missing_examples
                );
            }
            Ok(0)
        }
    }
}

fn validate_text(schema: &Schema, text: &str, json: bool) -> anyhow::Result<i32> {
    let mut failures = 0usize;
    let mut total = 0usize;
    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        total += 1;
        match uci_validate::validate_str(schema, line) {
            Ok(name) => {
                if !json {
                    println!("{} ok {}", line_number + 1, name);
                }
            }
            Err(errors) => {
                failures += 1;
                if json {
                    println!("{}", serde_json::to_string(&errors)?);
                } else {
                    for error in errors {
                        println!(
                            "{}:{}: {} {} expected={:?} found={:?}",
                            line_number + 1,
                            error.path,
                            error.code,
                            error.path,
                            error.expected,
                            error.found
                        );
                    }
                }
            }
        }
    }
    if !json {
        println!("{total} checked, {failures} failed");
    }
    Ok(if failures == 0 { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn path(name: &str) -> String {
        root().join(name).to_string_lossy().into_owned()
    }

    fn parse(parts: &[&str]) -> Cli {
        let mut args = vec!["forge".to_owned()];
        args.extend(parts.iter().map(|s| (*s).to_owned()));
        Cli::parse_from(args)
    }

    #[tokio::test]
    async fn schema_stats_runs() {
        let spec = path("spec/002.6.0");
        let cli = parse(&["schema-stats", "--dir", &spec]);
        assert_eq!(run(cli).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn schema_export_writes_json() {
        let spec = path("spec/002.6.0");
        let dir = std::env::temp_dir().join(format!("uci-cli-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("schema.json");
        assert_eq!(
            run(parse(&[
                "schema-export",
                "--dir",
                &spec,
                "--out",
                out.to_str().unwrap(),
            ]))
            .await
            .unwrap(),
            0
        );
        assert!(out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn validate_valid_and_invalid() {
        let fixtures = path("fixtures");
        let dir = std::env::temp_dir().join(format!("uci-cli-val-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.json");
        std::fs::write(
            &good,
            r#"{"MiniMessage":{"MessageHeader":{"MessageID":"m","Timestamp":"2026-11-02T12:00:00Z"},"Payload":{"$type":"TrackPayloadType","Kind":"k","TrackID":"t"},"Choice":{"Alpha":"a"}}}"#,
        )
        .unwrap();
        let bad = dir.join("bad.json");
        std::fs::write(&bad, r#"{"Nope":{}}"#).unwrap();
        assert_eq!(
            run(parse(&[
                "validate",
                "--dir",
                &fixtures,
                "--file",
                good.to_str().unwrap(),
                "--json",
            ]))
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            run(parse(&[
                "validate",
                "--dir",
                &fixtures,
                "--file",
                bad.to_str().unwrap(),
                "--json",
            ]))
            .await
            .unwrap(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn gen_writes_crate() {
        let fixtures = path("fixtures");
        let dir = std::env::temp_dir().join(format!("uci-cli-gen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            run(parse(&[
                "gen",
                "--dir",
                &fixtures,
                "--out",
                dir.to_str().unwrap(),
                "--message",
                "MiniMessage",
            ]))
            .await
            .unwrap(),
            0
        );
        assert!(dir.join("src/types.rs").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn diff_runs_and_reports() {
        let fixtures = path("fixtures");
        let cli = parse(&[
            "diff",
            "--old",
            &fixtures,
            "--new",
            &fixtures,
            "--json",
            "--fail-on-breaking",
        ]);
        assert_eq!(run(cli).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn docs_builds_mini_site() {
        let fixtures = path("fixtures");
        let dir = std::env::temp_dir().join(format!("uci-cli-docs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            run(parse(&[
                "docs",
                "--dir",
                &fixtures,
                "--out",
                dir.to_str().unwrap(),
                "--diff-old",
                &fixtures,
            ]))
            .await
            .unwrap(),
            0
        );
        assert!(dir.join("messages/mini-message.html").exists());
        assert!(dir.join("diff/2.5-to-2.6.html").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_text_handles_mixed_sessions() {
        let schema = Schema::load_dir(std::path::Path::new(&path("fixtures"))).unwrap();
        let good = r#"{"MiniMessage":{"MessageHeader":{"MessageID":"m","Timestamp":"2026-11-02T12:00:00Z"},"Payload":{"$type":"TrackPayloadType","Kind":"k","TrackID":"t"},"Choice":{"Alpha":"a"}}}"#;
        let bad = r#"{"Nope":{}}"#;
        let text = format!("{good}\n\n{bad}\n");
        assert_eq!(validate_text(&schema, &text, false).unwrap(), 1);
        assert_eq!(validate_text(&schema, &text, true).unwrap(), 1);
        assert_eq!(
            validate_text(&schema, &format!("{good}\n"), false).unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn validate_requires_a_source() {
        let fixtures = path("fixtures");
        assert!(run(parse(&["validate", "--dir", &fixtures])).await.is_err());
    }

    #[tokio::test]
    async fn schema_export_reports_bad_output() {
        let spec = path("spec/002.6.0");
        let dir = std::env::temp_dir().join(format!("uci-cli-export-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, "x").unwrap();
        let out = blocked.join("out.json");
        assert!(
            run(parse(&[
                "schema-export",
                "--dir",
                &spec,
                "--out",
                out.to_str().unwrap(),
            ]))
            .await
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn gen_defaults_and_rejects_unknown() {
        let spec = path("spec/002.6.0");
        let dir = std::env::temp_dir().join(format!("uci-cli-gen-default-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            run(parse(&[
                "gen",
                "--dir",
                &spec,
                "--out",
                dir.to_str().unwrap(),
            ]))
            .await
            .unwrap(),
            0
        );
        assert!(dir.join("src/types.rs").exists());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            run(parse(&[
                "gen",
                "--dir",
                &spec,
                "--out",
                dir.to_str().unwrap(),
                "--message",
                "Nope",
            ]))
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn diff_markdown_reports_breaking() {
        let new = path("spec/002.6.0");
        let old = path("spec/002.5.0");
        let cli = parse(&["diff", "--old", &new, "--new", &old, "--fail-on-breaking"]);
        assert_eq!(run(cli).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn docs_without_diff_has_no_diff_page() {
        let fixtures = path("fixtures");
        let dir = std::env::temp_dir().join(format!("uci-cli-docs-nodiff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            run(parse(&[
                "docs",
                "--dir",
                &fixtures,
                "--out",
                dir.to_str().unwrap(),
            ]))
            .await
            .unwrap(),
            0
        );
        assert!(!dir.join("diff/2.5-to-2.6.html").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn docs_error_paths_are_reported() {
        let fixtures = path("fixtures");
        let dir = std::env::temp_dir().join(format!("uci-cli-docs-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, "block").unwrap();
        assert!(
            run(parse(&[
                "docs",
                "--dir",
                &fixtures,
                "--out",
                blocked.to_str().unwrap(),
                "--diff-old",
                &fixtures,
            ]))
            .await
            .is_err()
        );
        assert!(
            run(parse(&[
                "docs",
                "--dir",
                &fixtures,
                "--out",
                blocked.to_str().unwrap(),
            ]))
            .await
            .is_err()
        );

        let old_blocked = dir.join("old");
        std::fs::create_dir_all(&old_blocked).unwrap();
        std::fs::write(old_blocked.join("v2.5"), "block").unwrap();
        assert!(
            run(parse(&[
                "docs",
                "--dir",
                &fixtures,
                "--out",
                old_blocked.to_str().unwrap(),
                "--diff-old",
                &fixtures,
            ]))
            .await
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn sim_reports_connection_failure() {
        let scenario = path("examples/scenarios/tasking.toml");
        assert!(
            run(parse(&["sim", &scenario, "--broker", "ws://127.0.0.1:1/",]))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn sim_runs_embedded() {
        let scenario = path("examples/scenarios/tasking.toml");
        let dir = std::env::temp_dir().join(format!("uci-cli-sim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let transcript = dir.join("transcript.jsonl");
        assert_eq!(
            run(parse(&[
                "sim",
                &scenario,
                "--embedded",
                "--transcript",
                transcript.to_str().unwrap(),
            ]))
            .await
            .unwrap(),
            0
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
