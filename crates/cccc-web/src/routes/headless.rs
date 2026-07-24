use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use futures_util::Stream;
use serde::{Deserialize, Deserializer};
use serde_json::json;
use std::convert::Infallible;
use std::path::PathBuf;
use std::time::Duration;

use crate::AppState;
use crate::api::{ApiResult, call, object, success};
use crate::routes::headless_store::{HeadlessEventTail, read_replay_events};

#[derive(Debug, Deserialize)]
struct StreamQuery {
    #[serde(default = "default_true", deserialize_with = "deserialize_replay")]
    replay: bool,
}

fn deserialize_replay<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    parse_replay(&value)
        .ok_or_else(|| serde::de::Error::custom("replay must be true, false, 1, or 0"))
}

fn parse_replay(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/headless/snapshot", get(snapshot))
        .route("/api/v1/groups/{group_id}/codex/snapshot", get(snapshot))
        .route("/api/v1/groups/{group_id}/headless/stream", get(stream))
        .route("/api/v1/groups/{group_id}/codex/stream", get(stream))
}

async fn snapshot(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    validate_group(&state, &group_id).await?;
    let path = events_path(&state, &group_id);
    let events = tokio::task::spawn_blocking(move || read_replay_events(&path))
        .await
        .map_err(|error| {
            crate::api::ApiError::unavailable("headless_snapshot_failed", error.to_string())
        })?
        .map_err(|error| {
            crate::api::ApiError::unavailable("headless_snapshot_failed", error.to_string())
        })?;
    Ok(success(
        json!({"group_id":group_id,"count":events.len(),"events":events}),
    ))
}

async fn stream(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut shutdown = state.shutdown.subscribe();
    let path = events_path(&state, &group_id);
    let output = async_stream::stream! {
        let (mut tail, replay_events) = match tokio::task::spawn_blocking(move || HeadlessEventTail::open(path, query.replay)).await {
            Ok(Ok(value)) => value,
            _ => return,
        };
        for item in replay_events {
            yield Ok(Event::default().event("headless").json_data(item).unwrap_or_default());
        }
        loop {
            tokio::select! {
                _ = shutdown.recv() => break,
                _ = tokio::time::sleep(Duration::from_millis(300)) => {
                    if let Ok(events) = tail.read_new() {
                        for item in events {
                            yield Ok(Event::default().event("headless").json_data(item).unwrap_or_default());
                        }
                    }
                },
            }
        }
    };
    Sse::new(output).keep_alive(KeepAlive::default())
}

async fn validate_group(state: &AppState, group_id: &str) -> Result<(), crate::api::ApiError> {
    let _ = call(
        state,
        "actor_list",
        object(json!({"group_id":group_id,"by":"user"})),
    )
    .await?;
    Ok(())
}

fn events_path(state: &AppState, group_id: &str) -> PathBuf {
    let home = &state.home;
    let Ok(store) = cccc_core::GroupStore::new(home.clone()) else {
        return PathBuf::new();
    };
    let Ok(directory) = store.state_dir(group_id) else {
        return PathBuf::new();
    };
    directory.join("headless/events.jsonl")
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::parse_replay;

    #[test]
    fn accepts_boolean_and_numeric_replay_values() {
        assert_eq!(parse_replay("true"), Some(true));
        assert_eq!(parse_replay("1"), Some(true));
        assert_eq!(parse_replay("false"), Some(false));
        assert_eq!(parse_replay("0"), Some(false));
        assert_eq!(parse_replay("invalid"), None);
    }
}
