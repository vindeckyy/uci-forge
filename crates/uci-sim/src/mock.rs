//! In-process OWP broker used by tests and `--embedded`.

use std::collections::HashMap;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, broadcast};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

type Subs = Arc<Mutex<HashMap<String, (u64, String, String)>>>;

/// Start a broker on a random loopback port.
pub async fn spawn() -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (bus, _) = broadcast::channel::<(String, String)>(1024);
    let subs: Subs = Arc::new(Mutex::new(HashMap::new()));
    let handle = tokio::spawn(async move {
        let mut next_connection = 0u64;
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let id = next_connection;
            next_connection += 1;
            let bus = bus.clone();
            let subs = Arc::clone(&subs);
            tokio::spawn(async move {
                let _ = handle(stream, id, bus, subs).await;
            });
        }
    });
    Ok((format!("ws://{addr}/"), handle))
}

async fn handle(
    stream: tokio::net::TcpStream,
    connection: u64,
    bus: broadcast::Sender<(String, String)>,
    subs: Subs,
) -> anyhow::Result<()> {
    // Tungstenite fixes this callback signature.
    #[allow(clippy::result_large_err)]
    let callback = |request: &Request, mut response: Response| -> Result<Response, ErrorResponse> {
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
    let mut ws = tokio_tungstenite::accept_hdr_async(stream, callback).await?;
    let mut events = bus.subscribe();
    let mut initialized = false;

    loop {
        tokio::select! {
            frame = ws.next() => {
                let Some(frame) = frame else { break };
                let Ok(frame) = frame else { break };
                let Message::Text(text) = frame else { continue };
                let text = text.as_str();
                if let Some(rest) = text.strip_prefix("INIT ") {
                    if initialized {
                        ws.send(Message::Text("-ERR Illegal-State already initialized".into())).await?;
                        continue;
                    }
                    let payload: Value = serde_json::from_str(rest).unwrap_or(Value::Null);
                    let service = payload.get("service_id").and_then(Value::as_str).unwrap_or("service");
                    initialized = true;
                    let info = serde_json::json!({
                        "version": "1.0",
                        "server_id": "uci-sim-mock",
                        "uuids": {
                            "system": "00000000-0000-0000-0000-000000000000",
                            "service": "00000000-0000-0000-0000-000000000001"
                        },
                        "system_label": format!("mock:{service}")
                    });
                    ws.send(Message::Text(format!("INFO {info}").into())).await?;
                } else if let Some(rest) = text.strip_prefix("SUB ") {
                    let mut parts = rest.splitn(3, ' ');
                    let sid = parts.next().unwrap_or_default();
                    let message = parts.next().unwrap_or_default();
                    let topic = parts.next().unwrap_or_default();
                    if sid.is_empty() || message.is_empty() || topic.is_empty() {
                        ws.send(Message::Text("-ERR Illegal-Argument bad SUB".into())).await?;
                        continue;
                    }
                    subs.lock().await.insert(
                        sid.to_owned(),
                        (connection, topic.to_owned(), message.to_owned()),
                    );
                    ws.send(Message::Text("+OK".into())).await?;
                } else if let Some(rest) = text.strip_prefix("UNSUB ") {
                    subs.lock().await.remove(rest.trim());
                    ws.send(Message::Text("+OK".into())).await?;
                } else if let Some(rest) = text.strip_prefix("PUB ") {
                    let Some((topic, payload)) = rest.split_once(' ') else {
                        ws.send(Message::Text("-ERR Illegal-Argument bad PUB".into())).await?;
                        continue;
                    };
                    let Ok(value) = serde_json::from_str::<Value>(payload) else {
                        ws.send(Message::Text("-ERR Invalid-Message invalid JSON".into())).await?;
                        continue;
                    };
                    if value.as_object().is_none_or(|o| o.len() != 1) {
                        ws.send(Message::Text("-ERR Invalid-Message one root key required".into())).await?;
                        continue;
                    }
                    ws.send(Message::Text("+OK".into())).await?;
                    let _ = bus.send((topic.to_owned(), payload.to_owned()));
                } else {
                    ws.send(Message::Text("-ERR Illegal-Operation unknown op".into())).await?;
                }
            }
            event = events.recv() => {
                let Ok((topic, payload)) = event else { continue };
                let Ok(value) = serde_json::from_str::<Value>(&payload) else { continue };
                let Some(message) = value.as_object().and_then(|o| o.keys().next()) else { continue };
                let targets: Vec<String> = {
                    let subs = subs.lock().await;
                    subs.iter()
                        .filter(|(_, (id, t, m))| *id == connection && t == &topic && m == message)
                        .map(|(sid, _)| sid.clone())
                        .collect()
                };
                for sid in targets {
                    ws.send(Message::Text(format!("MSG {sid} {payload}").into())).await?;
                }
            }
        }
    }

    let mut subs = subs.lock().await;
    subs.retain(|_, (id, _, _)| *id != connection);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    async fn raw(
        url: &str,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let mut request = url.into_client_request().unwrap();
        request
            .headers_mut()
            .insert("Sec-WebSocket-Protocol", "owp".parse().unwrap());
        let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        ws.send(Message::Text(
            "INIT {\"versions\":[\"1.0\"],\"schema\":\"x\",\"service_id\":\"t\"}".into(),
        ))
        .await
        .unwrap();
        let info = ws.next().await.unwrap().unwrap();
        assert!(info.to_text().unwrap().starts_with("INFO "));
        ws
    }

    async fn send(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        text: &str,
    ) -> String {
        ws.send(Message::Text(text.into())).await.unwrap();
        let frame = ws.next().await.unwrap().unwrap();
        frame.to_text().unwrap().to_owned()
    }

    #[tokio::test]
    async fn rejects_bad_frames() {
        let (url, handle) = spawn().await.unwrap();
        let mut ws = raw(&url).await;
        assert!(send(&mut ws, "SUB bad").await.contains("-ERR"));
        assert!(send(&mut ws, "PUB topic").await.contains("-ERR"));
        assert!(send(&mut ws, "PUB topic not-json").await.contains("-ERR"));
        assert!(
            send(&mut ws, "PUB topic {\"A\":{},\"B\":{}}")
                .await
                .contains("-ERR")
        );
        assert!(send(&mut ws, "NOPE").await.contains("-ERR"));
        assert!(send(&mut ws, "INIT {}").await.contains("-ERR"));
        assert_eq!(send(&mut ws, "UNSUB missing").await, "+OK");
        handle.abort();
    }

    #[tokio::test]
    async fn routes_and_unsubscribes() {
        let (url, handle) = spawn().await.unwrap();
        let mut listener = raw(&url).await;
        let mut sender = raw(&url).await;
        assert_eq!(
            send(&mut listener, "SUB s1 TaskCommand mission.task").await,
            "+OK"
        );
        assert_eq!(
            send(&mut sender, "PUB mission.task {\"TaskCommand\":{}}").await,
            "+OK"
        );
        let message = listener.next().await.unwrap().unwrap();
        assert_eq!(message.to_text().unwrap(), "MSG s1 {\"TaskCommand\":{}}");
        assert_eq!(send(&mut listener, "UNSUB s1").await, "+OK");
        assert_eq!(
            send(&mut sender, "PUB mission.task {\"TaskCommand\":{}}").await,
            "+OK"
        );
        let quiet =
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.next()).await;
        assert!(
            quiet.is_err(),
            "unsubscribed listener should receive nothing"
        );
        handle.abort();
    }
}
