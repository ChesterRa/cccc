//! One resident global secretary, processing fixed-target tasks serially.
//! No viewer/poll owns admission, cancellation, or provider lifetime.
use cccc_contracts::voice_secretary::{SecretaryTaskKind, SecretaryTaskPhase};
use cccc_contracts::{ActorRuntime, DaemonRequest};
use cccc_core::codex_voice_settings::ResolvedAgentRuntime;
use cccc_core::voice_secretary::{SecretaryReceipt, SecretaryTask, SecretaryTaskStore, digest};
use cccc_core::{HomeLayout, profiles::ProfileStore, settings};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::{Notify, watch};

use super::codex_voice_analyst::{
    AnalystSession, LaunchConfig, PendingManagedStartup, managed_runtime,
};
use super::operation::{Operation, Policy};
use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};
use crate::dispatch_concurrency::DispatchLocks;

mod configuration;
mod recovery;
mod resident;
mod sources;
mod task_actions;
mod task_controls;
mod task_reads;
pub(crate) mod terminal;
use task_controls::{candidate, retry};
#[cfg(test)]
mod tests;

const MAX_QUEUED_TASKS: usize = 64;
const MAX_BATCH_BYTES: usize = 24_000;

pub(super) fn group_projection(home: &HomeLayout, group_id: &str) -> io::Result<Value> {
    let view = owner_group_view(home, group_id)?;
    let state = cccc_core::assistant_state::load_workflow(home, group_id)?;
    let readiness = configuration::readiness(home);
    let status = view.status.map_or_else(
        || {
            json!(if readiness.ready {
                "ready"
            } else if !readiness.configured
                && readiness.readiness_code
                    == Some(cccc_contracts::voice_secretary::SecretaryReadinessCode::NotConfigured)
            {
                "unconfigured"
            } else {
                "unavailable"
            })
        },
        |phase| json!(phase),
    );
    let sources = source_counts(home, group_id)?;
    let deferred = sources.deferred;
    Ok(configuration::with_readiness(
        json!({"owner":"global","busy":view.busy,
        "pending":view.pending || deferred > 0,"status":status,
        "deferred_sources":deferred,"held_sources":sources.held,"invalid_sources":sources.invalid,
        "unprocessed_document_sources":view.unprocessed_document_sources,
        "tasks":view.tasks.iter().map(|task| observed_projection(home, task, &state)).collect::<Vec<_>>()}),
        &readiness,
    ))
}

struct Execution {
    task_id: String,
    cancel: watch::Sender<bool>,
    generation: OnceLock<String>,
    attempted: AtomicBool,
    session: Mutex<Option<Arc<AnalystSession>>>,
    startup_cleanup: Mutex<Option<PendingManagedStartup>>,
    quiescent: AtomicBool,
    progress: Mutex<terminal::Progress>,
    activity: Mutex<String>,
    submitted: Notify,
}

struct Manager {
    home: HomeLayout,
    store: SecretaryTaskStore,
    acceptance: Mutex<()>,
    grants: Mutex<HashMap<String, Arc<Execution>>>,
    wake: Notify,
    resident: Mutex<Option<Arc<resident::Resident>>>,
    reset_requested: AtomicBool,
    closing: AtomicBool,
    pending_sources: Mutex<BTreeMap<(String, u64, String), Value>>,
    source_errors: Mutex<std::collections::BTreeSet<(String, u64, String)>>,
    workers: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    locks: Option<DispatchLocks>,
}

enum Owner {
    Running(Arc<Manager>),
    Failed(String),
}

fn registry() -> &'static Mutex<HashMap<PathBuf, Owner>> {
    static MANAGERS: OnceLock<Mutex<HashMap<PathBuf, Owner>>> = OnceLock::new();
    MANAGERS.get_or_init(Mutex::default)
}

fn lookup(home: &HomeLayout) -> Option<Arc<Manager>> {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(home.root())
        .and_then(|owner| match owner {
            Owner::Running(manager) => Some(Arc::clone(manager)),
            Owner::Failed(_) => None,
        })
}

fn owner_readiness_error(home: &HomeLayout) -> Option<String> {
    match registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(home.root())
    {
        Some(Owner::Running(manager)) if manager.closing.load(Ordering::Acquire) => {
            Some("Secretary execution owner is stopping; saved inputs are retained".into())
        }
        Some(Owner::Running(_)) => None,
        Some(Owner::Failed(diagnostic)) => Some(diagnostic.clone()),
        None => Some("Secretary execution owner is unavailable; saved inputs are retained".into()),
    }
}

// Startup can read malformed configuration or task files. Parser messages may
// quote private values; expose the stage, error category and location instead.
fn safe_error_detail(error: &io::Error) -> String {
    if let Some(task) = error.get_ref().and_then(|source| {
        source.downcast_ref::<cccc_core::voice_secretary::SecretaryTaskRecordError>()
    }) {
        task.to_string()
    } else if let Some(yaml) = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<serde_yaml::Error>())
    {
        yaml.location().map_or_else(
            || "invalid YAML data".into(),
            |location| {
                format!(
                    "invalid YAML data at line {} column {}",
                    location.line(),
                    location.column()
                )
            },
        )
    } else if let Some(json) = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<serde_json::Error>())
    {
        format!(
            "invalid JSON data at line {} column {}",
            json.line(),
            json.column()
        )
    } else if let Some(code) = error.raw_os_error() {
        format!("{} (OS error {code})", error.kind())
    } else {
        error.kind().to_string()
    }
}

