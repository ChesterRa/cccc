use cccc_client::DaemonClient;
use cccc_contracts::Event;
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

mod dingtalk;
mod discord;
mod feishu;
mod slack;
mod state;
mod telegram;
mod wecom;
mod wecom_client;
mod wecom_media;
mod wecom_message;
mod wecom_outbound;
mod weixin;
mod weixin_login;

use state::*;

#[derive(Default)]
pub(crate) struct ImWorkerRegistry {
    workers: Mutex<HashMap<String, WorkerHandles>>,
    restoring: Mutex<HashSet<String>>,
    weixin_logins: weixin_login::LoginRegistry,
}

struct WorkerHandles {
    tasks: Vec<JoinHandle<()>>,
    stoppers: Vec<Box<dyn Fn() + Send + Sync>>,
}

impl Drop for WorkerHandles {
    fn drop(&mut self) {
        for stop in &self.stoppers {
            stop();
        }
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl ImWorkerRegistry {
    pub(crate) fn restore_enabled(self: &Arc<Self>, home: HomeLayout, client: DaemonClient) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let candidates = restore_candidates(&home);
        if let Ok(mut restoring) = self.restoring.lock() {
            restoring.extend(candidates.iter().map(|(group_id, _)| group_id.clone()));
        }
        for (group_id, config) in candidates {
            let registry = Arc::clone(self);
            let home = home.clone();
            let client = client.clone();
            runtime.spawn(async move {
                let result = registry
                    .start(home.clone(), client, &group_id, &config)
                    .await;
                if let Ok(store) = GroupStore::new(home)
                    && let Err(error) = cccc_core::integration_state::group_update(
                        &store,
                        &group_id,
                        "im_bridge",
                        |value| {
                            if !value.is_object() {
                                *value = json!({});
                            }
                            let state = value.as_object_mut().expect("IM state initialized");
                            state.insert("running".into(), Value::Bool(result.is_ok()));
                            state.insert("adapter_available".into(), Value::Bool(result.is_ok()));
                            state.insert(
                                "pid".into(),
                                if result.is_ok() {
                                    json!(std::process::id())
                                } else {
                                    Value::Null
                                },
                            );
                            state.insert(
                                "last_error".into(),
                                result
                                    .as_ref()
                                    .err()
                                    .map_or(Value::Null, |error| json!(error)),
                            );
                            state.insert("updated_at".into(), json!(cccc_contracts::utc_now()));
                            Ok(())
                        },
                    )
                {
                    tracing::warn!(%error, %group_id, "failed to persist restored IM worker state");
                }
                registry
                    .restoring
                    .lock()
                    .expect("IM restore registry poisoned")
                    .remove(&group_id);
                if let Err(error) = result {
                    tracing::warn!(%error, %group_id, "failed to restore enabled IM worker");
                }
            });
        }
    }

    pub(crate) async fn start_weixin_login(&self, group_id: &str) -> Result<Value, String> {
        self.weixin_logins.start(group_id).await
    }

    pub(crate) async fn weixin_login_status(
        &self,
        home: &HomeLayout,
        group_id: &str,
    ) -> Result<Value, String> {
        self.weixin_logins.status(home, group_id).await
    }

    pub(crate) fn logout_weixin(&self, home: &HomeLayout, group_id: &str) -> Value {
        self.stop(group_id);
        self.weixin_logins.clear(group_id);
        weixin_login::remove_credentials(home, group_id);
        json!({
            "status":"logged_out","logged_in":false,"running":false,
            "pid":null,"updated_at":cccc_contracts::utc_now()
        })
    }

    pub(crate) async fn start(
        &self,
        home: HomeLayout,
        client: DaemonClient,
        group_id: &str,
        config: &Map<String, Value>,
    ) -> Result<(), String> {
        let platform = string(config, "platform");
        if platform == "telegram" {
            let tasks = telegram::start(home, client, group_id, config).await?;
            self.workers
                .lock()
                .expect("IM worker registry poisoned")
                .insert(group_id.to_owned(), worker(tasks));
            return Ok(());
        }
        if platform == "discord" {
            let tasks = discord::start(home, client, group_id, config).await?;
            self.workers
                .lock()
                .expect("IM worker registry poisoned")
                .insert(group_id.to_owned(), worker(tasks));
            return Ok(());
        }
        if platform == "slack" {
            let tasks = slack::start(home, client, group_id, config).await?;
            self.workers
                .lock()
                .expect("IM worker registry poisoned")
                .insert(group_id.to_owned(), worker(tasks));
            return Ok(());
        }
        if platform == "feishu" {
            let tasks = feishu::start(home, client, group_id, config).await?;
            self.workers
                .lock()
                .expect("IM worker registry poisoned")
                .insert(group_id.to_owned(), worker(tasks));
            return Ok(());
        }
        if platform == "wecom" {
            let (tasks, sdk) = wecom::start(home, client, group_id, config).await?;
            self.workers
                .lock()
                .expect("IM worker registry poisoned")
                .insert(
                    group_id.to_owned(),
                    WorkerHandles {
                        tasks,
                        stoppers: vec![Box::new(move || sdk.shutdown())],
                    },
                );
            return Ok(());
        }
        if platform == "weixin" {
            let (tasks, sdk) = weixin::start(home, client, group_id).await?;
            self.workers
                .lock()
                .expect("IM worker registry poisoned")
                .insert(
                    group_id.to_owned(),
                    WorkerHandles {
                        tasks,
                        stoppers: vec![Box::new(move || sdk.shutdown())],
                    },
                );
            return Ok(());
        }
        if platform == "dingtalk" {
            let tasks = dingtalk::start(home, client, group_id, config).await?;
            self.workers
                .lock()
                .expect("IM worker registry poisoned")
                .insert(group_id.to_owned(), worker(tasks));
            return Ok(());
        }
        Err(format!(
            "Rust network adapter is not migrated for platform {platform}"
        ))
    }

    pub(crate) fn stop(&self, group_id: &str) -> bool {
        let Some(_worker) = self
            .workers
            .lock()
            .expect("IM worker registry poisoned")
            .remove(group_id)
        else {
            return false;
        };
        true
    }

    pub(crate) fn is_running(&self, group_id: &str) -> bool {
        if self
            .restoring
            .lock()
            .expect("IM restore registry poisoned")
            .contains(group_id)
        {
            return true;
        }
        let mut workers = self.workers.lock().expect("IM worker registry poisoned");
        let finished = workers
            .get(group_id)
            .is_some_and(|worker| worker.tasks.iter().any(JoinHandle::is_finished));
        if finished {
            workers.remove(group_id);
            return false;
        }
        workers.contains_key(group_id)
    }
}

fn restore_candidates(home: &HomeLayout) -> Vec<(String, Map<String, Value>)> {
    let Ok(store) = GroupStore::new(home.clone()) else {
        return Vec::new();
    };
    store
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|meta| {
            let state =
                cccc_core::integration_state::group_get(&store, &meta.group_id, "im_bridge")
                    .ok()?;
            if !state["enabled"].as_bool().unwrap_or(false) {
                return None;
            }
            Some((meta.group_id, state.get("config")?.as_object()?.clone()))
        })
        .collect()
}

