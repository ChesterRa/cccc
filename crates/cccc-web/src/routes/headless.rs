use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use cccc_contracts::utc_now;
use futures_util::Stream;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::time::Duration;
use uuid::Uuid;

use crate::AppState;
use crate::api::{ApiResult, call, object, success};

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
    let events = collect(&state, &group_id, &mut BTreeMap::new(), true).await?;
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
    let output = async_stream::stream! {
        let mut cursors = BTreeMap::<String,u64>::new();
        let mut first = true;
        loop {
            if let Ok(events) = collect(&state,&group_id,&mut cursors, first && query.replay).await {
                for item in events {
                    yield Ok(Event::default().event("headless").json_data(item).unwrap_or_default());
                }
            }
            first=false;
            tokio::select! {
                _ = shutdown.recv() => break,
                _ = tokio::time::sleep(Duration::from_millis(300)) => {},
            }
        }
    };
    Sse::new(output).keep_alive(KeepAlive::default())
}

async fn collect(
    state: &AppState,
    group_id: &str,
    cursors: &mut BTreeMap<String, u64>,
    replay: bool,
) -> Result<Vec<Value>, crate::api::ApiError> {
    let actors = call(
        state,
        "actor_list",
        object(json!({"group_id":group_id,"by":"user"})),
    )
    .await?;
    let actors = actors.0["result"]["actors"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut events = Vec::new();
    for actor in actors.iter().filter(|actor| actor["runner"] == "headless") {
        let actor_id = actor["id"].as_str().unwrap_or("");
        let after = cursors
            .get(actor_id)
            .copied()
            .unwrap_or(if replay { 0 } else { u64::MAX });
        let Ok(response) = call(
            state,
            "terminal_since",
            object(json!({"group_id":group_id,"actor_id":actor_id,"after":after,"limit_bytes":2_000_000})),
        )
        .await
        else {
            continue;
        };
        let history = &response.0["result"]["history"];
        let start = history["start_cursor"].as_u64().unwrap_or(after);
        let end = history["end_cursor"].as_u64().unwrap_or(start);
        cursors.insert(actor_id.to_owned(), end);
        let text = history["data"].as_str().unwrap_or("");
        if text.is_empty() {
            continue;
        }
        events.push(json!({"id":format!("he_{}",&Uuid::new_v4().simple().to_string()[..16]),"ts":utc_now(),"group_id":group_id,"actor_id":actor_id,"type":"output","data":{"text":text,"start_cursor":start,"end_cursor":end}}));
    }
    Ok(events)
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
