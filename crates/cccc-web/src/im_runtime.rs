use cccc_client::DaemonClient;
use cccc_contracts::Event;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

mod commands;
mod dingtalk;
mod dingtalk_inbound;
mod dingtalk_outbound;
mod dingtalk_outbound_media;
mod dingtalk_outbound_report;
mod dingtalk_streaming;
mod discord;
mod discord_dedup;
mod discord_gateway_proxy;
mod discord_inbound;
mod discord_outbound;
mod discord_reactions;
mod feishu;
mod feishu_inbound;
mod feishu_outbound;
mod inbound_attachments;
mod outbound_attachment;
mod outbound_chunks;
mod outbound_message;
mod outbound_stream_state;
mod processing_reactions;
mod slack;
mod slack_inbound;
mod slack_outbound;
mod state;
mod telegram;
mod telegram_inbound;
mod telegram_outbound;
mod wecom;
mod wecom_client;
mod wecom_media;
mod wecom_message;
mod wecom_outbound;
mod weixin;
mod weixin_authorization;
mod weixin_inbound;
mod weixin_login;
mod weixin_outbound;
mod worker;

use commands::*;
use outbound_message::outbound_text;
use state::*;
use worker::{Stopper, WorkerHandles, no_op_stopper};

pub(crate) struct ImWorkerRegistry {
    workers: Mutex<HashMap<String, WorkerHandles>>,
    lifecycle_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    restoring: Mutex<HashSet<String>>,
    restore_tasks: Mutex<Vec<JoinHandle<()>>>,
    generations: Arc<Mutex<HashMap<String, u64>>>,
    next_generation: std::sync::atomic::AtomicU64,
    discord_deduper: Arc<discord_dedup::DiscordMessageDeduper>,
    weixin_logins: weixin_login::LoginRegistry,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
}

impl ImWorkerRegistry {
    pub(crate) fn new(ledger_events: crate::ledger_event_hub::LedgerEventHub) -> Self {
        Self {
            workers: Mutex::new(HashMap::new()),
            lifecycle_locks: Mutex::new(HashMap::new()),
            restoring: Mutex::new(HashSet::new()),
            restore_tasks: Mutex::new(Vec::new()),
            generations: Arc::new(Mutex::new(HashMap::new())),
            next_generation: std::sync::atomic::AtomicU64::new(0),
            discord_deduper: Arc::new(discord_dedup::DiscordMessageDeduper::default()),
            weixin_logins: weixin_login::LoginRegistry::default(),
            ledger_events,
        }
    }