fn startup_error(stage: impl std::fmt::Display, error: io::Error) -> io::Error {
    let detail = safe_error_detail(&error);
    let diagnostic =
        format!("Secretary startup failed while {stage}: {detail}; saved inputs are retained")
            .chars()
            .take(1600)
            .collect::<String>();
    io::Error::new(error.kind(), diagnostic)
}

fn task_store(home: &HomeLayout) -> SecretaryTaskStore {
    lookup(home).map_or_else(
        || SecretaryTaskStore::new(home.clone()),
        |m| m.store.clone(),
    )
}

fn owner_group_view(
    home: &HomeLayout,
    group_id: &str,
) -> io::Result<cccc_core::voice_secretary::SecretaryGroupView> {
    // Failed startup cannot publish a partial authority index. Report its saved
    // readiness error instead of rereading the record that blocked that startup.
    lookup(home).map_or_else(
        || Ok(cccc_core::voice_secretary::SecretaryGroupView::default()),
        |manager| manager.store.group_view(group_id, 12),
    )
}

#[cfg(test)]
fn deferred_count(home: &HomeLayout, group_id: &str) -> usize {
    lookup(home).map_or(0, |m| {
        m.pending_sources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .filter(|(group, _, _)| group == group_id)
            .count()
    })
}

fn source_counts(home: &HomeLayout, group_id: &str) -> io::Result<sources::SourceCounts> {
    let held = configuration::held_baselines(home)?;
    Ok(
        lookup(home).map_or_else(sources::SourceCounts::default, |manager| {
            manager.source_counts(Some(group_id), &held)
        }),
    )
}

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "voice_secretary_settings_get" => Operation::new(Policy::Read, configuration::get),
        "voice_secretary_settings_update" => {
            Operation::new(Policy::GlobalWrite, configuration::update)
        }
        "voice_secretary_runtime" => Operation::new(Policy::Read, resident::status),
        "voice_secretary_runtime_reset" => Operation::new(Policy::ResourceOwned, resident::reset),
        "voice_secretary_tasks" => Operation::new(Policy::Read, tasks),
        "voice_secretary_task_cancel" => Operation::new(Policy::ResourceOwned, cancel),
        "voice_secretary_task_retry" => Operation::new(Policy::Write, retry),
        "voice_secretary_task_candidate" => Operation::new(Policy::Read, candidate),
        "voice_secretary_task_handoff" => Operation::new(Policy::Write, task_controls::handoff),
        "voice_secretary_task" => Operation::new(Policy::ResourceOwned, task_actions::call),
        "voice_secretary_terminal_resize" => {
            Operation::new(Policy::ResourceOwned, terminal::resize)
        }
        _ => return None,
    })
}

#[derive(serde::Deserialize)]
struct StartupGroupActors {
    #[serde(default)]
    actors: Vec<StartupActorIdentity>,
}

#[derive(serde::Deserialize)]
struct StartupActorIdentity {
    id: String,
    #[serde(default)]
    internal_kind: Option<String>,
}

fn retire_obsolete_actors(home: &HomeLayout) -> io::Result<()> {
    let groups = cccc_core::GroupStore::new(home.clone())
        .map_err(|error| startup_error("opening the Group store", error))?;
    for meta in groups
        .list()
        .map_err(|error| startup_error("reading the Group registry", error))?
    {
        let path = groups
            .group_dir(&meta.group_id)
            .map_err(|error| startup_error("resolving a Group directory", error))?
            .join("group.yaml");
        // The global service needs only Actor identities for this scan. An
        // unrelated Group's unsupported Runtime must not become a dependency.
        let identities = match cccc_core::fs::read_yaml::<StartupGroupActors>(&path) {
            Ok(identities) => identities,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tracing::warn!(group_id = %meta.group_id, "Secretary startup skipped a missing Group; registry entry is unchanged");
                continue;
            }
            Err(error) => {
                return Err(startup_error(
                    format!("reading Actor identities in Group {}", meta.group_id),
                    error,
                ));
            }
        };
        if !identities.actors.iter().any(|actor| {
            actor.id == "voice-secretary"
                || actor.internal_kind.as_deref() == Some("voice_secretary")
        }) {
            continue;
        }
        let group = groups.load(&meta.group_id).map_err(|error| {
            startup_error(
                format!("loading Group {} for obsolete Actor cleanup", meta.group_id),
                error,
            )
        })?;
        for actor in group.actors.iter().filter(|actor| {
            actor.id == "voice-secretary"
                || actor.internal_kind.as_deref() == Some("voice_secretary")
        }) {
            super::actor_delivery::shutdown_actor(&group.group_id, &actor.id);
            super::actor_runtime::apply(home, &group, &actor.id, "actor.stop").map_err(
                |error| {
                    startup_error(
                        format!(
                            "stopping obsolete Actor {} in Group {} ({})",
                            actor.id, group.group_id, error.code
                        ),
                        io::Error::other(error.message),
                    )
                },
            )?;
            let request = DaemonRequest {
                v: 1,
                op: "actor_remove".into(),
                args: json!({"group_id":group.group_id,"actor_id":actor.id,"by":"user"})
                    .as_object()
                    .expect("retirement arguments")
                    .clone(),
            };
            // The ordinary removal path owns private env, bindings and session
            // records. Stop first so failed removal never restarts an old owner.
            super::actors::resolve_operation(&request)
                .expect("Actor removal")
                .execute(home, &request)
                .map_err(|error| {
                    startup_error(
                        format!(
                            "removing obsolete Actor {} in Group {} ({})",
                            actor.id, group.group_id, error.code
                        ),
                        io::Error::other(error.message),
                    )
                })?;
        }
    }
    Ok(())
}

