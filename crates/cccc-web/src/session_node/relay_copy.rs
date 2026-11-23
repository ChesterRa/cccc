use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::{net::TcpStream, sync::broadcast};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message as RelayMessage};

pub(super) async fn copy(
    edge: WebSocket,
    local: WebSocketStream<MaybeTlsStream<TcpStream>>,
    mut shutdown: broadcast::Receiver<()>,
) {
    let (mut edge_write, mut edge_read) = edge.split();
    let (mut local_write, mut local_read) = local.split();
    let upstream = async {
        while let Some(Ok(frame)) = edge_read.next().await {
            let closing = matches!(frame, Message::Close(_));
            let frame = match frame {
                Message::Binary(bytes) => RelayMessage::Binary(bytes),
                Message::Text(text) => RelayMessage::Text(text.as_str().into()),
                Message::Ping(bytes) => RelayMessage::Ping(bytes),
                Message::Pong(bytes) => RelayMessage::Pong(bytes),
                Message::Close(_) => RelayMessage::Close(None),
            };
            if local_write.send(frame).await.is_err() || closing {
                break;
            }
        }
    };
    let downstream = async {
        while let Some(Ok(frame)) = local_read.next().await {
            let closing = matches!(frame, RelayMessage::Close(_));
            let frame = match frame {
                RelayMessage::Binary(bytes) => Message::Binary(bytes),
                RelayMessage::Text(text) => Message::Text(text.as_str().into()),
                RelayMessage::Ping(bytes) => Message::Ping(bytes),
                RelayMessage::Pong(bytes) => Message::Pong(bytes),
                RelayMessage::Close(_) => Message::Close(None),
                RelayMessage::Frame(_) => continue,
            };
            if edge_write.send(frame).await.is_err() || closing {
                break;
            }
        }
    };
    tokio::select! { _ = upstream => {}, _ = downstream => {}, _ = shutdown.recv() => {} }
}
