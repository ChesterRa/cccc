mod run;
mod state;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cccc_client::DaemonClient;
use cccc_contracts::utc_now;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::browser_surface::BrowserSurfaces;

pub(super) const PROVIDER: &str = "notebooklm";
pub(super) const BROWSER_KEY: &str = "space-provider::notebooklm";
pub(super) const LOGIN_URL: &str = "https://notebooklm.google.com/";
pub(super) const BROWSER_WIDTH: u32 = 1366;
pub(super) const BROWSER_HEIGHT: u32 = 900;

#[derive(Clone)]
struct FlowState {
    state: &'static str,
    phase: &'static str,
    session_id: String,
    started_at: String,
    finished_at: String,
    updated_at: String,
    message: String,
    error: Value,
}

impl Default for FlowState {
    fn default() -> Self {
        Self {
            state: "idle",
            phase: "idle",
            session_id: String::new(),
            started_at: String::new(),
            finished_at: String::new(),
            updated_at: utc_now(),
            message: "Authentication browser is idle.".into(),
            error: Value::Null,
        }
    }
}

struct ActiveFlow {
    session_id: String,
    profile: PathBuf,
    cancel: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

pub(super) struct RunContext {
    pub(super) home: HomeLayout,
    pub(super) client: DaemonClient,
    pub(super) browsers: Arc<BrowserSurfaces>,
    pub(super) session_id: String,
    pub(super) profile: PathBuf,
    pub(super) cancel: watch::Receiver<bool>,
    pub(super) timeout: Duration,
    pub(super) force_reauth: bool,
}

#[derive(Default)]
struct Inner {
    state: FlowState,
    active: Option<ActiveFlow>,
}

#[derive(Default)]
pub(crate) struct AuthFlowManager {
    inner: Mutex<Inner>,
}

impl AuthFlowManager {
    pub(crate) async fn start(
        self: &Arc<Self>,
        home: HomeLayout,
        client: DaemonClient,
        browsers: Arc<BrowserSurfaces>,
        timeout_seconds: u64,
        force_reauth: bool,
    ) {
        let session_id = format!("nbl_auth_{}", &Uuid::new_v4().simple().to_string()[..12]);
        let profile = home
            .root()
            .join("state/notebooklm_auth/browser_profiles")
            .join(&session_id);
        let (cancel, receiver) = watch::channel(false);
        {
            let mut inner = self.inner.lock().await;
            if inner.active.is_some() {
                return;
            }
            let now = utc_now();
            inner.state = FlowState {
                state: "running",
                phase: "checking_saved_session",
                session_id: session_id.clone(),
                started_at: now.clone(),
                finished_at: String::new(),
                updated_at: now,
                message: "Checking the saved Google session...".into(),
                error: Value::Null,
            };
            inner.active = Some(ActiveFlow {
                session_id: session_id.clone(),
                profile: profile.clone(),
                cancel,
                task: None,
            });
        }
        let manager = Arc::clone(self);
        let task_session_id = session_id.clone();
        let context = RunContext {
            home,
            client,
            browsers,
            session_id,
            profile,
            cancel: receiver,
            timeout: run::auth_timeout(timeout_seconds),
            force_reauth,
        };
        let mut pending_task = Some(tokio::spawn(async move { manager.run(context).await }));
        {
            let mut inner = self.inner.lock().await;
            if let Some(active) = inner
                .active
                .as_mut()
                .filter(|active| active.session_id == task_session_id)
            {
                active.task = pending_task.take();
            }
        }
        if let Some(task) = pending_task {
            task.abort();
            let _ = task.await;
        }
    }

    pub(crate) async fn cancel(&self, browsers: &BrowserSurfaces, message: &str) {
        let active = {
            let mut inner = self.inner.lock().await;
            let active = inner.active.take();
            if let Some(active) = &active {
                let _ = active.cancel.send(true);
            }
            if active.is_some() {
                inner.state.state = "canceled";
                inner.state.phase = "canceled";
                inner.state.finished_at = utc_now();
                inner.state.updated_at = inner.state.finished_at.clone();
                inner.state.message = message.into();
                inner.state.error = Value::Null;
            }
            active
        };
        if let Some(mut active) = active {
            if let Some(mut task) = active.task.take()
                && tokio::time::timeout(Duration::from_secs(2), &mut task)
                    .await
                    .is_err()
            {
                task.abort();
                let _ = task.await;
            }
            run::close_browser_and_remove_profile(browsers, &active.profile).await;
        }
    }

    pub(crate) async fn shutdown(&self, browsers: &BrowserSurfaces) {
        self.cancel(
            browsers,
            "Authentication stopped because CCCC is shutting down.",
        )
        .await;
    }

    pub(crate) async fn snapshot(&self, browsers: &BrowserSurfaces) -> Value {
        let state = self.inner.lock().await.state.clone();
        let mut surface = browsers.info(BROWSER_KEY).await;
        if surface["active"].as_bool() == Some(true) {
            let ready = browsers
                .notebooklm_auth_ready(BROWSER_KEY)
                .await
                .unwrap_or(false);
            if let Some(metadata) = surface["metadata"].as_object_mut() {
                metadata.insert("auth_ready".into(), Value::Bool(ready));
            } else {
                surface["metadata"] = json!({"auth_ready":ready});
            }
        }
        json!({
            "provider":PROVIDER,"state":state.state,"phase":state.phase,
            "delivery":"projected_browser","session_id":state.session_id,
            "started_at":state.started_at,"finished_at":state.finished_at,
            "updated_at":state.updated_at,"message":state.message,"error":state.error,
            "projected_browser":surface,
        })
    }
}

pub(crate) async fn remove_legacy_profile(home: &HomeLayout) {
    run::remove_profile(&home.root().join("browser-profiles/space-auth/notebooklm")).await;
}

#[cfg(test)]
mod tests;
