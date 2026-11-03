//! Run scripted OWP mission threads against a broker.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::Instant;

pub mod client;
pub mod mock;
pub mod scenario;

pub use scenario::{Action, Scenario, Step, parse_duration};

/// Run options.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Broker URL override.
    pub broker: Option<String>,
    /// Transcript path. No file is written when absent.
    pub transcript: Option<PathBuf>,
}

/// One transcript record.
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptEvent {
    /// Logical milliseconds from scenario start.
    pub t_ms: u64,
    /// Service identifier.
    pub service: String,
    /// `out` or `in`.
    pub dir: String,
    /// Topic.
    pub topic: String,
    /// Payload size in bytes.
    pub bytes: usize,
    /// Message payload.
    pub message: Value,
}

/// Run a scenario.
pub async fn run(scenario: &Scenario, options: &RunOptions) -> Result<Vec<TranscriptEvent>> {
    scenario.validate()?;
    let broker = options
        .broker
        .clone()
        .unwrap_or_else(|| scenario.broker_url().to_owned());

    let services: BTreeSet<String> = scenario.step.iter().map(|s| s.service.clone()).collect();
    let mut clients: HashMap<String, client::Client> = HashMap::new();
    let mut receivers: HashMap<String, mpsc::Receiver<client::Event>> = HashMap::new();
    for service in &services {
        let (client, receiver) = client::connect(&broker, service, scenario.schema_version())
            .await
            .with_context(|| format!("connect service {service}"))?;
        clients.insert(service.clone(), client);
        receivers.insert(service.clone(), receiver);
    }

    for (index, step) in scenario.step.iter().enumerate() {
        if step.action == Action::Expect {
            let client = clients
                .get(&step.service)
                .with_context(|| format!("no client for {}", step.service))?;
            client
                .subscribe(
                    &format!("sub-{index}"),
                    step.message.as_deref().unwrap_or_default(),
                    step.topic.as_deref().unwrap_or_default(),
                )
                .await
                .with_context(|| format!("subscribe step {}", index + 1))?;
        }
    }

    let start = Instant::now();
    let mut transcript = Vec::new();
    for (index, step) in scenario.step.iter().enumerate() {
        let at = parse_duration(&step.at)?;
        tokio::time::sleep_until(start + at).await;
        let t_ms = at.as_millis() as u64;
        match step.action {
            Action::Publish => {
                let raw = step.message.as_deref().unwrap_or_default();
                let path =
                    if Path::new(raw).is_absolute() || scenario.base_dir.as_os_str().is_empty() {
                        PathBuf::from(raw)
                    } else {
                        scenario.base_dir.join(raw)
                    };
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("read payload {}", path.display()))?;
                let value: Value = serde_json::from_str(&text)
                    .with_context(|| format!("parse payload {}", path.display()))?;
                let client = clients
                    .get(&step.service)
                    .with_context(|| format!("no client for {}", step.service))?;
                client
                    .publish(step.topic.as_deref().unwrap_or_default(), &text)
                    .await
                    .with_context(|| format!("publish step {}", index + 1))?;
                transcript.push(TranscriptEvent {
                    t_ms,
                    service: step.service.clone(),
                    dir: "out".to_owned(),
                    topic: step.topic.clone().unwrap_or_default(),
                    bytes: text.len(),
                    message: value,
                });
            }
            Action::Expect => {
                let count = step.count.unwrap_or(1);
                let timeout = parse_duration(step.timeout.as_deref().unwrap_or("5s"))?;
                let receiver = receivers
                    .get_mut(&step.service)
                    .with_context(|| format!("no receiver for {}", step.service))?;
                let deadline = Instant::now() + timeout;
                for _ in 0..count {
                    let event = wait_for(
                        receiver,
                        step.topic.as_deref().unwrap_or_default(),
                        step.message.as_deref().unwrap_or_default(),
                        deadline,
                    )
                    .await
                    .with_context(|| format!("expect step {}", index + 1))?;
                    transcript.push(TranscriptEvent {
                        t_ms,
                        service: step.service.clone(),
                        dir: "in".to_owned(),
                        topic: event.topic,
                        bytes: event.payload.to_string().len(),
                        message: event.payload,
                    });
                }
            }
            Action::Sleep => {}
        }
    }

    for client in clients.values() {
        client.shutdown().await;
    }
    if let Some(path) = &options.transcript {
        write_transcript(path, &transcript)?;
    }
    Ok(transcript)
}

async fn wait_for(
    receiver: &mut mpsc::Receiver<client::Event>,
    topic: &str,
    message: &str,
    deadline: Instant,
) -> Result<client::Event> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("timeout waiting for {message} on {topic}");
        }
        match tokio::time::timeout(remaining, receiver.recv()).await {
            Ok(Some(event)) => {
                if event.topic == topic && event.message == message {
                    return Ok(event);
                }
            }
            Ok(None) => bail!("connection closed while waiting for {message}"),
            Err(_) => bail!("timeout waiting for {message} on {topic}"),
        }
    }
}

/// Write records as JSONL.
pub fn write_transcript(path: &Path, events: &[TranscriptEvent]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = String::new();
    for event in events {
        text.push_str(&serde_json::to_string(event)?);
        text.push('\n');
    }
    std::fs::write(path, text)?;
    Ok(())
}

