use super::realtime::event::{EventStream, StreamEvent};
use axum::Router;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, HeaderName};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use futures_util::{Stream, StreamExt};
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::convert::Infallible;
use tokio::sync::broadcast;

use crate::AppState;
use crate::api::ApiError;
use crate::auth::Principal;
use crate::connect_frames::{
    LIVE_ACCESS_INTERVAL, ResourceFrameQuery, live_access, live_group_access,
};

const GLOBAL_EVENT_NAME: &str = "event";
const GROUP_LEDGER_EVENT_NAME: &str = "ledger";
const ACTOR_ACTIVITY_EVENT_KIND: &str = "actor.activity";

fn should_replay_group_event(event: &cccc_contracts::Event) -> bool {
    event.kind != ACTOR_ACTIVITY_EVENT_KIND
}

fn sse_event(name: &'static str, event: cccc_contracts::Event) -> StreamEvent {
    let id = event.id.clone();
    StreamEvent::new(name, event).with_id(id)
}

#[derive(Serialize)]
struct GlobalEvent<'a> {
    v: u8,
    id: &'a str,
    ts: &'a str,
    kind: &'a str,
    group_id: &'a str,
}

fn global_sse_event(event: &cccc_contracts::Event) -> StreamEvent {
    StreamEvent::new(
        GLOBAL_EVENT_NAME,
        GlobalEvent {
            v: event.v,
            id: &event.id,
            ts: &event.ts,
            kind: &event.kind,
            group_id: &event.group_id,
        },
    )
    .with_id(event.id.clone())
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/events/stream", get(global_events))
        .route("/api/v1/groups/{group_id}/ledger/stream", get(group_events))
}

async fn global_events(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(frame): Query<ResourceFrameQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    Sse::new(global_source(state, principal, frame).map(|event| event.map(StreamEvent::into_sse)))
        .keep_alive(KeepAlive::default())
}

pub(super) fn global_source(
    state: AppState,
    principal: Principal,
    frame: ResourceFrameQuery,
) -> EventStream {
    let mut receiver = state.ledger_events.subscribe_global();
    let shutdown_guard = state.shutdown.clone();
    let mut shutdown = state.shutdown.subscribe();
    let stream = async_stream::stream! {
        let _shutdown_guard = shutdown_guard;
        let Some(mut principal) = live_access(&state, &principal, frame.connect_frame.as_deref()) else {
            yield Ok(stream_error("auth_required", "Web access expired; reopen this workbench".into()));
            return;
        };
        let mut access_poll = tokio::time::interval(LIVE_ACCESS_INTERVAL);
        access_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        yield Ok(connected_event());
        loop {
            let received = tokio::select! {
                _ = shutdown.recv() => break,
                _ = access_poll.tick() => {
                    let Some(current) = live_access(&state, &principal, frame.connect_frame.as_deref()) else {
                        yield Ok(stream_error("auth_required", "Web access was revoked; sign in again".into()));
                        break;
                    };
                    principal = current;
                    continue;
                },
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
    Box::pin(stream)
}

async fn group_events(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
    Extension(principal): Extension<Principal>,
    Query(frame): Query<ResourceFrameQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let cursor = headers
        .get(HeaderName::from_static("last-event-id"))
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .trim()
        .to_owned();
    Ok(Sse::new(
        group_source(state, group_id, principal, frame, cursor)?
            .map(|event| event.map(StreamEvent::into_sse)),
    )
    .keep_alive(KeepAlive::default()))
}

pub(super) fn group_source(
    state: AppState,
    group_id: String,
    principal: Principal,
    frame: ResourceFrameQuery,
    last_event_id: String,
) -> Result<EventStream, ApiError> {
    let mut receiver = state
        .ledger_events
        .subscribe_group(&group_id)
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    let event_hub = state.ledger_events.clone();
    let shutdown_guard = state.shutdown.clone();
    let mut shutdown = state.shutdown.subscribe();
    let stream = async_stream::stream! {
        let _shutdown_guard = shutdown_guard;
        let mut access_poll = tokio::time::interval(LIVE_ACCESS_INTERVAL);
        access_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        yield Ok(connected_event());
        let mut cursor = last_event_id;
        let mut replayed = HashSet::new();
        let mut replayed_order = VecDeque::new();
        if !cursor.is_empty() {
            loop {
                if !live_group_access(&state, &principal, &group_id, frame.connect_frame.as_deref()) {
                    yield Ok(stream_error("auth_required", "Web access expired; reopen this Group".into()));
                    return;
                }
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
                _ = access_poll.tick() => {
                    if !live_group_access(&state, &principal, &group_id, frame.connect_frame.as_deref()) {
                        yield Ok(stream_error("auth_required", "Web access expired; reopen this Group".into()));
                        break;
                    }
                    continue;
                },
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
                        if !live_group_access(&state, &principal, &group_id, frame.connect_frame.as_deref()) {
                            yield Ok(stream_error("auth_required", "Web access expired; reopen this Group".into()));
                            return;
                        }
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
    Ok(Box::pin(stream))
}

fn stream_error(code: &str, message: String) -> StreamEvent {
    StreamEvent::error(code, message)
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

fn connected_event() -> StreamEvent {
    StreamEvent::connected()
}

#[cfg(test)]
#[path = "streams_tests.rs"]
mod tests;