fn worker(tasks: Vec<JoinHandle<()>>) -> WorkerHandles {
    WorkerHandles {
        tasks,
        stoppers: Vec::new(),
    }
}

pub(super) fn spawn_outbound<S, F, Fut>(
    home: HomeLayout,
    group_id: String,
    sender: S,
    send: F,
) -> JoinHandle<()>
where
    S: Send + Sync + 'static,
    F: Fn(Arc<S>, Vec<String>, Event) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    spawn_outbound_matching(home, group_id, sender, is_outbound, send)
}

pub(super) fn spawn_outbound_matching<S, P, F, Fut>(
    home: HomeLayout,
    group_id: String,
    sender: S,
    matches: P,
    send: F,
) -> JoinHandle<()>
where
    S: Send + Sync + 'static,
    P: Fn(&Event) -> bool + Send + Sync + 'static,
    F: Fn(Arc<S>, Vec<String>, Event) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let sender = Arc::new(sender);
    tokio::spawn(async move {
        let Ok(store) = GroupStore::new(home.clone()) else {
            return;
        };
        let Ok(path) = store.ledger_path(&group_id) else {
            return;
        };
        let mut seen: HashSet<String> = ledger::tail(&path, 1000)
            .unwrap_or_default()
            .into_iter()
            .map(|event| event.id)
            .collect();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(750)).await;
            for event in ledger::tail(&path, 1000).unwrap_or_default() {
                if !seen.insert(event.id.clone()) || !matches(&event) {
                    continue;
                }
                let targets = authorized_chat_ids(&home, &group_id).into_iter().collect();
                send(Arc::clone(&sender), targets, event).await;
            }
            if seen.len() > 4096 {
                seen = ledger::tail(&path, 1000)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|event| event.id)
                    .collect();
            }
        }
    })
}