    pub(crate) fn restore_enabled(self: &Arc<Self>, home: HomeLayout, client: DaemonClient) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let candidates = restore_candidates(&home);
        if let Ok(mut restoring) = self.restoring.lock() {
            restoring.extend(candidates.iter().map(|(group_id, _)| group_id.clone()));
        }
        for (group_id, _snapshot_config) in candidates {
            let registry = Arc::clone(self);
            let home = home.clone();
            let client = client.clone();
            let task = runtime.spawn(async move {
                let Some(config) = restore_config(&home, &group_id) else {
                    registry
                        .restoring
                        .lock()
                        .expect("IM restore registry poisoned")
                        .remove(&group_id);
                    return;
                };
                let result = registry
                    .start(home.clone(), client, &group_id, &config)
                    .await;
                if let Ok(store) = GroupStore::new(home)
                    && let Err(error) = cccc_core::im_state::update(&store, &group_id, |value| {
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
                    })
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
            self.restore_tasks
                .lock()
                .expect("IM restore task registry poisoned")
                .push(task);
        }
    }

    pub(crate) async fn start_weixin_login(
        &self,
        home: &HomeLayout,
        group_id: &str,
    ) -> Result<Value, String> {
        self.weixin_logins.start(home, group_id).await
    }

    pub(crate) async fn weixin_login_status(
        &self,
        home: &HomeLayout,
        group_id: &str,
    ) -> Result<Value, String> {
        self.weixin_logins.status(home, group_id).await
    }

    pub(crate) async fn verify_weixin_login(
        &self,
        home: &HomeLayout,
        group_id: &str,
        verify_code: &str,
    ) -> Result<Value, String> {
        self.weixin_logins.verify(home, group_id, verify_code).await
    }

    pub(crate) async fn logout_weixin(
        &self,
        home: &HomeLayout,
        group_id: &str,
    ) -> Result<Value, String> {
        self.stop(group_id).await;
        let store = GroupStore::new(home.clone()).map_err(|error| error.to_string())?;
        cccc_core::im_state::update(&store, group_id, |value| {
            if !value.is_object() {
                *value = json!({});
            }
            let state = value.as_object_mut().expect("IM state initialized");
            state.insert("enabled".into(), Value::Bool(false));
            state.insert("running".into(), Value::Bool(false));
            state.insert("adapter_available".into(), Value::Bool(false));
            state.insert("pid".into(), Value::Null);
            state.insert("last_error".into(), Value::Null);
            state.insert("updated_at".into(), json!(cccc_contracts::utc_now()));
            Ok(())
        })
        .map_err(|error| error.to_string())?;
        if let Some(user_id) = weixin_login::stored_user_id(home, group_id)
            && let Err(error) =
                weixin_authorization::revoke_login_authorization(home, group_id, &user_id)
        {
            return Err(format!(
                "failed to revoke Weixin login authorization: {error}"
            ));
        }
        weixin_login::remove_credentials(home, group_id)
            .map_err(|error| format!("failed to remove Weixin credentials: {error}"))?;
        Ok(json!({
            "status":"logged_out","logged_in":false,"running":false,
            "pid":null,"updated_at":cccc_contracts::utc_now()
        }))
    }

    pub(crate) async fn start(
        &self,
        home: HomeLayout,
        client: DaemonClient,
        group_id: &str,
        config: &Map<String, Value>,
    ) -> Result<(), String> {
        let (generation, previous) = self.begin_start(group_id).await;
        if let Some(previous) = previous {
            previous.shutdown().await;
        }
        if !self.is_generation_current(group_id, generation) {
            return Err("IM worker start was superseded by a newer request".into());
        }
        let platform = string(config, "platform");
        if platform == "telegram" {
            let (tasks, stopper) =
                telegram::start(home, client, group_id, config, self.ledger_events.clone()).await?;
            return self
                .install(group_id, generation, worker(tasks, stopper))
                .await;
        }
        if platform == "discord" {
            let (tasks, stopper) = discord::start(
                home,
                client,
                group_id,
                config,
                self.ledger_events.clone(),
                Arc::clone(&self.discord_deduper),
            )
            .await?;
            return self
                .install(group_id, generation, worker(tasks, stopper))
                .await;
        }
        if platform == "slack" {
            let tasks =
                slack::start(home, client, group_id, config, self.ledger_events.clone()).await?;
            return self
                .install(group_id, generation, worker(tasks, no_op_stopper()))
                .await;
        }
        if platform == "feishu" {
            let tasks =
                feishu::start(home, client, group_id, config, self.ledger_events.clone()).await?;
            return self
                .install(group_id, generation, worker(tasks, no_op_stopper()))
                .await;
        }
        if platform == "wecom" {
            let generations = Arc::clone(&self.generations);
            let status_group = group_id.to_owned();
            let on_terminal_error = move |home: &HomeLayout, group_id: &str, error: &str| {
                let current = generations
                    .lock()
                    .expect("IM generation registry poisoned")
                    .get(&status_group)
                    .copied();
                if current == Some(generation) {
                    wecom::persist_terminal_error(home, group_id, error);
                }
            };
            let (tasks, sdk) = wecom::start(
                home,
                client,
                group_id,
                config,
                self.ledger_events.clone(),
                on_terminal_error,
            )
            .await?;
            let stopper: Stopper = Arc::new(move || sdk.shutdown());
            return self
                .install(group_id, generation, worker(tasks, stopper))
                .await;
        }
        if platform == "weixin" {
            let (tasks, sdk) =
                weixin::start(home, client, group_id, self.ledger_events.clone()).await?;
            let stopper: Stopper = Arc::new(move || sdk.shutdown());
            return self
                .install(group_id, generation, worker(tasks, stopper))
                .await;
        }
        if platform == "dingtalk" {
            let tasks =
                dingtalk::start(home, client, group_id, config, self.ledger_events.clone()).await?;
            return self
                .install(group_id, generation, worker(tasks, no_op_stopper()))
                .await;
        }
        Err(format!(
            "Rust network adapter is not migrated for platform {platform}"
        ))
    }

    async fn install(
        &self,
        group_id: &str,
        generation: u64,
        worker: WorkerHandles,
    ) -> Result<(), String> {
        let lifecycle_lock = self.lifecycle_lock(group_id);
        let lifecycle_guard = lifecycle_lock.lock().await;
        if !self.is_generation_current(group_id, generation) {
            drop(lifecycle_guard);
            worker.shutdown().await;
            return Err("IM worker start was superseded by a newer request".into());
        }
        let replaced = self
            .workers
            .lock()
            .expect("IM worker registry poisoned")
            .insert(group_id.to_owned(), worker);
        drop(lifecycle_guard);
        if let Some(replaced) = replaced {
            replaced.shutdown().await;
        }
        Ok(())
    }

    pub(crate) async fn stop(&self, group_id: &str) -> bool {
        let lifecycle_lock = self.lifecycle_lock(group_id);
        let (was_starting, worker) = {
            let _lifecycle_guard = lifecycle_lock.lock().await;
            let was_starting = self
                .generations
                .lock()
                .expect("IM generation registry poisoned")
                .remove(group_id)
                .is_some();
            let worker = self
                .workers
                .lock()
                .expect("IM worker registry poisoned")
                .remove(group_id);
            (was_starting, worker)
        };
        let was_running = worker.is_some();
        if let Some(worker) = worker {
            worker.shutdown().await;
        }
        let had_weixin_login = self.weixin_logins.clear(group_id);
        was_starting || was_running || had_weixin_login
    }

    async fn begin_start(&self, group_id: &str) -> (u64, Option<WorkerHandles>) {
        let lifecycle_lock = self.lifecycle_lock(group_id);
        let _lifecycle_guard = lifecycle_lock.lock().await;
        let generation = self
            .next_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        self.generations
            .lock()
            .expect("IM generation registry poisoned")
            .insert(group_id.to_owned(), generation);
        let previous = self
            .workers
            .lock()
            .expect("IM worker registry poisoned")
            .remove(group_id);
        (generation, previous)
    }

    fn is_generation_current(&self, group_id: &str, generation: u64) -> bool {
        self.generations
            .lock()
            .expect("IM generation registry poisoned")
            .get(group_id)
            .copied()
            == Some(generation)
    }

    fn lifecycle_lock(&self, group_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.lifecycle_locks
            .lock()
            .expect("IM lifecycle lock registry poisoned")
            .entry(group_id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub(crate) async fn shutdown(&self) {
        let restore_tasks = {
            let mut tasks = self
                .restore_tasks
                .lock()
                .expect("IM restore task registry poisoned");
            std::mem::take(&mut *tasks)
        };
        for task in &restore_tasks {
            task.abort();
        }
        for task in restore_tasks {
            let _ = task.await;
        }
        let workers = {
            let mut workers = self.workers.lock().expect("IM worker registry poisoned");
            workers
                .drain()
                .map(|(_, worker)| worker)
                .collect::<Vec<_>>()
        };
        futures_util::future::join_all(workers.into_iter().map(WorkerHandles::shutdown)).await;
        self.restoring
            .lock()
            .expect("IM restore registry poisoned")
            .clear();
        self.generations
            .lock()
            .expect("IM generation registry poisoned")
            .clear();
        self.weixin_logins.clear_all();
    }

    pub(crate) async fn stop_missing(&self, active_groups: &HashSet<String>) -> usize {
        let mut stale = self
            .workers
            .lock()
            .expect("IM worker registry poisoned")
            .keys()
            .filter(|group_id| !active_groups.contains(*group_id))
            .cloned()
            .collect::<HashSet<_>>();
        stale.extend(
            self.weixin_logins
                .group_ids()
                .into_iter()
                .filter(|group_id| !active_groups.contains(group_id)),
        );
        let mut stopped = 0;
        for group_id in stale {
            stopped += usize::from(self.stop(&group_id).await);
        }
        stopped
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
            .is_some_and(WorkerHandles::is_finished);
        if finished {
            workers.remove(group_id);
            self.generations
                .lock()
                .expect("IM generation registry poisoned")
                .remove(group_id);
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
            let state = cccc_core::im_state::load(&store, &meta.group_id).ok()?;
            if !state["enabled"].as_bool().unwrap_or(false) {
                return None;
            }
            Some((meta.group_id, state.get("config")?.as_object()?.clone()))
        })
        .collect()
}

fn restore_config(home: &HomeLayout, group_id: &str) -> Option<Map<String, Value>> {
    let store = GroupStore::new(home.clone()).ok()?;
    let state = cccc_core::im_state::load(&store, group_id).ok()?;
    if !state["enabled"].as_bool().unwrap_or(false) {
        return None;
    }
    state.get("config")?.as_object().cloned()
}

fn worker(tasks: Vec<JoinHandle<()>>, stopper: Stopper) -> WorkerHandles {
    WorkerHandles::new(tasks, stopper)
}

pub(super) fn spawn_outbound<S, F, Fut>(
    home: HomeLayout,
    group_id: String,
    platform: &'static str,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
    sender: S,
    send: F,
) -> JoinHandle<()>
where
    S: Send + Sync + 'static,
    F: Fn(Arc<S>, Vec<AuthorizedChat>, Event) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    spawn_outbound_matching(
        home,
        group_id,
        platform,
        ledger_events,
        sender,
        is_outbound,
        send,
    )
}

pub(super) fn spawn_outbound_matching<S, P, F, Fut>(
    home: HomeLayout,
    group_id: String,
    platform: &'static str,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
    sender: S,
    matches: P,
    send: F,
) -> JoinHandle<()>
where
    S: Send + Sync + 'static,
    P: Fn(&Event) -> bool + Send + Sync + 'static,
    F: Fn(Arc<S>, Vec<AuthorizedChat>, Event) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let sender = Arc::new(sender);
    tokio::spawn(async move {
        let Ok((mut receiver, cursor)) = ledger_events.subscribe_group_with_cursor(&group_id)
        else {
            return;
        };
        let mut delivery = OutboundDeliveryState {
            seen: HashSet::new(),
            cursor,
        };
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    deliver_outbound(
                        OutboundScope {
                            home: &home,
                            group_id: &group_id,
                            platform,
                        },
                        &sender,
                        &matches,
                        &send,
                        &mut delivery,
                        event,
                    )
                    .await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let Some(mut replay_cursor) = delivery.cursor.clone() else {
                        continue;
                    };
                    loop {
                        let page = ledger_events
                            .replay_after(&group_id, &replay_cursor, 2048)
                            .unwrap_or_default();
                        let page_len = page.len();
                        for event in page {
                            replay_cursor.clone_from(&event.id);
                            deliver_outbound(
                                OutboundScope {
                                    home: &home,
                                    group_id: &group_id,
                                    platform,
                                },
                                &sender,
                                &matches,
                                &send,
                                &mut delivery,
                                event,
                            )
                            .await;
                        }
                        if page_len < 2048 {
                            break;
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

struct OutboundDeliveryState {
    seen: HashSet<String>,
    cursor: Option<String>,
}

#[derive(Clone, Copy)]
struct OutboundScope<'a> {
    home: &'a HomeLayout,
    group_id: &'a str,
    platform: &'a str,
}

async fn deliver_outbound<S, P, F, Fut>(
    scope: OutboundScope<'_>,
    sender: &Arc<S>,
    matches: &P,
    send: &F,
    state: &mut OutboundDeliveryState,
    event: Event,
) where
    S: Send + Sync + 'static,
    P: Fn(&Event) -> bool + Send + Sync + 'static,
    F: Fn(Arc<S>, Vec<AuthorizedChat>, Event) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    if !state.seen.insert(event.id.clone()) {
        return;
    }
    state.cursor.replace(event.id.clone());
    if !matches(&event) {
        return;
    }
    if !event_visible_to_im(&event) {
        return;
    }
    if state.seen.len() > 8192 {
        state.seen.clear();
        state.seen.insert(event.id.clone());
    }
    let home = scope.home.clone();
    let group_id = scope.group_id.to_owned();
    let platform = scope.platform.to_owned();
    let chats = tokio::task::spawn_blocking(move || authorized_chats(&home, &group_id, &platform))
        .await
        .unwrap_or_default();
    let targets = delivery_targets(chats, &event);
    send(Arc::clone(sender), targets, event).await;
}

fn delivery_targets(chats: Vec<AuthorizedChat>, event: &Event) -> Vec<AuthorizedChat> {
    chats
        .into_iter()
        .filter(|chat| event_is_user_facing(event) || chat.verbose)
        .collect()
}

pub(super) fn is_outbound(event: &Event) -> bool {
    matches!(event.kind.as_str(), "chat.message" | "system.notify")
        && event.by != "user"
        && !event.by.starts_with("im:")
        && event.data.get("transport").and_then(Value::as_str) != Some("im")
}

pub(super) fn is_outbound_or_stream(event: &Event) -> bool {
    matches!(
        event.kind.as_str(),
        "chat.message" | "chat.stream" | "system.notify"
    ) && event.by != "user"
        && !event.by.starts_with("im:")
        && event.data.get("transport").and_then(Value::as_str) != Some("im")
}

fn completes_processing(event: &Event) -> bool {
    event.kind == "chat.message" && event_is_user_facing(event)
}

pub(super) fn processing_reply_to(event: &Event) -> Option<&str> {
    event
        .data
        .get("reply_to")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn event_is_user_facing(event: &Event) -> bool {
    event
        .data
        .get("to")
        .and_then(Value::as_array)
        .is_none_or(|targets| {
            targets.is_empty()
                || targets
                    .iter()
                    .any(|target| matches!(target.as_str(), Some("user" | "@user" | "@all")))
        })
}

fn event_visible_to_im(event: &Event) -> bool {
    if event.kind != "system.notify" {
        return true;
    }
    if event.data.get("im_visibility").and_then(Value::as_str) != Some("public") {
        return false;
    }
    if ["target_actor_id", "actor_id"].into_iter().any(|key| {
        event
            .data
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    }) {
        return false;
    }
    event_is_user_facing(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::ledger;

    #[test]
    fn only_final_chat_messages_complete_processing_feedback() {
        for kind in ["chat.stream", "system.notify"] {
            let event = Event::new(kind, "group");
            assert!(!completes_processing(&event), "kind={kind}");
        }
        assert!(completes_processing(&Event::new("chat.message", "group")));

        let mut peer_message = Event::new("chat.message", "group");
        peer_message.data.insert("to".into(), json!(["@foreman"]));
        assert!(!completes_processing(&peer_message));
    }

    #[test]
    fn processing_reply_to_ignores_missing_and_blank_ids() {
        let mut event = Event::new("chat.message", "group");
        assert_eq!(processing_reply_to(&event), None);
        event.data.insert("reply_to".into(), json!("  "));
        assert_eq!(processing_reply_to(&event), None);
        event.data.insert("reply_to".into(), json!(" event-1 "));
        assert_eq!(processing_reply_to(&event), Some("event-1"));
    }
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn finished_worker_aborts_sibling_tasks_and_runs_stoppers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let registry = ImWorkerRegistry::new(crate::ledger_event_hub::LedgerEventHub::new(home));
        let finished = tokio::spawn(async {});
        let sibling = tokio::spawn(std::future::pending());
        let sibling_abort = sibling.abort_handle();
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped_on_drop = Arc::clone(&stopped);
        let stopper: Stopper = Arc::new(move || {
            stopped_on_drop.store(true, Ordering::SeqCst);
        });
        registry
            .workers
            .lock()
            .expect("registry")
            .insert("g_test".into(), worker(vec![finished, sibling], stopper));

        tokio::task::yield_now().await;
        assert!(!registry.is_running("g_test"));
        tokio::task::yield_now().await;
        assert!(sibling_abort.is_finished());
        assert!(stopped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn stops_workers_for_deleted_groups() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let registry = ImWorkerRegistry::new(crate::ledger_event_hub::LedgerEventHub::new(home));
        registry.workers.lock().expect("registry").insert(
            "g_deleted".into(),
            worker(vec![tokio::spawn(std::future::pending())], no_op_stopper()),
        );
        assert_eq!(registry.stop_missing(&HashSet::new()).await, 1);
        assert!(!registry.is_running("g_deleted"));
    }

    #[tokio::test]
    async fn stop_retires_group_owned_weixin_login_attempt() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let registry = ImWorkerRegistry::new(crate::ledger_event_hub::LedgerEventHub::new(home));
        registry
            .weixin_logins
            .insert_test_attempt("g_deleted_login");
        assert!(registry.weixin_logins.contains("g_deleted_login"));

        registry.stop("g_deleted_login").await;

        assert!(!registry.weixin_logins.contains("g_deleted_login"));
    }

    #[tokio::test]
    async fn reaper_retires_login_attempt_without_network_worker() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let registry = ImWorkerRegistry::new(crate::ledger_event_hub::LedgerEventHub::new(home));
        registry.weixin_logins.insert_test_attempt("g_missing");

        assert_eq!(registry.stop_missing(&HashSet::new()).await, 1);
        assert!(!registry.weixin_logins.contains("g_missing"));
    }

    #[tokio::test]
    async fn stop_invalidates_a_pending_start_before_late_install() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let registry = ImWorkerRegistry::new(crate::ledger_event_hub::LedgerEventHub::new(home));
        let (generation, previous) = registry.begin_start("g_pending").await;
        assert!(previous.is_none());

        assert!(registry.stop("g_pending").await);
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped_by_worker = Arc::clone(&stopped);
        let stopper: Stopper = Arc::new(move || {
            stopped_by_worker.store(true, Ordering::SeqCst);
        });
        let result = registry
            .install("g_pending", generation, worker(Vec::new(), stopper))
            .await;

        assert_eq!(
            result.expect_err("late worker must be rejected"),
            "IM worker start was superseded by a newer request"
        );
        assert!(stopped.load(Ordering::SeqCst));
        assert!(!registry.is_running("g_pending"));
    }

    #[test]
    fn restore_only_selects_enabled_configured_groups() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let enabled = store.create("enabled", "").expect("enabled");
        let disabled = store.create("disabled", "").expect("disabled");
        for (group_id, active) in [(&enabled.group_id, true), (&disabled.group_id, false)] {
            cccc_core::im_state::update(&store, group_id, |state| {
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

    #[tokio::test]
    async fn restore_snapshot_does_not_reverse_a_newer_disable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("restore race", "").expect("group");
        cccc_core::im_state::update(&store, &group.group_id, |state| {
            *state = json!({
                "enabled":true,
                "config":{
                    "platform":"telegram",
                    "bot_token_env":"CCCC_IM_RESTORE_TEST_TOKEN_THAT_DOES_NOT_EXIST"
                }
            });
            Ok(())
        })
        .expect("enabled state");
        let registry = Arc::new(ImWorkerRegistry::new(
            crate::ledger_event_hub::LedgerEventHub::new(home.clone()),
        ));

        registry.restore_enabled(home.clone(), DaemonClient::new(home.clone()));
        cccc_core::im_state::update(&store, &group.group_id, |state| {
            state["enabled"] = Value::Bool(false);
            Ok(())
        })
        .expect("disable after restore snapshot");
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while registry
                .restoring
                .lock()
                .expect("restoring")
                .contains(&group.group_id)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("restore task settled");

        let state = cccc_core::im_state::load(&store, &group.group_id).expect("state");
        assert!(state.get("last_error").is_none_or(Value::is_null));
        assert!(!registry.is_running(&group.group_id));
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

        let mut notification = Event::new("system.notify", "g_test");
        notification.by = "system".into();
        assert!(is_outbound(&notification));
    }

    #[test]
    fn actor_targeted_system_notifications_never_escape_to_im() {
        let mut actor_notice = Event::new("system.notify", "g_test");
        actor_notice.by = "system".into();
        actor_notice.data = json!({
            "actor_id":"peer-1",
            "to":["peer-1"],
            "im_visibility":"public",
            "text":"You have an unread collaboration message."
        })
        .as_object()
        .cloned()
        .expect("data");
        assert!(!event_visible_to_im(&actor_notice));

        let mut direct_notice = Event::new("system.notify", "g_test");
        direct_notice
            .data
            .insert("im_visibility".into(), json!("public"));
        direct_notice
            .data
            .insert("target_actor_id".into(), json!("foreman"));
        assert!(!event_visible_to_im(&direct_notice));

        let implicit = Event::new("system.notify", "g_test");
        assert!(!event_visible_to_im(&implicit));

        let mut broadcast = Event::new("system.notify", "g_test");
        broadcast
            .data
            .insert("im_visibility".into(), json!("public"));
        broadcast.data.insert("to".into(), json!(["@all"]));
        assert!(event_visible_to_im(&broadcast));
    }

    #[test]
    fn non_verbose_targets_only_receive_user_facing_events() {
        let targets = || {
            vec![
                AuthorizedChat {
                    chat_id: "quiet".into(),
                    thread_id: String::new(),
                    verbose: false,
                },
                AuthorizedChat {
                    chat_id: "verbose".into(),
                    thread_id: String::new(),
                    verbose: true,
                },
            ]
        };
        let mut peer = Event::new("chat.message", "g_test");
        peer.data.insert("to".into(), json!(["peer"]));
        assert_eq!(
            delivery_targets(targets(), &peer)
                .into_iter()
                .map(|target| target.chat_id)
                .collect::<Vec<_>>(),
            vec!["verbose"]
        );

        peer.data.insert("to".into(), json!(["@user"]));
        let mut user_targets = delivery_targets(targets(), &peer)
            .into_iter()
            .map(|target| target.chat_id)
            .collect::<Vec<_>>();
        user_targets.sort();
        assert_eq!(user_targets, vec!["quiet", "verbose"]);

        let notification = Event::new("system.notify", "g_test");
        let mut notification_targets = delivery_targets(targets(), &notification)
            .into_iter()
            .map(|target| target.chat_id)
            .collect::<Vec<_>>();
        notification_targets.sort();
        assert_eq!(notification_targets, vec!["quiet", "verbose"]);
    }

    #[tokio::test]
    async fn outbound_subscription_replays_broadcast_lag_without_polling_gaps() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("IM events", "").expect("group");
        let path = store.ledger_path(&group.group_id).expect("ledger");
        ledger::append(&path, &Event::new("group.create", &group.group_id)).expect("cursor");
        let hub = crate::ledger_event_hub::LedgerEventHub::new(home.clone());
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
        let task = spawn_outbound(
            home,
            group.group_id.clone(),
            "telegram",
            hub,
            (),
            move |_, _, event| {
                let sent = sent.clone();
                async move {
                    sent.send(event.id).ok();
                }
            },
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        for index in 0..1_100 {
            let mut event = Event::new("chat.message", &group.group_id);
            event.by = "foreman".into();
            event.data.insert("text".into(), json!(index.to_string()));
            ledger::append(&path, &event).expect("append");
        }
        let mut ids = HashSet::new();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while ids.len() < 1_100 {
                ids.insert(received.recv().await.expect("outbound event"));
            }
        })
        .await
        .expect("event-driven outbound timeout");
        task.abort();
        assert_eq!(ids.len(), 1_100);
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

    #[tokio::test]
    async fn subscribe_creates_pending_request_without_authorizing_chat() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("test", "").expect("group");
        assert!(matches!(
            inbound_decision(&home, &group.group_id, "telegram", "chat-1", "/subscribe").await,
            InboundDecision::Reply(_)
        ));
        let InboundDecision::Reply(body) =
            inbound_decision(&home, &group.group_id, "telegram", "chat-2", "hello").await
        else {
            panic!("unauthorized plain text must receive binding guidance");
        };
        assert!(body.contains("not authorized"));
        assert!(body.contains("CCCC group \"test\""));
        assert!(!body.contains(&group.group_id));
        assert!(body.contains("direct messages work as plain text"));
        let state = cccc_core::im_state::load(&store, &group.group_id).expect("state");
        let pending = state["pending"].as_array().expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0]["chat_id"], "chat-1");
        assert_eq!(pending[0]["platform"], "telegram");
    }
}
