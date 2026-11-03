//! Scenario parsing for the mission-thread harness.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

/// A scenario file.
#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    /// Broker URL. Overridden by the CLI when provided.
    pub broker: Option<String>,
    /// OMS schema version. Defaults to `002.6.0`.
    pub schema: Option<String>,
    /// Seed for deterministic identifiers.
    pub seed: Option<u64>,
    /// Directory containing the scenario. Relative payloads resolve here.
    #[serde(skip)]
    pub base_dir: PathBuf,
    /// Steps in execution order.
    #[serde(default)]
    pub step: Vec<Step>,
}

/// One scenario step.
#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    /// Logical offset, for example `0s` or `250ms`.
    pub at: String,
    /// Service identifier.
    pub service: String,
    /// Action to perform.
    pub action: Action,
    /// Topic for publish or expect.
    pub topic: Option<String>,
    /// File path for publish, message name for expect.
    pub message: Option<String>,
    /// Expect timeout, default `5s`.
    pub timeout: Option<String>,
    /// Expected message count, default 1.
    pub count: Option<usize>,
}

/// Supported actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Publish a JSON file.
    Publish,
    /// Wait for a message.
    Expect,
    /// Wait without doing anything.
    Sleep,
}

impl Scenario {
    /// Load and validate a scenario.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let mut scenario: Self = toml::from_str(&text)?;
        scenario.base_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        scenario.validate()?;
        Ok(scenario)
    }

    /// Validate required fields and ordering.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.step.is_empty(), "scenario has no steps");
        let mut last = 0u64;
        for (index, step) in self.step.iter().enumerate() {
            let at = parse_duration(&step.at)?;
            anyhow::ensure!(
                at.as_millis() as u64 >= last,
                "step {} moves backwards in time",
                index + 1
            );
            last = at.as_millis() as u64;
            anyhow::ensure!(
                !step.service.is_empty(),
                "step {} has no service",
                index + 1
            );
            match step.action {
                Action::Publish | Action::Expect => {
                    anyhow::ensure!(
                        step.topic.as_deref().is_some_and(|t| !t.is_empty()),
                        "step {} has no topic",
                        index + 1
                    );
                    anyhow::ensure!(
                        step.message.as_deref().is_some_and(|m| !m.is_empty()),
                        "step {} has no message",
                        index + 1
                    );
                }
                Action::Sleep => {}
            }
            if let Some(timeout) = &step.timeout {
                parse_duration(timeout)?;
            }
        }
        Ok(())
    }

    /// Broker URL, with a default.
    #[must_use]
    pub fn broker_url(&self) -> &str {
        self.broker.as_deref().unwrap_or("ws://127.0.0.1:9000/")
    }

    /// Schema version, with a default.
    #[must_use]
    pub fn schema_version(&self) -> &str {
        self.schema.as_deref().unwrap_or("002.6.0")
    }
}

/// Parse `250ms`, `1.5s`, or `2m`.
pub fn parse_duration(text: &str) -> anyhow::Result<Duration> {
    let text = text.trim();
    let (value, multiplier) = if let Some(value) = text.strip_suffix("ms") {
        (value, 1.0)
    } else if let Some(value) = text.strip_suffix('s') {
        (value, 1000.0)
    } else if let Some(value) = text.strip_suffix('m') {
        (value, 60_000.0)
    } else {
        anyhow::bail!("duration '{text}' needs ms, s, or m");
    };
    let number: f64 = value.parse()?;
    anyhow::ensure!(number >= 0.0, "duration must not be negative");
    Ok(Duration::from_millis((number * multiplier) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("0s").unwrap(), Duration::ZERO);
        assert_eq!(parse_duration("250ms").unwrap(), Duration::from_millis(250));
        assert_eq!(parse_duration("1.5s").unwrap(), Duration::from_millis(1500));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
    }

    #[test]
    fn rejects_bad_durations() {
        assert!(parse_duration("10").is_err());
        assert!(parse_duration("-1s").is_err());
        assert!(parse_duration("abc").is_err());
    }

    #[test]
    fn validates_missing_fields() {
        let scenario = Scenario {
            broker: None,
            schema: None,
            seed: None,
            base_dir: PathBuf::new(),
            step: vec![Step {
                at: "0s".to_owned(),
                service: "c2".to_owned(),
                action: Action::Publish,
                topic: None,
                message: None,
                timeout: None,
                count: None,
            }],
        };
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn rejects_missing_message() {
        let scenario = Scenario {
            broker: None,
            schema: None,
            seed: None,
            base_dir: PathBuf::new(),
            step: vec![Step {
                at: "0s".to_owned(),
                service: "c2".to_owned(),
                action: Action::Expect,
                topic: Some("mission.task".to_owned()),
                message: None,
                timeout: None,
                count: None,
            }],
        };
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn rejects_backwards_time() {
        let scenario = Scenario {
            broker: None,
            schema: None,
            seed: None,
            base_dir: PathBuf::new(),
            step: vec![
                Step {
                    at: "2s".to_owned(),
                    service: "c2".to_owned(),
                    action: Action::Sleep,
                    topic: None,
                    message: None,
                    timeout: None,
                    count: None,
                },
                Step {
                    at: "1s".to_owned(),
                    service: "c2".to_owned(),
                    action: Action::Sleep,
                    topic: None,
                    message: None,
                    timeout: None,
                    count: None,
                },
            ],
        };
        assert!(scenario.validate().is_err());
    }
}
