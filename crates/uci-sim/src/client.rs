//! Minimal OWP client for the mission-thread harness.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// One received message.
#[derive(Debug, Clone)]
pub struct Event {
    /// Receiving service.
    pub service: String,
    /// Topic from the matching subscription.
    pub topic: String,
    /// Message root name.
    pub message: String,
    /// Full JSON payload.
    pub payload: Value,
}

enum Command {
    Publish {
        topic: String,
        payload: String,
        ack: oneshot::Sender<Result<(), String>>,
    },
    Subscribe {
        sid: String,
        message: String,
        topic: String,
        ack: oneshot::Sender<Result<(), String>>,
    },
    Shutdown,
}

/// A connected OWP service.
#[derive(Debug, Clone)]
pub struct Client {
    /// Service identifier.
    pub service: String,
    tx: mpsc::Sender<Command>,
}

impl Client {
    /// Publish a JSON payload.
    pub async fn publish(&self, topic: &str, payload: &str) -> Result<()> {
        let (ack, wait) = oneshot::channel();
        self.tx
            .send(Command::Publish {
                topic: topic.to_owned(),
                payload: payload.to_owned(),
                ack,
            })
            .await
            .context("send publish command")?;
        wait.await
            .context("publish ack dropped")?
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }

    /// Subscribe to a message name on a topic.
    pub async fn subscribe(&self, sid: &str, message: &str, topic: &str) -> Result<()> {
        let (ack, wait) = oneshot::channel();
        self.tx
            .send(Command::Subscribe {
                sid: sid.to_owned(),
                message: message.to_owned(),
                topic: topic.to_owned(),
                ack,
            })
            .await
            .context("send subscribe command")?;
        wait.await
            .context("subscribe ack dropped")?
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }

    /// Close the connection.
    pub async fn shutdown(&self) {
        let _ = self.tx.send(Command::Shutdown).await;
    }
}

/// Connect one service and return the client plus its event stream.
pub async fn connect(
    url: &str,
    service: &str,
    schema: &str,
) -> Result<(Client, mpsc::Receiver<Event>)> {
    let mut request = url
        .into_client_request()
        .context("build websocket request")?;
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "owp, OWP".parse().context("protocol header")?,
    );
    let (mut ws, _) = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .context("connect timeout")?
    .context("connect websocket")?;

    let init = serde_json::json!({
        "versions": ["1.0"],
        "schema": schema,
        "verbose": true,
        "service_id": service,
    });
    ws.send(Message::Text(format!("INIT {init}").into()))
        .await
        .context("send INIT")?;

    let ready = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let text = text.as_str();
                    if text.starts_with("INFO ") {
                        return Ok(());
                    }
                    if let Some(detail) = text.strip_prefix("-ERR ") {
                        bail!("server rejected INIT: {detail}");
                    }
                }
                Some(Ok(Message::Close(_))) | None => bail!("server closed before INFO"),
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error).context("read INFO"),
            }
        }
    })
    .await
    .context("INFO timeout")?;
    ready?;

    let (cmd_tx, cmd_rx) = mpsc::channel(256);
    let (event_tx, event_rx) = mpsc::channel(1024);
    let service_owned = service.to_owned();
    tokio::spawn(async move {
        run_socket(ws, service_owned, cmd_rx, event_tx).await;
    });

    Ok((
        Client {
            service: service.to_owned(),
            tx: cmd_tx,
        },
        event_rx,
    ))
}

