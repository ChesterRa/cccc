//! One browser socket multiplexes the existing typed event producers.
pub(super) mod event;
mod subscriptions;

use crate::{
    AppState,
    auth::Principal,
    connect_frames::{LIVE_ACCESS_INTERVAL, ResourceFrameQuery, live_access},
};
use axum::{
    Router,
    extract::{
        Extension, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::time::Duration;
use subscriptions::{Command, Subscriptions};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/events/ws", get(upgrade))
}

async fn upgrade(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(frame): Query<ResourceFrameQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.max_message_size(16 * 1024)
        .max_frame_size(16 * 1024)
        .on_upgrade(move |socket| serve(socket, state, principal, frame))
}

async fn send(socket: &mut WebSocket, value: serde_json::Value) -> Result<(), ()> {
    tokio::time::timeout(
        Duration::from_secs(5),
        socket.send(Message::Text(value.to_string().into())),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())
}

async fn serve(
    mut socket: WebSocket,
    state: AppState,
    principal: Principal,
    frame: ResourceFrameQuery,
) {
    let mut subscriptions = Subscriptions::default();
    let (sender, mut outgoing) = tokio::sync::mpsc::channel(8);
    let mut shutdown = state.shutdown.subscribe();
    let mut access = tokio::time::interval(LIVE_ACCESS_INTERVAL);
    access.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_seen = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _ = shutdown.recv() => break,
            _ = access.tick() => {
                if live_access(&state, &principal, frame.connect_frame.as_deref()).is_none() {
                    let _ = send(&mut socket, json!({"type":"fatal","code":"auth_required"})).await;
                    break;
                }
                if last_seen.elapsed() > Duration::from_secs(45) { break; }
                // Ping verifies the browser is alive; a data heartbeat lets the client
                // detect a blackholed server even though browser JS cannot observe Pong.
                if !tokio::time::timeout(Duration::from_secs(5), socket.send(Message::Ping(Vec::new().into()))).await.is_ok_and(|result| result.is_ok()) { break; }
                if send(&mut socket, json!({"type":"heartbeat"})).await.is_err() { break; }
            },
            packet = outgoing.recv() => {
                let Some(packet) = packet else { break; };
                if subscriptions.is_current(&packet) && send(&mut socket, packet.value).await.is_err() { break; }
            },
            message = socket.next() => {
                let Some(Ok(message)) = message else { break; };
                last_seen = tokio::time::Instant::now();
                match message {
                    Message::Close(_) => break,
                    Message::Text(text) => {
                        let Ok(command) = serde_json::from_str::<Command>(&text) else {
                            let _ = send(&mut socket, json!({"type":"fatal","code":"invalid_subscription"})).await;
                            break;
                        };
                        match subscriptions.apply(command, &state, &principal, &frame, &sender) {
                            Ok(Some(ready)) => if send(&mut socket, ready).await.is_err() { break; },
                            Ok(None) => {},
                            Err(denied) => if send(&mut socket, denied).await.is_err() { break; },
                        }
                    },
                    Message::Binary(_) => break,
                    _ => {},
                }
            }
        }
    }
    drop(subscriptions); // Abort every producer and release its hub/tail before closing.
    let _ = tokio::time::timeout(Duration::from_secs(2), socket.close()).await;
}
