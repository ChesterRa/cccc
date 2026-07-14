use axum::Router;
use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use cccc_core::GroupStore;
use cccc_core::ledger;
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
        let mut seen=std::collections::BTreeMap::<String,usize>::new();
        loop{
            if let Ok(store)=GroupStore::new(state.home.clone())
                && let Ok(groups)=store.list(){
                for group in groups{
                    if let Ok(events)=store.ledger_path(&group.group_id).and_then(|path|ledger::read_all(&path)){
                        let start=seen.get(&group.group_id).copied().unwrap_or(events.len());
                        for event in events.iter().skip(start){yield Ok(Event::default().event("event").json_data(event).unwrap_or_default());}
                        seen.insert(group.group_id,events.len());
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
        let mut seen=0usize;
        loop{
            if let Ok(store)=GroupStore::new(state.home.clone())
                && let Ok(path)=store.ledger_path(&group_id)
                && let Ok(events)=ledger::read_all(&path){
                if seen==0{seen=events.len();}
                for event in events.iter().skip(seen){yield Ok(Event::default().event("event").json_data(event).unwrap_or_default());}
                seen=events.len();
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}