async fn run_socket(
    mut ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    service: String,
    mut cmd_rx: mpsc::Receiver<Command>,
    event_tx: mpsc::Sender<Event>,
) {
    let mut pending: Option<oneshot::Sender<Result<(), String>>> = None;
    let mut topics: HashMap<String, String> = HashMap::new();

    loop {
        tokio::select! {
            command = cmd_rx.recv() => {
                let Some(command) = command else { break };
                match command {
                    Command::Publish { topic, payload, ack } => {
                        let frame = format!("PUB {topic} {payload}");
                        if let Err(error) = ws.send(Message::Text(frame.into())).await {
                            let _ = ack.send(Err(error.to_string()));
                            break;
                        }
                        pending = Some(ack);
                    }
                    Command::Subscribe { sid, message, topic, ack } => {
                        let frame = format!("SUB {sid} {message} {topic}");
                        if let Err(error) = ws.send(Message::Text(frame.into())).await {
                            let _ = ack.send(Err(error.to_string()));
                            break;
                        }
                        topics.insert(sid, topic);
                        pending = Some(ack);
                    }
                    Command::Shutdown => {
                        let _ = ws.close(None).await;
                        break;
                    }
                }
            }
            frame = ws.next() => {
                match frame {
                    Some(Ok(Message::Text(text))) => {
                        let text = text.as_str();
                        if text == "+OK" {
                            if let Some(ack) = pending.take() {
                                let _ = ack.send(Ok(()));
                            }
                        } else if let Some(detail) = text.strip_prefix("-ERR ") {
                            if let Some(ack) = pending.take() {
                                let _ = ack.send(Err(detail.to_owned()));
                            }
                        } else if let Some(rest) = text.strip_prefix("MSG ") {
                            let (sid, payload) = rest.split_once(' ').unwrap_or((rest, ""));
                            let Ok(value) = serde_json::from_str::<Value>(payload) else {
                                continue;
                            };
                            let Some(message) = value.as_object().and_then(|o| o.keys().next()) else {
                                continue;
                            };
                            let topic = topics.get(sid).cloned().unwrap_or_default();
                            let event = Event {
                                service: service.clone(),
                                topic,
                                message: message.clone(),
                                payload: value,
                            };
                            if event_tx.send(event).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        if let Some(ack) = pending.take() {
                            let _ = ack.send(Err(error.to_string()));
                        }
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_shape_is_stable() {
        let event = Event {
            service: "c2".to_owned(),
            topic: "mission.task".to_owned(),
            message: "TaskCommand".to_owned(),
            payload: serde_json::json!({"TaskCommand": {}}),
        };
        assert_eq!(event.message, "TaskCommand");
    }
}

#[cfg(test)]
mod connection_tests {
    #![allow(clippy::result_large_err)]
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

    async fn accept(
        stream: tokio::net::TcpStream,
    ) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
        let callback =
            |request: &Request, mut response: Response| -> Result<Response, ErrorResponse> {
                if let Some(value) = request.headers().get("Sec-WebSocket-Protocol")
                    && let Ok(text) = value.to_str()
                    && let Some(first) = text.split(',').next()
                    && let Ok(header) = first.trim().parse()
                {
                    response
                        .headers_mut()
                        .insert("Sec-WebSocket-Protocol", header);
                }
                Ok(response)
            };
        tokio_tungstenite::accept_hdr_async(stream, callback)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn rejects_bad_url() {
        assert!(connect("not-a-url", "c2", "002.6.0").await.is_err());
    }

    #[tokio::test]
    async fn rejects_closed_port() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        assert!(
            connect(&format!("ws://{addr}/"), "c2", "002.6.0")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn rejects_init_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept(stream).await;
            let _ = ws.next().await;
            ws.send(Message::Text("-ERR Illegal-State nope".into()))
                .await
                .unwrap();
        });
        let error = connect(&format!("ws://{addr}/"), "c2", "002.6.0")
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("rejected INIT"), "{error:#}");
        handle.abort();
    }

    #[tokio::test]
    async fn rejects_close_before_info() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept(stream).await;
            let _ = ws.next().await;
            let _ = ws.close(None).await;
        });
        assert!(
            connect(&format!("ws://{addr}/"), "c2", "002.6.0")
                .await
                .is_err()
        );
        handle.abort();
    }

    #[tokio::test]
    async fn publish_acknowledges_and_rejects() {
        let (url, handle) = crate::mock::spawn().await.unwrap();
        let (client, _events) = connect(&url, "c2", "002.6.0").await.unwrap();
        client
            .publish("mission.task", "{\"TaskCommand\":{}}")
            .await
            .unwrap();
        assert!(client.publish("mission.task", "not json").await.is_err());
        client.shutdown().await;
        handle.abort();
    }
}