/// Default expect timeout.
#[must_use]
pub fn default_timeout() -> Duration {
    Duration::from_secs(5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("uci-sim-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn step(at: &str, service: &str, action: Action, topic: &str, message: &str) -> Step {
        Step {
            at: at.to_owned(),
            service: service.to_owned(),
            action,
            topic: Some(topic.to_owned()),
            message: Some(message.to_owned()),
            timeout: Some("2s".to_owned()),
            count: None,
        }
    }

    #[tokio::test]
    async fn runs_scenario_against_mock() {
        let (url, handle) = mock::spawn().await.unwrap();
        let dir = temp_dir("run");
        let task = dir.join("task.json");
        let signal = dir.join("signal.json");
        std::fs::write(
            &task,
            json!({"TaskCommand": {"TaskCommand": {}}}).to_string(),
        )
        .unwrap();
        std::fs::write(
            &signal,
            json!({"SignalReport": {"SignalReport": {}}}).to_string(),
        )
        .unwrap();
        let scenario = Scenario {
            broker: Some(url),
            schema: None,
            seed: None,
            base_dir: PathBuf::new(),
            step: vec![
                {
                    let mut s = step(
                        "0s",
                        "c2",
                        Action::Publish,
                        "mission.task",
                        &task.to_string_lossy(),
                    );
                    s.message = Some(task.to_string_lossy().to_string());
                    s
                },
                step(
                    "10ms",
                    "sensor",
                    Action::Expect,
                    "mission.task",
                    "TaskCommand",
                ),
                {
                    let mut s = step(
                        "20ms",
                        "sensor",
                        Action::Publish,
                        "mission.signal",
                        &signal.to_string_lossy(),
                    );
                    s.message = Some(signal.to_string_lossy().to_string());
                    s
                },
                step(
                    "30ms",
                    "c2",
                    Action::Expect,
                    "mission.signal",
                    "SignalReport",
                ),
            ],
        };
        let transcript_path = dir.join("transcript.jsonl");
        let events = run(
            &scenario,
            &RunOptions {
                broker: None,
                transcript: Some(transcript_path.clone()),
            },
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].dir, "out");
        assert_eq!(events[1].dir, "in");
        assert_eq!(events[1].message["TaskCommand"]["TaskCommand"], json!({}));
        assert!(transcript_path.exists());
        let lines = std::fs::read_to_string(&transcript_path).unwrap();
        assert_eq!(lines.lines().count(), 4);
        handle.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn times_out_when_no_message_arrives() {
        let (url, handle) = mock::spawn().await.unwrap();
        let scenario = Scenario {
            broker: Some(url),
            schema: None,
            seed: None,
            base_dir: PathBuf::new(),
            step: vec![{
                let mut s = step("0s", "c2", Action::Expect, "mission.task", "TaskCommand");
                s.timeout = Some("50ms".to_owned());
                s
            }],
        };
        let error = run(&scenario, &RunOptions::default()).await.unwrap_err();
        assert!(format!("{error:#}").contains("timeout"), "{error:#}");
        handle.abort();
    }

    #[tokio::test]
    async fn rejects_missing_payload_file() {
        let (url, handle) = mock::spawn().await.unwrap();
        let scenario = Scenario {
            broker: Some(url),
            schema: None,
            seed: None,
            base_dir: PathBuf::new(),
            step: vec![step(
                "0s",
                "c2",
                Action::Publish,
                "mission.task",
                "/does/not/exist.json",
            )],
        };
        let error = run(&scenario, &RunOptions::default()).await.unwrap_err();
        assert!(error.to_string().contains("read payload"), "{error}");
        handle.abort();
    }

    #[tokio::test]
    async fn server_rejects_invalid_publish() {
        let (url, handle) = mock::spawn().await.unwrap();
        let dir = temp_dir("invalid");
        let file = dir.join("bad.json");
        std::fs::write(&file, "not json").unwrap();
        let scenario = Scenario {
            broker: Some(url),
            schema: None,
            seed: None,
            base_dir: PathBuf::new(),
            step: vec![step(
                "0s",
                "c2",
                Action::Publish,
                "mission.task",
                &file.to_string_lossy(),
            )],
        };
        assert!(run(&scenario, &RunOptions::default()).await.is_err());
        handle.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_timeout_is_five_seconds() {
        assert_eq!(default_timeout(), Duration::from_secs(5));
    }

    #[tokio::test]
    async fn sleep_only_scenario_returns_no_events() {
        let (url, handle) = mock::spawn().await.unwrap();
        let scenario = Scenario {
            broker: Some(url),
            schema: None,
            seed: None,
            base_dir: PathBuf::new(),
            step: vec![Step {
                at: "0s".to_owned(),
                service: "c2".to_owned(),
                action: Action::Sleep,
                topic: None,
                message: None,
                timeout: None,
                count: None,
            }],
        };
        assert!(
            run(&scenario, &RunOptions::default())
                .await
                .unwrap()
                .is_empty()
        );
        handle.abort();
    }

    #[tokio::test]
    async fn expired_deadline_fails_before_receive() {
        let (_tx, mut rx) = mpsc::channel(1);
        let deadline = Instant::now() - Duration::from_secs(1);
        let error = wait_for(&mut rx, "t", "M", deadline).await.unwrap_err();
        assert!(error.to_string().contains("timeout"));
    }

    #[test]
    fn transcript_write_errors_on_bad_path() {
        assert!(write_transcript(Path::new("/"), &[]).is_err());
    }
}