pub(super) fn outbound_text(event: &Event, markdown_bold: bool) -> Option<String> {
    let text = event.data.get("text").and_then(Value::as_str)?;
    Some(if markdown_bold {
        format!("**{}**\n\n{}", event.by, text)
    } else {
        format!("{}\n\n{}", event.by, text)
    })
}

pub(super) fn is_outbound(event: &Event) -> bool {
    event.kind == "chat.message"
        && event.by != "user"
        && !event.by.starts_with("im:")
        && event.data.get("transport").and_then(Value::as_str) != Some("im")
}

pub(super) fn is_outbound_or_stream(event: &Event) -> bool {
    matches!(event.kind.as_str(), "chat.message" | "chat.stream")
        && event.by != "user"
        && !event.by.starts_with("im:")
        && event.data.get("transport").and_then(Value::as_str) != Some("im")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn finished_worker_aborts_sibling_tasks_and_runs_stoppers() {
        let registry = ImWorkerRegistry::default();
        let finished = tokio::spawn(async {});
        let sibling = tokio::spawn(std::future::pending());
        let sibling_abort = sibling.abort_handle();
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped_on_drop = Arc::clone(&stopped);
        registry.workers.lock().expect("registry").insert(
            "g_test".into(),
            WorkerHandles {
                tasks: vec![finished, sibling],
                stoppers: vec![Box::new(move || {
                    stopped_on_drop.store(true, Ordering::SeqCst);
                })],
            },
        );

        tokio::task::yield_now().await;
        assert!(!registry.is_running("g_test"));
        tokio::task::yield_now().await;
        assert!(sibling_abort.is_finished());
        assert!(stopped.load(Ordering::SeqCst));
    }

    #[test]
    fn restore_only_selects_enabled_configured_groups() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let enabled = store.create("enabled", "").expect("enabled");
        let disabled = store.create("disabled", "").expect("disabled");
        for (group_id, active) in [(&enabled.group_id, true), (&disabled.group_id, false)] {
            cccc_core::integration_state::group_update(&store, group_id, "im_bridge", |state| {
                *state = json!({
                    "enabled":active,
                    "config":{"platform":"telegram","bot_token_env":"TOKEN"}
                });
                Ok(())
            })
            .expect("state");
        }
        let candidates = restore_candidates(&home);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0, enabled.group_id);
    }

    #[test]
    fn outbound_filter_does_not_echo_users_or_im_ingress() {
        let mut actor = Event::new("chat.message", "g_test");
        actor.by = "foreman".into();
        assert!(is_outbound(&actor));
        actor.by = "im:dingtalk:user-1".into();
        assert!(!is_outbound(&actor));
        actor.by = "user".into();
        assert!(!is_outbound(&actor));
    }

    #[test]
    fn authorization_parser_accepts_configured_chat_ids_across_platforms() {
        let value = json!([
            {"platform":"dingtalk","chat_id":"cid-1"},
            {"platform":"telegram","chat_id":"chat-2"}
        ]);
        let mut ids = HashSet::new();
        collect_chat_ids(Some(&value), &mut ids);
        assert_eq!(
            ids,
            HashSet::from(["cid-1".to_owned(), "chat-2".to_owned()])
        );
    }

    #[test]
    fn subscribe_creates_pending_request_without_authorizing_chat() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("test", "").expect("group");
        assert!(!accepts_inbound(
            &home,
            &group.group_id,
            "telegram",
            "chat-1",
            "/subscribe"
        ));
        assert!(!accepts_inbound(
            &home,
            &group.group_id,
            "telegram",
            "chat-2",
            "hello"
        ));
        let state = cccc_core::integration_state::group_get(&store, &group.group_id, "im_bridge")
            .expect("state");
        let pending = state["pending"].as_array().expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0]["chat_id"], "chat-1");
        assert_eq!(pending[0]["platform"], "telegram");
    }
}
