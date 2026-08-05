use crate::AppState;
use crate::api::{ApiResult, call, object, success};
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use cccc_core::runtime_activity::RuntimeActivityEvent;
use chrono::{DateTime, Utc};
use futures_util::Stream;
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
use std::convert::Infallible;
use std::time::Duration;

const RECENT_COMPLETED_SECONDS: i64 = 15;
const STUCK_AFTER_SECONDS: i64 = 60;

#[derive(Debug, Deserialize)]
struct StreamQuery {
    #[serde(default = "default_true")]
    replay: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/groups/{group_id}/runtime-activity/snapshot",
            get(snapshot),
        )
        .route(
            "/api/v1/groups/{group_id}/runtime-activity/stream",
            get(stream),
        )
}

async fn snapshot(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    validate_group(&state, &group_id).await?;
    let home = state.home.clone();
    let events = tokio::task::spawn_blocking(move || {
        cccc_core::runtime_activity::read_events(&home, &group_id)
            .map(|events| project_snapshot(events, Utc::now()))
    })
    .await
    .map_err(|error| {
        crate::api::ApiError::unavailable("runtime_activity_snapshot_failed", error.to_string())
    })?
    .map_err(|error| {
        crate::api::ApiError::unavailable("runtime_activity_snapshot_failed", error.to_string())
    })?;
    Ok(success(json!({"count":events.len(),"events":events})))
}

async fn stream(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut shutdown = state.shutdown.subscribe();
    let home = state.home.clone();
    let output = async_stream::stream! {
        let mut seen = HashSet::new();
        if let Ok(events) = cccc_core::runtime_activity::read_events(&home, &group_id) {
            if query.replay {
                for activity in project_snapshot(events.clone(), Utc::now()) {
                    seen.insert(activity.id.clone());
                    yield Ok(sse_event(activity));
                }
            }
            seen.extend(events.into_iter().map(|event| event.id));
        }
        loop {
            tokio::select! {
                _ = shutdown.recv() => break,
                _ = tokio::time::sleep(Duration::from_millis(300)) => {
                    let Ok(events) = cccc_core::runtime_activity::read_events(&home, &group_id) else {
                        continue;
                    };
                    let fresh = events
                        .iter()
                        .filter(|event| !seen.contains(&event.id))
                        .cloned()
                        .collect::<Vec<_>>();
                    for activity in fresh {
                        seen.insert(activity.id.clone());
                        yield Ok(sse_event(activity));
                    }
                    for activity in stuck_events(&events, Utc::now()) {
                        if seen.insert(activity.id.clone()) {
                            yield Ok(sse_event(activity));
                        }
                    }
                    if seen.len() > 1024 {
                        let current = events.iter().map(|event| event.id.clone()).collect::<HashSet<_>>();
                        seen.retain(|id| current.contains(id) || id.starts_with("stuck:"));
                    }
                }
            }
        }
    };
    Sse::new(output).keep_alive(KeepAlive::default())
}

fn sse_event(activity: RuntimeActivityEvent) -> Event {
    Event::default()
        .event("runtime-activity")
        .id(activity.id.clone())
        .json_data(activity)
        .unwrap_or_default()
}

fn project_snapshot(
    events: Vec<RuntimeActivityEvent>,
    now: DateTime<Utc>,
) -> Vec<RuntimeActivityEvent> {
    let mut latest = BTreeMap::<(String, String), RuntimeActivityEvent>::new();
    for event in &events {
        latest.insert(
            (event.actor_id.clone(), event.activity_id.clone()),
            event.clone(),
        );
    }
    let mut projected = latest
        .into_values()
        .filter(|event| {
            is_active(event)
                || event_age_seconds(event, now).is_some_and(|age| age <= RECENT_COMPLETED_SECONDS)
        })
        .collect::<Vec<_>>();
    projected.extend(stuck_events(&events, now));
    projected.sort_by(|left, right| left.ts.cmp(&right.ts).then(left.id.cmp(&right.id)));
    projected
}

fn stuck_events(events: &[RuntimeActivityEvent], now: DateTime<Utc>) -> Vec<RuntimeActivityEvent> {
    let mut latest = BTreeMap::<(String, String), &RuntimeActivityEvent>::new();
    for event in events {
        latest.insert((event.actor_id.clone(), event.activity_id.clone()), event);
    }
    let actors_with_active_tools = latest
        .values()
        .filter(|event| event.kind == "tool" && is_active(event))
        .map(|event| event.actor_id.clone())
        .collect::<HashSet<_>>();
    latest
        .into_values()
        .filter(|event| {
            is_active(event)
                && matches!(event.kind.as_str(), "turn" | "tool")
                && !(event.kind == "turn"
                    && actors_with_active_tools.contains(event.actor_id.as_str()))
                && event_age_seconds(event, now).is_some_and(|age| age >= STUCK_AFTER_SECONDS)
        })
        .map(|event| RuntimeActivityEvent {
            id: format!("stuck:{}", event.id),
            ts: now.to_rfc3339(),
            status: "stuck".into(),
            event_type: "StuckDetected".into(),
            duration_ms: event_age_seconds(event, now)
                .and_then(|seconds| u64::try_from(seconds.saturating_mul(1000)).ok()),
            ..event.clone()
        })
        .collect()
}

fn is_active(event: &RuntimeActivityEvent) -> bool {
    matches!(event.status.as_str(), "started" | "waiting")
}

fn event_age_seconds(event: &RuntimeActivityEvent, now: DateTime<Utc>) -> Option<i64> {
    DateTime::parse_from_rfc3339(&event.ts)
        .ok()
        .map(|timestamp| (now - timestamp.with_timezone(&Utc)).num_seconds().max(0))
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

fn default_true() -> bool {
    true
}

#[cfg(test)]
#[path = "runtime_activity_tests.rs"]
mod tests;