/// Register at daemon startup, after process ownership reconciliation. Merely
/// reading configuration/status must never create this manager or model work.
pub(crate) fn start(home: &HomeLayout, locks: Option<DispatchLocks>) -> io::Result<()> {
    let mut managers = registry().lock().unwrap_or_else(|e| e.into_inner());
    if matches!(managers.get(home.root()), Some(Owner::Running(_))) {
        return Ok(());
    }
    let manager = match initialize(home, locks) {
        Ok(manager) => manager,
        Err(error) => {
            managers.insert(home.root().to_owned(), Owner::Failed(error.to_string()));
            return Err(error);
        }
    };
    // Publish only after reconciliation and cleanup; retain the startup lock
    // until both worker handles are registered so shutdown cannot miss them.
    managers.insert(home.root().to_owned(), Owner::Running(Arc::clone(&manager)));
    let mut workers = manager.workers.lock().unwrap_or_else(|e| e.into_inner());
    {
        let owner = Arc::clone(&manager);
        workers.push(managed_runtime().spawn(async move {
            owner.run().await;
        }));
    }
    Ok(())
}

fn initialize(home: &HomeLayout, locks: Option<DispatchLocks>) -> io::Result<Arc<Manager>> {
    retire_obsolete_actors(home)?;
    let store = SecretaryTaskStore::indexed(home.clone())
        .map_err(|error| startup_error("loading Secretary task records", error))?;
    resident::revoke_session_grant(&store)?;
    for task in store
        .list()
        .map_err(|error| startup_error("reading Secretary tasks", error))?
    {
        let previous = store.directory(&task.task_id)?.join("workspace");
        let previous_pending = super::codex_voice_analyst::secretary_cleanup_pending(&previous);
        if !previous_pending {
            store.relocate_workspace(&task.task_id)?;
        }
        // Grants are in-memory authority and expire on daemon restart. Remove
        // old bootstrap files even when native process cleanup is still pending.
        store.revoke_grant_file(&task.task_id).map_err(|error| {
            startup_error(
                format!("revoking the previous grant for task {}", task.task_id),
                error,
            )
        })?;
        if task.phase.executing() || (!task.prepared_version.is_empty() && task.receipt.is_none()) {
            store
                .recover(&task.task_id, |t| {
                    super::assistants::secretary_validate_document(home, t)
                })
                .map_err(|error| {
                    startup_error(format!("recovering task {}", task.task_id), error)
                })?;
        }
        if !task.cleanup_confirmed
            && !previous_pending
            && !super::codex_voice_analyst::secretary_cleanup_pending(&store.runtime_workspace())
            && !cccc_runtime::owned_process_recovery_pending()
            && !super::codex_voice_analyst::secretary_cleanup_pending(
                &store
                    .workspace(&task.task_id)
                    .map_err(|error| startup_error("resolving a task workspace", error))?,
            )
        {
            store
                .update(&task.task_id, |t| {
                    t.cleanup_confirmed = true;
                    Ok(())
                })
                .map_err(|error| {
                    startup_error(
                        format!("recording cleanup for task {}", task.task_id),
                        error,
                    )
                })?;
        }
    }
    let manager = Arc::new(Manager {
        home: home.clone(),
        store,
        acceptance: Mutex::new(()),
        grants: Mutex::new(HashMap::new()),
        wake: Notify::new(),
        resident: Mutex::new(None),
        reset_requested: AtomicBool::new(false),
        closing: AtomicBool::new(false),
        pending_sources: Mutex::new(BTreeMap::new()),
        source_errors: Mutex::new(std::collections::BTreeSet::new()),
        workers: Mutex::new(Vec::new()),
        locks,
    });
    manager
        .load_sources()
        .map_err(|error| startup_error("loading saved Secretary inputs", error))?;
    manager
        .reconcile_sources()
        .map_err(|error| startup_error("reconciling saved Secretary inputs", error))?;
    let mut group_ids = manager
        .store
        .list()
        .map_err(|error| startup_error("listing Secretary task targets", error))?
        .into_iter()
        .map(|t| t.target.group_id)
        .collect::<std::collections::BTreeSet<_>>();
    // A crash after Group deletion can leave raw sources even without any jobs
    // (for example, when the Secretary was unconfigured). Retire those too.
    let root = home.root().join("voice-secretary");
    if root.is_dir() {
        for entry in std::fs::read_dir(root)
            .map_err(|error| startup_error("reading Secretary source directories", error))?
        {
            let entry = entry
                .map_err(|error| startup_error("reading a Secretary source directory", error))?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if entry
                .file_type()
                .map_err(|error| startup_error("reading Secretary source metadata", error))?
                .is_dir()
                && id.len() == 14
                && id.starts_with("g_")
                && id[2..].bytes().all(|b| b.is_ascii_hexdigit())
            {
                group_ids.insert(id);
            }
        }
    }
    for group in group_ids {
        purge_deleted_group_from_store(home, &group, &manager.store)
            .map_err(|error| startup_error(format!("cleaning deleted Group {group}"), error))?;
    }
    Ok(manager)
}

