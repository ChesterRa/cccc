use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use cccc_contracts::utc_now;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::time::Duration;
use uuid::Uuid;

use crate::AppState;
use crate::api::{ApiResult, call, object, success};

#[derive(Debug, Deserialize)]
struct StreamQuery {
    #[serde(default = "default_true")]
    replay: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/headless/snapshot", get(snapshot))
        .route("/api/v1/groups/{group_id}/codex/snapshot", get(snapshot))
        .route("/api/v1/groups/{group_id}/headless/stream", get(stream))
        .route("/api/v1/groups/{group_id}/codex/stream", get(stream))
}

async fn snapshot(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    let events = collect(&state, &group_id, &BTreeMap::new(), true).await?;
    Ok(success(
        json!({"group_id":group_id,"count":events.len(),"events":events}),
    ))
}

async fn stream(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let output = async_stream::stream! {
        let mut cursors = BTreeMap::<String,u64>::new();
        let mut first = true;
        loop {
            if let Ok(events) = collect(&state,&group_id,&cursors, first && query.replay).await {
                for item in events {
                    let actor_id=item["actor_id"].as_str().unwrap_or("").to_owned();
                    if let Some(cursor)=item["data"]["end_cursor"].as_u64(){cursors.insert(actor_id,cursor);}
                    yield Ok(Event::default().event("headless").json_data(item).unwrap_or_default());
                }
            }
            first=false;
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    };
    Sse::new(output).keep_alive(KeepAlive::default())
}

async fn collect(
    state: &AppState,
    group_id: &str,
    cursors: &BTreeMap<String, u64>,
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
        let Ok(response) = call(
            state,
            "terminal_tail",
            object(json!({"group_id":group_id,"actor_id":actor_id,"max_chars":2_000_000})),
        )
        .await
        else {
            continue;
        };
        let result = &response.0["result"];
        let end = result["end_cursor"].as_u64().unwrap_or(0);
        let previous = cursors
            .get(actor_id)
            .copied()
            .unwrap_or(if replay { 0 } else { end });
        if end <= previous {
            continue;
        }
        let text = result["text"].as_str().unwrap_or("");
        events.push(json!({"id":format!("he_{}",&Uuid::new_v4().simple().to_string()[..16]),"ts":utc_now(),"group_id":group_id,"actor_id":actor_id,"type":"output","data":{"text":text,"start_cursor":end.saturating_sub(text.len() as u64),"end_cursor":end}}));
    }
    Ok(events)
}

fn default_true() -> bool {
    true
}
