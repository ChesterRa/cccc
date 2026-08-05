use cccc_contracts::DaemonRequest;
use serde_json::{Value, json};

use crate::AppState;

pub(super) struct SessionClose {
    state: AppState,
    args: Option<Value>,
}

impl SessionClose {
    pub(super) fn new(state: AppState, route: &Value, generation: &str) -> Self {
        let mut args = route.clone();
        args["generation"] = json!(generation);
        Self {
            state,
            args: Some(args),
        }
    }

    pub(super) async fn close(&mut self) {
        let Some(args) = self.args.take() else {
            return;
        };
        close(self.state.clone(), args).await;
    }
}

impl Drop for SessionClose {
    fn drop(&mut self) {
        let Some(args) = self.args.take() else {
            return;
        };
        let state = self.state.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(close(state, args));
        } else {
            tracing::error!("group bridge session dropped without a Tokio runtime; close not sent");
        }
    }
}

async fn close(state: AppState, args: Value) {
    let result = state
        .client
        .call(&DaemonRequest {
            v: 1,
            op: "group_bridge_session_close".into(),
            args: args.as_object().cloned().unwrap_or_default(),
        })
        .await;
    match result {
        Ok(response) if response.ok => {}
        Ok(response) => tracing::warn!(?response.error, "group bridge session close failed"),
        Err(error) => tracing::warn!(%error, "group bridge session close unavailable"),
    }
}