pub(crate) fn stop(home: &HomeLayout) -> io::Result<()> {
    let Some(manager) = lookup(home) else {
        let mut owners = registry().lock().unwrap_or_else(|e| e.into_inner());
        if matches!(owners.get(home.root()), Some(Owner::Failed(_))) {
            owners.remove(home.root());
        }
        return Ok(());
    };
    manager.closing.store(true, Ordering::Release);
    for execution in manager
        .grants
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
    {
        execution.cancel.send_replace(true);
    }
    manager.notify();
    let workers = std::mem::take(&mut *manager.workers.lock().unwrap_or_else(|e| e.into_inner()));
    block_on(async {
        for worker in workers {
            worker.await.map_err(io::Error::other)?;
        }
        let executions = manager
            .grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(key, execution)| (key.clone(), execution.clone()))
            .collect::<Vec<_>>();
        for (key, execution) in executions {
            manager.cleanup_execution(&execution, false).await?;
            manager.store.update(&execution.task_id, |task| {
                task.cleanup_confirmed = true;
                Ok(())
            })?;
            manager
                .grants
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&key);
        }
        manager.stop_resident().await?;
        manager.reconcile_native_cleanup().await?;
        Ok::<(), io::Error>(())
    })?;
    if !manager
        .grants
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty()
    {
        return Err(io::Error::other(
            "Voice Secretary provider cleanup is unresolved",
        ));
    }
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(home.root());
    Ok(())
}

/// Input persistence happens first. Retry of the same source finds the same
/// task, including failed/unconfirmed tasks, and never replays a provider prompt.
pub(super) fn enqueue(
    home: &HomeLayout,
    group_id: &str,
    input: &Value,
) -> io::Result<Option<String>> {
    let manager = lookup(home).ok_or_else(|| {
        io::Error::other(owner_readiness_error(home).unwrap_or_else(|| {
            "Secretary execution owner is unavailable; saved inputs are retained".into()
        }))
    })?;
    if manager.closing.load(Ordering::Acquire) {
        return Err(io::Error::other(
            "secretary is stopping; source is retained",
        ));
    }
    let _acceptance = manager.acceptance.lock().unwrap_or_else(|e| e.into_inner());
    manager.accept(group_id, input)
}

/// Group deletion revokes accepted work without joining a provider while the
/// delete operation holds the global mutation permit. Workers own cleanup.
pub(super) fn cancel_group(home: &HomeLayout, group_id: &str) -> io::Result<()> {
    let manager = lookup(home);
    let _acceptance = manager
        .as_ref()
        .map(|m| m.acceptance.lock().unwrap_or_else(|e| e.into_inner()));
    if let Some(manager) = manager.as_ref() {
        // A shared conversation may contain this Group. Discard it after the current turn.
        manager.reset_requested.store(true, Ordering::Release);
        for execution in manager
            .grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            if manager.store.load(&execution.task_id)?.target.group_id == group_id {
                manager.store.update(&execution.task_id, |t| {
                    t.cancellation_reason = "group_deleted".into();
                    Ok(())
                })?;
                execution.cancel.send_replace(true);
            }
        }
    }
    let store = task_store(home);
    for task in store
        .list()?
        .into_iter()
        .filter(|t| t.target.group_id == group_id && t.phase == SecretaryTaskPhase::Queued)
    {
        store.update(&task.task_id, |t| {
            t.phase = SecretaryTaskPhase::Cancelled;
            t.cancellation_reason = "group_deleted".into();
            Ok(())
        })?;
    }
    if let Some(manager) = manager.as_ref() {
        manager
            .pending_sources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(group, _, _), _| group != group_id);
        manager
            .source_errors
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(group, _, _)| group != group_id);
        manager.notify();
    }
    Ok(())
}

/// Called after Group deletion, and again after a revoked worker finishes cleanup.
pub(super) fn purge_deleted_group(home: &HomeLayout, group_id: &str) -> io::Result<()> {
    purge_deleted_group_from_store(home, group_id, &task_store(home))
}

