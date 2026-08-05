use axum::Router;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, HeaderName};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use futures_util::Stream;
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::AppState;
use crate::api::ApiError;
use crate::auth::Principal;

const GLOBAL_EVENT_NAME: &str = "event";
const GROUP_LEDGER_EVENT_NAME: &str = "ledger";
const ACTOR_ACTIVITY_EVENT_KIND: &str = "actor.activity";

fn should_replay_group_event(event: &cccc_contracts::Event) -> bool {
    event.kind != ACTOR_ACTIVITY_EVENT_KIND
}

fn sse_event(name: &'static str, event: cccc_contracts::Event) -> Event {
    let event_id = event.id.clone();
    Event::default()
        .event(name)
        .id(event_id)
        .json_data(event)
        .unwrap_or_default()
}

#[derive(Serialize)]
struct GlobalEvent<'a> {
    v: u8,
    id: &'a str,
    ts: &'a str,
    kind: &'a str,
    group_id: &'a str,
}

fn global_sse_event(event: &cccc_contracts::Event) -> Event {
    Event::default()
        .event(GLOBAL_EVENT_NAME)
        .id(&event.id)
        .json_data(GlobalEvent {
            v: event.v,
            id: &event.id,
            ts: &event.ts,
            kind: &event.kind,
            group_id: &event.group_id,
        })
        .unwrap_or_default()
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/events/stream", get(global_events))
        .route("/api/v1/groups/{group_id}/ledger/stream", get(group_events))
}

async fn global_events(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.ledger_events.subscribe_global();
    let shutdown_guard = state.shutdown.clone();
    let mut shutdown = state.shutdown.subscribe();
    let stream = async_stream::stream! {
        let _shutdown_guard = shutdown_guard;
        yield Ok(connected_event());
        loop {
            let received = tokio::select! {
                _ = shutdown.recv() => break,
                received = receiver.recv() => received,
            };
            match received {
                Ok(event) if principal.allows(&event.group_id) => {
                    yield Ok(global_sse_event(&event));
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    yield Ok(stream_error(
                        "global_stream_lagged",
                        "global event stream lagged; refresh required".into(),
                    ));
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn group_events(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let mut receiver = state
        .ledger_events
        .subscribe_group(&group_id)
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    let last_event_id = headers
        .get(HeaderName::from_static("last-event-id"))
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .trim()
        .to_owned();
    let event_hub = state.ledger_events.clone();
    let shutdown_guard = state.shutdown.clone();
    let mut shutdown = state.shutdown.subscribe();
    let stream = async_stream::stream! {
        let _shutdown_guard = shutdown_guard;
        yield Ok(connected_event());
        let mut cursor = last_event_id;
        let mut replayed = HashSet::new();
        let mut replayed_order = VecDeque::new();
        if !cursor.is_empty() {
            loop {
                let page = match event_hub.replay_after(&group_id, &cursor, 2048) {
                    Ok(page) => page,
                    Err(error) => {
                        yield Ok(stream_error("ledger_replay_failed", error.to_string()));
                        return;
                    }
                };
                let count = page.len();
                for event in page {
                    cursor.clone_from(&event.id);
                    remember_replayed(&mut replayed, &mut replayed_order, &event.id);
                    if should_replay_group_event(&event) {
                        yield Ok(sse_event(GROUP_LEDGER_EVENT_NAME, event));
                    }
                }
                if count < 2048 { break; }
            }
        }
        loop {
            let received = tokio::select! {
                _ = shutdown.recv() => break,
                received = receiver.recv() => received,
            };
            match received {
                Ok(event) => {
                    if event.id == cursor || replayed.remove(&event.id) {
                        continue;
                    }
                    cursor.clone_from(&event.id);
                    yield Ok(sse_event(GROUP_LEDGER_EVENT_NAME, event));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    if cursor.is_empty() {
                        continue;
                    }
                    let Ok(replacement) = event_hub.subscribe_group(&group_id) else { break; };
                    receiver = replacement;
                    loop {
                        let page = match event_hub.replay_after(&group_id, &cursor, 2048) {
                            Ok(page) => page,
                            Err(error) => {
                                yield Ok(stream_error("ledger_replay_failed", error.to_string()));
                                return;
                            }
                        };
                        let count = page.len();
                        for event in page {
                            cursor.clone_from(&event.id);
                            remember_replayed(&mut replayed, &mut replayed_order, &event.id);
                            if should_replay_group_event(&event) {
                                yield Ok(sse_event(GROUP_LEDGER_EVENT_NAME, event));
                            }
                        }
                        if count < 2048 { break; }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn stream_error(code: &str, message: String) -> Event {
    Event::default()
        .event("error")
        .json_data(serde_json::json!({
            "ok":false,
            "error":{"code":code,"message":message}
        }))
        .unwrap_or_default()
}

fn remember_replayed(seen: &mut HashSet<String>, order: &mut VecDeque<String>, event_id: &str) {
    const CAPACITY: usize = 1024;
    if seen.insert(event_id.to_owned()) {
        order.push_back(event_id.to_owned());
    }
    while order.len() > CAPACITY {
        if let Some(expired) = order.pop_front() {
            seen.remove(&expired);
        }
    }
}

fn connected_event() -> Event {
    Event::default()
        .comment("connected")
        .retry(Duration::from_secs(1))
}

#[cfg(test)]
#[path = "streams_tests.rs"]
mod tests;
