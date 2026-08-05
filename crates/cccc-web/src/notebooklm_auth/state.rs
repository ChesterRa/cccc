use cccc_contracts::utc_now;
use serde_json::{Value, json};
use tokio::sync::watch;

use super::AuthFlowManager;

impl AuthFlowManager {
    pub(super) async fn was_canceled(
        &self,
        session_id: &str,
        cancel: &watch::Receiver<bool>,
    ) -> bool {
        *cancel.borrow()
            || self
                .inner
                .lock()
                .await
                .active
                .as_ref()
                .is_none_or(|active| active.session_id != session_id)
    }

    pub(super) async fn update(
        &self,
        session_id: &str,
        state: &'static str,
        phase: &'static str,
        message: &str,
        error: Option<String>,
    ) {
        let mut inner = self.inner.lock().await;
        if inner
            .active
            .as_ref()
            .is_none_or(|active| active.session_id != session_id)
        {
            return;
        }
        inner.state.state = state;
        inner.state.phase = phase;
        inner.state.message = message.into();
        inner.state.error = error.map_or(
            Value::Null,
            |message| json!({"code":"group_space_provider_auth_failed","message":message}),
        );
        inner.state.updated_at = utc_now();
    }

    pub(super) async fn finish(
        &self,
        session_id: &str,
        state: &'static str,
        phase: &'static str,
        message: &str,
        error: Option<String>,
    ) {
        self.update(session_id, state, phase, message, error).await;
        let mut inner = self.inner.lock().await;
        if inner
            .active
            .as_ref()
            .is_some_and(|active| active.session_id == session_id)
        {
            inner.state.finished_at = utc_now();
            inner.state.updated_at = inner.state.finished_at.clone();
            inner.active = None;
        }
    }

    pub(super) async fn clear_active(&self, session_id: &str) {
        let mut inner = self.inner.lock().await;
        if inner
            .active
            .as_ref()
            .is_some_and(|active| active.session_id == session_id)
        {
            inner.active = None;
        }
    }
}
