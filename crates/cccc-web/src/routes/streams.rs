use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use futures_util::Stream;
use std::collections::{HashSet, VecDeque};
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::AppState;
use crate::api::ApiError;

const GLOBAL_EVENT_NAME: &str = "event";
const GROUP_LEDGER_EVENT_NAME: &str = "ledger";

fn sse_event(name: &'static str, event: cccc_contracts::Event) -> Event {
    let event_id = event.id.clone();
    Event::default()
        .event(name)
        .id(event_id)
        .json_data(event)
        .unwrap_or_default()
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/events/stream", get(global_events))
        .route("/api/v1/groups/{group_id}/ledger/stream", get(group_events))
}

async fn global_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.ledger_events.subscribe_global();
    let stream = async_stream::stream! {
        yield Ok(connected_event());
        loop {
            match receiver.recv().await {
                Ok(event) => yield Ok(sse_event(GLOBAL_EVENT_NAME, event)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
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
    let stream = async_stream::stream! {
        yield Ok(connected_event());
        let mut cursor = last_event_id;
        let mut replayed = HashSet::new();
        let mut replayed_order = VecDeque::new();
        if !cursor.is_empty() {
            loop {
                let page = event_hub.replay_after(&group_id, &cursor, 2048).unwrap_or_default();
                let count = page.len();
                for event in page {
                    cursor.clone_from(&event.id);
                    remember_replayed(&mut replayed, &mut replayed_order, &event.id);
                    yield Ok(sse_event(GROUP_LEDGER_EVENT_NAME, event));
                }
                if count < 2048 { break; }
            }
        }
        loop {
            match receiver.recv().await {
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
                        let page = event_hub.replay_after(&group_id, &cursor, 2048).unwrap_or_default();
                        let count = page.len();
                        for event in page {
                            cursor.clone_from(&event.id);
                            remember_replayed(&mut replayed, &mut replayed_order, &event.id);
                            yield Ok(sse_event(GROUP_LEDGER_EVENT_NAME, event));
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
mod tests {
    use super::*;
    use crate::{browser_surface, im_runtime, ledger_event_hub};
    use axum::response::IntoResponse;
    use cccc_client::DaemonClient;
    use cccc_core::{GroupStore, HomeLayout, ledger};
    use futures_util::{StreamExt, stream};
    use std::sync::Arc;

    async fn encoded_event_name(name: &'static str) -> String {
        let event = cccc_contracts::Event::new("chat.message", "g_test");
        let response =
            Sse::new(stream::iter([Ok::<_, Infallible>(sse_event(name, event))])).into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read SSE body");
        String::from_utf8(body.to_vec()).expect("SSE is UTF-8")
    }

    #[tokio::test]
    async fn stream_event_names_match_frontend_listeners() {
        assert!(
            encoded_event_name(GLOBAL_EVENT_NAME)
                .await
                .contains("event: event\n")
        );
        assert!(
            encoded_event_name(GROUP_LEDGER_EVENT_NAME)
                .await
                .contains("event: ledger\n")
        );
        assert!(
            encoded_event_name(GROUP_LEDGER_EVENT_NAME)
                .await
                .contains("id: ")
        );
    }

    #[tokio::test]
    async fn last_event_id_replay_crosses_multiple_pages_without_gaps() {
        const REPLAY_COUNT: usize = 2_050;
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("paged SSE replay", "").expect("group");
        let path = store.ledger_path(&group.group_id).expect("ledger path");
        let cursor = cccc_contracts::Event::new("chat.message", &group.group_id);
        ledger::append(&path, &cursor).expect("cursor");
        let expected = (0..REPLAY_COUNT)
            .map(|index| {
                let mut event = cccc_contracts::Event::new("chat.message", &group.group_id);
                event.data.insert("index".into(), serde_json::json!(index));
                ledger::append(&path, &event).expect("append replay event");
                event.id
            })
            .collect::<Vec<_>>();
        let hub = ledger_event_hub::LedgerEventHub::new(home.clone());
        let state = AppState {
            client: DaemonClient::new(home.clone()),
            home,
            browser_surfaces: Arc::new(browser_surface::BrowserSurfaces::default()),
            ledger_events: hub,
            im_workers: Arc::new(im_runtime::ImWorkerRegistry::default()),
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("last-event-id"),
            cursor.id.parse().expect("header value"),
        );
        let response = group_events(State(state), Path(group.group_id), headers)
            .await
            .expect("group stream")
            .into_response();
        let mut body = response.into_body().into_data_stream();
        let mut received = Vec::with_capacity(REPLAY_COUNT);
        while received.len() < REPLAY_COUNT {
            let chunk = tokio::time::timeout(Duration::from_secs(2), body.next())
                .await
                .expect("SSE replay timeout")
                .expect("SSE body ended")
                .expect("SSE body chunk");
            let text = String::from_utf8(chunk.to_vec()).expect("SSE is UTF-8");
            for line in text.lines().filter_map(|line| line.strip_prefix("id: ")) {
                received.push(line.to_owned());
            }
        }
        assert_eq!(received, expected);
    }

    #[tokio::test]
    async fn initial_replay_suppresses_events_already_queued_by_the_subscription() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("SSE replay race", "").expect("group");
        let path = store.ledger_path(&group.group_id).expect("ledger path");
        let cursor = cccc_contracts::Event::new("chat.message", &group.group_id);
        ledger::append(&path, &cursor).expect("cursor");
        let hub = ledger_event_hub::LedgerEventHub::new(home.clone());
        let state = AppState {
            client: DaemonClient::new(home.clone()),
            home,
            browser_surfaces: Arc::new(browser_surface::BrowserSurfaces::default()),
            ledger_events: hub,
            im_workers: Arc::new(im_runtime::ImWorkerRegistry::default()),
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("last-event-id"),
            cursor.id.parse().expect("header value"),
        );
        let response = group_events(State(state), Path(group.group_id.clone()), headers)
            .await
            .expect("group stream")
            .into_response();

        let expected = (0..2)
            .map(|_| {
                let event = cccc_contracts::Event::new("chat.message", &group.group_id);
                ledger::append(&path, &event).expect("append queued event");
                event.id
            })
            .collect::<Vec<_>>();
        tokio::time::sleep(Duration::from_millis(150)).await;

        let mut body = response.into_body().into_data_stream();
        let mut received = Vec::new();
        while received.len() < expected.len() {
            let chunk = tokio::time::timeout(Duration::from_secs(2), body.next())
                .await
                .expect("SSE replay timeout")
                .expect("SSE body ended")
                .expect("SSE body chunk");
            let text = String::from_utf8(chunk.to_vec()).expect("SSE is UTF-8");
            received.extend(
                text.lines()
                    .filter_map(|line| line.strip_prefix("id: "))
                    .map(str::to_owned),
            );
        }
        assert_eq!(received, expected);
        assert!(
            tokio::time::timeout(Duration::from_millis(150), body.next())
                .await
                .is_err()
        );
    }
}