fn purge_deleted_group_from_store(
    home: &HomeLayout,
    group_id: &str,
    store: &SecretaryTaskStore,
) -> io::Result<()> {
    let groups = cccc_core::GroupStore::new(home.clone())?;
    if groups.group_dir(group_id)?.exists() {
        return Ok(());
    }
    let tasks = store.group_tasks(group_id)?;
    if tasks
        .iter()
        .any(|t| t.phase.executing() || !t.cleanup_confirmed)
    {
        return Ok(());
    }
    for task in tasks {
        store.remove(&task.task_id)?;
    }
    let path = home.root().join("voice-secretary").join(group_id);
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

impl Manager {
    /// Agent View jobs are supervised by Claude, not by our process group guard.
    /// Recover only task-owned cwd records; keep capacity when control is unavailable.
    async fn reconcile_native_cleanup(&self) -> io::Result<()> {
        let tasks = self.store.unfinished()?;
        for task in tasks.into_iter().filter(|task| !task.cleanup_confirmed) {
            if self
                .grants
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .any(|execution| execution.task_id == task.task_id)
            {
                continue;
            }
            let workspace = self.store.directory(&task.task_id)?.join("workspace");
            if !super::codex_voice_analyst::secretary_cleanup_pending(&workspace) {
                continue;
            }
            super::codex_voice_analyst::cleanup_secretary_runtime(&self.home, &workspace).await?;
            self.store.relocate_workspace(&task.task_id)?;
            self.store.update(&task.task_id, |task| {
                task.cleanup_confirmed = true;
                Ok(())
            })?;
            self.project(&self.store.load(&task.task_id)?).await;
        }
        Ok(())
    }
}

fn input_bytes(inputs: &[Value]) -> usize {
    inputs
        .iter()
        .map(|v| v["text"].as_str().unwrap_or("").len())
        .sum()
}

fn resolve_runtime(home: &HomeLayout) -> io::Result<ResolvedAgentRuntime> {
    cccc_core::voice_secretary_settings::resolve(
        home,
        &settings::load(home)?.voice_secretary,
        &cccc_core::voice_secretary_settings::private_environment(home)?,
    )
}

impl Manager {
    fn notify(&self) {
        self.wake.notify_one();
    }

    async fn run(self: Arc<Self>) {
        if let Ok(tasks) = self.store.list() {
            for task in &tasks {
                self.project(task).await;
            }
        }
        loop {
            if self.closing.load(Ordering::Acquire) {
                break;
            }
            if let Err(error) = self.reconcile_sources() {
                tracing::error!(%error, "Secretary accepted-source reconciliation failed");
            }
            if let Err(error) = self.recover_documents().await {
                tracing::error!(%error, "Secretary document recovery remains pending");
            }
            let notified = self.wake.notified();
            // Failed cleanup still consumes capacity. A client disappearing is
            // not proof that its owned provider tree is gone.
            let resident_cleanup = self.maintain_resident().await;
            let native_cleanup = self.reconcile_native_cleanup().await;
            if let Err(error) = &native_cleanup {
                tracing::warn!(%error, "Secretary Agent View cleanup remains pending");
            }
            let blocked = native_cleanup.is_err()
                || resident_cleanup.is_err()
                || self.resident().is_some_and(|r| r.busy())
                || self
                    .grants
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .values()
                    .next()
                    .is_some()
                || cccc_runtime::owned_process_recovery_pending();
            let work = if blocked {
                None
            } else {
                match self.store.queued() {
                    Ok(tasks) => tasks
                        .into_iter()
                        .find(|t| t.phase == SecretaryTaskPhase::Queued),
                    Err(error) => {
                        tracing::error!(%error, "Secretary task records could not be read; admission paused");
                        None
                    }
                }
            };
            if let Some(task) = work {
                let config = {
                    // Settings and private environment are one saved configuration.
                    // Do not read halfway through a global settings transaction.
                    let _permit = if let Some(locks) = &self.locks {
                        Some(locks.global_read().await)
                    } else {
                        None
                    };
                    resolve_runtime(&self.home)
                };
                match config {
                    Ok(config) => {
                        self.execute(task, config).await;
                        continue;
                    }
                    Err(error) => {
                        let _ = self.store.update(&task.task_id, |t| {
                            t.diagnostic = configuration::configuration_diagnostic(
                                "resolving saved Runtime or Profile",
                                &error,
                            );
                            Ok(())
                        });
                    }
                }
            }
            tokio::select! { _ = notified => {}, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
        }
    }

    async fn execute(&self, task: SecretaryTask, config: ResolvedAgentRuntime) {
        let (cancel, cancellation) = watch::channel(false);
        let execution = Arc::new(Execution {
            task_id: task.task_id.clone(),
            cancel,
            generation: OnceLock::new(),
            attempted: AtomicBool::new(false),
            session: Mutex::new(None),
            startup_cleanup: Mutex::new(None),
            quiescent: AtomicBool::new(false),
            progress: Mutex::new(terminal::Progress::default()),
            activity: Mutex::new(String::new()),
            submitted: Notify::new(),
        });
        let token = self.resident().map(|r| r.token.clone()).unwrap_or_else(|| {
            format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            )
        });
        let token_hash = digest(token.as_bytes());
        let claimed = {
            let _acceptance = self.acceptance.lock().unwrap_or_else(|e| e.into_inner());
            if self.resident().is_some_and(|r| r.busy()) {
                return;
            }
            let claimed = self.store.update(&task.task_id, |task| {
                if task.phase != SecretaryTaskPhase::Queued
                    || self.reset_requested.load(Ordering::Acquire)
                {
                    return Ok(false);
                }
                task.phase = SecretaryTaskPhase::Starting;
                task.cleanup_confirmed = false;
                task.runtime_fingerprint = digest(&config.fingerprint());
                task.diagnostic.clear();
                Ok(true)
            });
            if matches!(claimed, Ok(true)) {
                self.grants
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(token_hash.clone(), Arc::clone(&execution));
            }
            claimed
        };
        if !matches!(claimed, Ok(true)) {
            return;
        }
        let outcome = self
            .execute_owned(&task, &config, &execution, cancellation, &token)
            .await;
        // Revoke before cleanup or another admission, regardless of business outcome.
        execution.cancel.send_replace(true);
        let startup_cleanup = outcome
            .as_ref()
            .err()
            .and_then(PendingManagedStartup::from_error);
        *execution
            .startup_cleanup
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = startup_cleanup;
        let cleanup = self
            .cleanup_execution(
                &execution,
                (outcome.is_ok() || !execution.attempted.load(Ordering::Acquire))
                    && execution.quiescent.load(Ordering::Acquire)
                    && !self.closing.load(Ordering::Acquire),
            )
            .await;
        let cleaned = cleanup.is_ok();
        let result = self.store.update(&task.task_id, |task| {
            task.cleanup_confirmed = cleaned;
            task.phase = task.receipt.as_ref().map_or_else(
                || match &outcome {
                    Ok(status) => *status,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                        if task.cancellation_reason.is_empty() {
                            task.cancellation_reason = if self.closing.load(Ordering::Acquire) {
                                "shutdown".into()
                            } else {
                                "user".into()
                            };
                        }
                        SecretaryTaskPhase::Cancelled
                    }
                    Err(_) if execution.attempted.load(Ordering::Acquire) => {
                        SecretaryTaskPhase::Unconfirmed
                    }
                    Err(_) => SecretaryTaskPhase::Failed,
                },
                |receipt| receipt.status,
            );
            if let Err(error) = &outcome {
                task.diagnostic = if super::codex_voice_analyst::untrusted_claude_workspace(error).is_some() {
                    "Claude Code has not trusted the Voice Secretary task directory. Trust $CCCC_HOME/voice-secretary/workspace once using the configured Claude installation and CLAUDE_CONFIG_DIR, then retry. No model prompt was sent.".into()
                } else { safe_diagnostic(error, &config, &token) };
            }
            if let Err(error) = &cleanup {
                task.diagnostic = format!(
                    "Provider cleanup is unresolved: {}",
                    safe_diagnostic(error, &config, &token)
                );
            }
            Ok(())
        });
        if result.is_err() {
            tracing::error!(task_id = %task.task_id, "could not persist secretary terminal outcome; execution is retained");
        }
        if cleaned && result.is_ok() {
            self.grants
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&token_hash);
        }
        if let Ok(saved) = self.store.load(&task.task_id) {
            self.project(&saved).await;
            if !self.closing.load(Ordering::Acquire) {
                let _ = self.recover_document(&saved).await;
            }
        }
        if let Err(error) = purge_deleted_group(&self.home, &task.target.group_id) {
            tracing::error!(%error, "Deleted Group secretary cleanup remains pending");
        }
        self.notify();
    }

    async fn project(&self, task: &SecretaryTask) {
        if !task.projected_at.is_empty() {
            if let Err(error) = self.retire_resolved_artifacts(&task.task_id) {
                tracing::error!(%error, "Secretary artifact retirement remains pending");
            }
            return;
        }
        if task.phase.executing() || task.phase == SecretaryTaskPhase::Queued {
            return;
        }
        let _permit = if let Some(locks) = self.locks.as_ref() {
            Some(locks.group_write(&task.target.group_id).await)
        } else {
            None
        };
        let result = super::assistants::secretary_project_result(&self.home, task);
        let _ = self.store.update(&task.task_id, |task| {
            match result {
                Ok(()) => {
                    task.projected_at = cccc_contracts::utc_now();
                    task.projection_error.clear();
                }
                Err(error) => task.projection_error = error.to_string(),
            }
            Ok(())
        });
        if let Err(error) = self.retire_resolved_artifacts(&task.task_id) {
            tracing::error!(%error, task_id=%task.task_id, "Secretary artifact retirement remains pending");
        }
    }

    fn retire_resolved_artifacts(&self, task_id: &str) -> io::Result<()> {
        self.store.retire_artifacts(task_id)?;
        let resolved = self.store.load(task_id)?;
        if resolved.phase != SecretaryTaskPhase::Done
            || !resolved.cleanup_confirmed
            || resolved.projected_at.is_empty()
        {
            return Ok(());
        }
        let mut previous = resolved.previous_task_id;
        let mut seen = std::collections::HashSet::new();
        while !previous.is_empty() {
            if !seen.insert(previous.clone()) {
                return Err(io::Error::other("Secretary continuation chain is cyclic"));
            }
            let old = self.store.load(&previous)?;
            self.store
                .retire_resolved_artifacts(&old.task_id, task_id)?;
            previous = old.previous_task_id;
        }
        Ok(())
    }

    async fn execute_owned(
        &self,
        task: &SecretaryTask,
        config: &ResolvedAgentRuntime,
        execution: &Arc<Execution>,
        mut cancellation: watch::Receiver<bool>,
        token: &str,
    ) -> io::Result<SecretaryTaskPhase> {
        super::assistants::secretary_validate_request(&self.home, task)?;
        if task.target.kind == SecretaryTaskKind::Document {
            let file = super::assistants::secretary_document_file(&self.home, task)?;
            self.store.prepare_document(&task.task_id, &file)?;
        }
        let resident = self
            .session_for(config, cancellation.clone(), token)
            .await?;
        let session = Arc::clone(&resident.session);
        let _ = execution.generation.set(session.generation().to_owned());
        *execution.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&session));
        self.store.update(&task.task_id, |t| {
            t.generation = session.generation().into();
            t.thread_id = session.thread_id().into();
            t.isolated_document_writes = config.runtime == ActorRuntime::Codex;
            Ok(())
        })?;
        if *cancellation.borrow() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Secretary task cancelled before submission",
            ));
        }
        *resident.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(execution));
        let prompt = format!(
            "TASK_ID: {}\nGroup: {}\nScope: {}\nKind: {:?}\nThis is the only active task. Use task_id={} on every task-tool call. Read its context and current Group requirements; do not borrow facts or instructions from previous tasks.\n\n{}",
            task.task_id,
            task.target.group_id,
            task.target.scope_key,
            task.target.kind,
            task.task_id,
            super::codex_voice_analyst::SECRETARY_INSTRUCTIONS
        );
        let mut events = session.subscribe();
        // Persist the attempt before writing to the provider. A disconnect at
        // this boundary is quarantined, never placed back in the queue.
        let receipt = loop {
            execution.quiescent.store(false, Ordering::Release);
            execution.attempted.store(true, Ordering::Release);
            let result = tokio::select! {
                receipt = session.start_turn(session.generation(), &task.task_id, &prompt) => receipt,
                _ = async { let _ = cancellation.wait_for(|v| *v).await; } => return Err(io::Error::new(io::ErrorKind::Interrupted, "Secretary task cancelled during admission")),
            };
            match result {
                // WouldBlock confirms that a competing native turn prevented
                // admission. Uncertain deliveries still never enter this retry.
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    execution.attempted.store(false, Ordering::Release);
                    execution.quiescent.store(true, Ordering::Release);
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(250)) => {},
                        _ = async { let _ = cancellation.wait_for(|v| *v).await; } => return Err(io::Error::new(io::ErrorKind::Interrupted, "Secretary task cancelled before admission")),
                    }
                    // Requests from the competing native turn do not belong
                    // to the Secretary task that has not been admitted yet.
                    events = session.subscribe();
                }
                result => break result?,
            }
        };
        self.store.update(&task.task_id, |t| {
            t.phase = SecretaryTaskPhase::Running;
            t.turn_id = receipt.turn_id.clone();
            Ok(())
        })?;
        let mut terminal_deadline = None;
        loop {
            tokio::select! {
                _ = async { let _ = cancellation.wait_for(|v| *v).await; } => return Err(io::Error::new(io::ErrorKind::Interrupted, "Secretary task cancelled")),
                event = events.recv() => {
                    let event = event.map_err(io::Error::other)?;
                    if event.generation != session.generation() { continue; }
                    execution.observe_progress(&event.message, &receipt.turn_id, config, token);
                    let method = event.message["method"].as_str();
                    let raw_request = event.message.get("id").is_some() && method.is_some();
                    // ACP keeps provider request IDs inside its adapter. Its
                    // normalized notifications still require an owner outcome.
                    if raw_request || matches!(method, Some("cccc/approvalRequired" | "mcpServer/elicitation/request")) {
                        let pending = session.permissions();
                        let questions = event.message.pointer("/params/questions")
                            .and_then(Value::as_array)
                            .or_else(|| pending.iter().find_map(|request| request["questions"].as_array()))
                            .map(|questions| questions.iter()
                                .filter_map(|question| question["question"].as_str().or_else(|| question["prompt"].as_str()))
                                .collect::<Vec<_>>().join("\n"))
                            .filter(|text| !text.is_empty());
                        let reply = questions.unwrap_or_else(|| {
                            let requested = pending.iter().find_map(|request| request["title"].as_str())
                                .map_or_else(|| format!("Runtime requested {}", method.unwrap_or("user input")),
                                    |title| format!("Runtime requested approval: {title}"));
                            format!("{requested}. This task cannot approve additional permissions or plans; clarify or narrow the request, or adjust the Runtime permissions before retrying.")
                        });
                        let reply = safe_diagnostic(&io::Error::other(reply), config, token);
                        let status = self.store.update(&task.task_id, |task| {
                            let receipt = task.receipt.get_or_insert_with(|| SecretaryReceipt {
                                status: SecretaryTaskPhase::NeedsUser,
                                output: json!({"reply_text":reply}),
                                document_version: String::new(),
                                committed_at: cccc_contracts::utc_now(),
                            });
                            Ok(receipt.status)
                        })?;
                        if raw_request {
                            let _ = session.respond_error(event.message["id"].clone(), json!({
                                "code":-32000,
                                "message":"Secretary task requires user clarification; no interactive approval was granted"
                            })).await;
                        }
                        return Ok(status);
                    }
                    if event.message["method"] == super::codex_voice_analyst::MANAGED_AGENT_DISCONNECTED_METHOD {
                        return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Secretary provider disconnected; prompt was not replayed"));
                    }
                    if event.message["method"] == "turn/completed"
                        && event.message["params"]["turn"]["id"] == receipt.turn_id {
                        execution.quiescent.store(true, Ordering::Release);
                        let saved = self.store.load(&task.task_id)?;
                        if let Some(receipt) = saved.receipt { return Ok(receipt.status); }
                        let reason = event.message["params"]["turn"]["error"]["message"].as_str()
                            .unwrap_or("Provider turn ended without a terminal secretary submission");
                        self.store.update(&task.task_id, |t| { t.diagnostic = safe_diagnostic(&io::Error::other(reason), config, token); Ok(()) })?;
                        return Ok(SecretaryTaskPhase::Failed);
                    }
                }
                _ = execution.submitted.notified() => {
                    let saved = self.store.load(&task.task_id)?;
                    if saved.receipt.is_some() && terminal_deadline.is_none() {
                        terminal_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(15));
                    }
                }
                _ = async { match terminal_deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending().await,
                } } => {
                    let saved = self.store.load(&task.task_id)?;
                    let _ = session.interrupt(session.generation(), &saved.turn_id).await;
                    return Ok(saved.receipt.ok_or_else(|| io::Error::other("Terminal submission receipt is missing"))?.status);
                }
            }
        }
    }
}

