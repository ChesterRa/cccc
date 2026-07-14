use axum::Router;
use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use cccc_core::GroupStore;
use cccc_core::ledger::LedgerFollower;
use futures_util::Stream;
use std::convert::Infallible;
use std::time::Duration;

use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/events/stream", get(global_events))
        .route("/api/v1/groups/{group_id}/ledger/stream", get(group_events))
}

async fn global_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut followers=std::collections::BTreeMap::<String,LedgerFollower>::new();
        loop{
            if let Ok(store)=GroupStore::new(state.home.clone())
                && let Ok(groups)=store.list(){
                let active_groups: std::collections::BTreeSet<_> = groups
                    .iter()
                    .map(|group| group.group_id.clone())
                    .collect();
                followers.retain(|group_id, _| active_groups.contains(group_id));
                for group in groups{
                    if let Ok(path)=store.ledger_path(&group.group_id)
                        && let Ok(events)=followers.entry(group.group_id).or_default().poll(&path){
                        for event in events{yield Ok(Event::default().event("event").json_data(event).unwrap_or_default());}
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn group_events(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut follower=LedgerFollower::default();
        loop{
            if let Ok(store)=GroupStore::new(state.home.clone())
                && let Ok(path)=store.ledger_path(&group_id)
                && let Ok(events)=follower.poll(&path){
                for event in events{yield Ok(Event::default().event("event").json_data(event).unwrap_or_default());}
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}
