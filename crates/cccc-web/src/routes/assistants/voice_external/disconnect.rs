use super::{active::Active, connection, persistence};
use crate::AppState;
use futures_util::StreamExt;

pub(super) async fn finalize(state: &AppState, group: &str, run: &mut Active) {
    if !run.persist {
        return;
    }
    let result = tokio::time::timeout(connection::FINISH_TIMEOUT, async {
        if !run.stopping {
            run.stop(serde_json::Value::Null).await?;
        }
        while !run.completed {
            let message = run
                .reader
                .next()
                .await
                .ok_or_else(connection::transport_error)?
                .map_err(|_| connection::transport_error())?;
            run.receive(message)?;
            persistence::checkpoints(state, group, run, false).await?;
        }
        persistence::checkpoints(state, group, run, true).await
    })
    .await;
    if matches!(result, Ok(Ok(()))) {
        let _ = persistence::final_event(state, group, run).await;
    } else {
        let _ = persistence::recover(state, group, run).await;
    }
}