fn safe_diagnostic(error: &io::Error, config: &ResolvedAgentRuntime, token: &str) -> String {
    let mut message = error.to_string().replace(token, "[redacted]");
    for value in config.environment.values().filter(|v| !v.is_empty()) {
        message = message.replace(value, "[redacted]");
    }
    message.chars().take(1600).collect()
}

fn projection(task: &SecretaryTask) -> Value {
    json!({"task_id":task.task_id,"target":task.target,"phase":task.phase,
        "created_at":task.created_at,"updated_at":task.updated_at,"cleanup_confirmed":task.cleanup_confirmed,
        "source_count":task.inputs.len(),"preview":task.preview(),"projected_at":task.projected_at,
        "receipt":task.receipt,"diagnostic":task.diagnostic,"projection_error":task.projection_error,
        "previous_task_id":task.previous_task_id,"superseded_by":task.superseded_by,
        "candidate_available":task.target.kind==SecretaryTaskKind::Document && !task.base_version.is_empty() && !task.artifacts_retired,
        "cancellation_reason":task.cancellation_reason,"recovery_attempts":task.recovery_attempts,
        "forwarded_event_id":task.forwarded_event_id})
}

fn observed_projection(home: &HomeLayout, task: &SecretaryTask, state: &Value) -> Value {
    let mut value = projection(task);
    if task.target.kind == SecretaryTaskKind::Prompt {
        let request = &state["voice_prompt_requests"][&task.target.request_id];
        // Existing canonical input can supply a summary; never parse model instructions.
        if task.preview().is_empty()
            && request["composer_snapshot_hash"].as_str().unwrap_or("")
                == task.target.composer_snapshot_hash
            && task.inputs.last().is_some_and(|input| {
                input["input_append_id"].is_string()
                    && input["input_append_id"] == request["last_input_append_id"]
            })
        {
            let text = request["composer_text"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    request["voice_transcripts"]
                        .as_array()
                        .and_then(|items| items.first())
                        .and_then(Value::as_str)
                })
                .unwrap_or("");
            value["preview"] = json!(text.chars().take(240).collect::<String>());
        }
        let draft = &state["voice_prompt_drafts"][&task.target.request_id];
        if draft["secretary_task_id"] == task.task_id
            && let Some(status @ ("pending" | "applied" | "dismissed" | "stale" | "no_change")) =
                draft["status"].as_str()
        {
            value["prompt_draft_status"] = json!(status);
        }
    }
    if let Some(execution) = terminal::observation(home, task) {
        value["execution"] = json!(execution);
    }
    value
}

fn tasks(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    cccc_core::GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&group_id)
        .map_err(OpError::not_found)?;
    let view = owner_group_view(home, &group_id).map_err(OpError::io)?;
    let state = cccc_core::assistant_state::load_workflow(home, &group_id).map_err(OpError::io)?;
    let sources = source_counts(home, &group_id).map_err(OpError::io)?;
    let readiness = configuration::readiness(home);
    object(configuration::with_readiness(
        json!({"group_id":group_id,"tasks":view.tasks.iter().map(|task| observed_projection(home, task, &state)).collect::<Vec<_>>(),
        "global_owner":true,
        "deferred_sources":sources.deferred,"held_sources":sources.held,"invalid_sources":sources.invalid,
        "unprocessed_document_sources":view.unprocessed_document_sources}),
        &readiness,
    ))
}

fn cancel(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    configuration::require_user(request)?;
    let group_id = required_arg(request, "group_id")?;
    let task_id = required_arg(request, "task_id")?;
    let store = task_store(home);
    let manager = lookup(home);
    let acceptance = manager
        .as_ref()
        .map(|m| m.acceptance.lock().unwrap_or_else(|e| e.into_inner()));
    let task = store.load(&task_id).map_err(OpError::not_found)?;
    if task.target.group_id != group_id {
        return Err(OpError::new(
            "permission_denied",
            "Task belongs to another Group",
        ));
    }
    if let Some(manager) = manager.as_ref()
        && let Some(execution) = manager
            .grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .find(|e| e.task_id == task_id)
    {
        store
            .update(&task_id, |t| {
                t.cancellation_reason = "user".into();
                Ok(())
            })
            .map_err(OpError::io)?;
        execution.cancel.send_replace(true);
        return object(json!({"task_id":task_id,"cancel_requested":true}));
    }
    store
        .update(&task_id, |task| {
            if task.phase == SecretaryTaskPhase::Queued {
                task.phase = SecretaryTaskPhase::Cancelled;
                task.cancellation_reason = "user".into();
            }
            Ok(())
        })
        .map_err(OpError::io)?;
    drop(acceptance);
    if let Some(manager) = manager {
        block_on(manager.project(&store.load(&task_id).map_err(OpError::io)?));
    }
    object(json!({"task":projection(&store.load(&task_id).map_err(OpError::io)?)}))
}

fn block_on<F: std::future::Future + Send>(future: F) -> F::Output
where
    F::Output: Send,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::scope(|scope| {
            scope
                .spawn(|| managed_runtime().block_on(future))
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    } else {
        managed_runtime().block_on(future)
    }
}
