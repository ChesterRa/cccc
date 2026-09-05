use super::super::codex_voice_analyst::lifecycle_timing;
use super::{HeadlessStatus, Session, managed_reader, managed_runtime, poisoned, provider_cli};
use cccc_contracts::{Actor, ActorRuntime, Event, RunnerKind, utc_now};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::Map;
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};
use tracing::Instrument;

type Key = (String, String);

fn sessions() -> &'static RwLock<HashMap<Key, Arc<Session>>> {
    static SESSIONS: OnceLock<RwLock<HashMap<Key, Arc<Session>>>> = OnceLock::new();
    SESSIONS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn starts() -> &'static (Mutex<HashSet<Key>>, Condvar) {
    static STARTS: OnceLock<(Mutex<HashSet<Key>>, Condvar)> = OnceLock::new();
    STARTS.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()))
}

struct StartGuard {
    key: Key,
}

impl StartGuard {
    fn acquire(key: &Key) -> io::Result<Self> {
        let (active, changed) = starts();
        let mut active = active.lock().map_err(|_| poisoned())?;
        while active.contains(key) {
            active = changed.wait(active).map_err(|_| poisoned())?;
        }
        active.insert(key.clone());
        Ok(Self { key: key.clone() })
    }
}

impl Drop for StartGuard {
    fn drop(&mut self) {
        let (active, changed) = starts();
        if let Ok(mut active) = active.lock() {
            active.remove(&self.key);
            changed.notify_all();
        }
    }
}

#[must_use]
pub fn supports(actor: &Actor) -> bool {
    uses_managed_session(actor)
}

pub(super) fn uses_managed_session(actor: &Actor) -> bool {
    matches!(
        actor.runtime,
        ActorRuntime::Claude
            | ActorRuntime::Codex
            | ActorRuntime::Grok
            | ActorRuntime::Opencode
            | ActorRuntime::Kilo
    )
}

fn start_managed_agent(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    key: Key,
) -> io::Result<()> {
    let cwd = working_directory(group, actor)?;
    let mut env = actor.env.clone();
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
    env.insert("CCCC_GROUP_ID".into(), group.group_id.clone());
    env.insert("CCCC_ACTOR_ID".into(), actor.id.clone());
    super::super::codex_mcp::configure_actor_cli(&mut env);
    let app = managed_runtime().block_on(
        super::super::codex_voice_analyst::AnalystSession::launch_actor(
            home,
            super::super::codex_voice_analyst::ActorLaunchConfig {
                workdir: cwd.clone(),
                group_id: group.group_id.clone(),
                actor_id: actor.id.clone(),
                runtime: actor.runtime,
                command: provider_cli::base_command(actor),
                environment: env,
            },
        )
        .instrument(tracing::info_span!(
            "actor_runtime_start", group_id = %group.group_id, actor_id = %actor.id
        )),
    )?;
    let app = Arc::new(app);
    let prompt = cccc_core::system_prompt::render_session(home, group, actor);
    let item = Arc::new(Session {
        home: home.clone(),
        group_id: group.group_id.clone(),
        actor_id: actor.id.clone(),
        managed: Arc::clone(&app),
        has_terminal: AtomicBool::new(false),
        status: Mutex::new(HeadlessStatus {
            status: "idle".into(),
            task_id: None,
            updated_at: utc_now(),
            pid: app.process_id(),
        }),
        stopped: AtomicBool::new(false),
        stop_lock: Mutex::new(()),
        startup_prompt: Mutex::new(Some(prompt)),
        active_turn: Mutex::new(None),
    });
    if let Err(error) = managed_reader::spawn(Arc::clone(&item), app.subscribe()) {
        return Err(cleanup_failed_start(&item, error));
    }
    let history = match super::super::actor_runtime::terminal_history::config(
        home,
        &group.group_id,
        &actor.id,
    ) {
        Ok(history) => history,
        Err(error) => {
            return Err(cleanup_failed_start(&item, error));
        }
    };
    let terminal = match lifecycle_timing::run_sync("runtime.terminal_attach", || {
        cccc_runtime::start_with_history(
            cccc_runtime::LaunchSpec {
                group_id: group.group_id.clone(),
                actor_id: actor.id.clone(),
                runner: RunnerKind::Pty,
                command: app.actor_tui_command(),
                cwd: cwd.clone(),
                env: app.tui_environment(),
                cols: 120,
                rows: 40,
            },
            history,
        )
        .map_err(io::Error::other)
    }) {
        Ok(status) => status,
        Err(error) => {
            return Err(cleanup_failed_start(&item, error));
        }
    };
    item.attach_terminal(terminal.pid);
    if let Err(error) = sessions()
        .write()
        .map_err(|_| poisoned())
        .map(|mut items| items.insert(key.clone(), Arc::clone(&item)))
    {
        return Err(cleanup_failed_start(&item, error));
    }
    super::output::emit(&item, "headless.session.started", Map::new());
    Ok(())
}

fn cleanup_failed_start(item: &Session, primary: io::Error) -> io::Error {
    match item.stop() {
        Ok(_) => primary,
        Err(cleanup) => io::Error::new(
            primary.kind(),
            format!("{primary}; managed-session rollback also failed: {cleanup}"),
        ),
    }
}

pub fn start(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> io::Result<()> {
    start_session(home, group, actor)
}

fn start_session(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> io::Result<()> {
    if !supports(actor) {
        return Ok(());
    }
    let key = (group.group_id.clone(), actor.id.clone());
    let _start = StartGuard::acquire(&key)?;
    if lookup(&key).is_some_and(|item| item.running()) {
        return Ok(());
    }
    stop(&group.group_id, &actor.id)?;

    start_managed_agent(home, group, actor, key)
}

pub fn stop(group_id: &str, actor_id: &str) -> io::Result<()> {
    let key = (group_id.to_owned(), actor_id.to_owned());
    let Some(item) = lookup(&key) else {
        return Ok(());
    };
    item.stop()?;
    let mut items = sessions().write().map_err(|_| poisoned())?;
    if items
        .get(&key)
        .is_some_and(|current| Arc::ptr_eq(current, &item))
    {
        items.remove(&key);
    }
    Ok(())
}

pub fn stop_group(group_id: &str) -> io::Result<()> {
    let actor_ids = sessions()
        .read()
        .map_err(|_| poisoned())?
        .keys()
        .filter(|key| key.0 == group_id)
        .map(|key| key.1.clone())
        .collect::<Vec<_>>();
    for actor_id in actor_ids {
        stop(group_id, &actor_id)?;
    }
    Ok(())
}

pub fn stop_all() -> io::Result<()> {
    let keys = sessions()
        .read()
        .map_err(|_| poisoned())?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let mut failures = Vec::new();
    for (group_id, actor_id) in keys {
        if let Err(error) = stop(&group_id, &actor_id) {
            failures.push(format!("{group_id}/{actor_id}: {error}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "failed to stop managed Actors: {}",
            failures.join("; ")
        )))
    }
}

#[must_use]
pub fn running(group_id: &str, actor_id: &str) -> bool {
    lookup(&(group_id.to_owned(), actor_id.to_owned())).is_some_and(|item| item.running())
}

#[must_use]
pub fn status(group_id: &str, actor_id: &str) -> Option<HeadlessStatus> {
    let item = lookup(&(group_id.to_owned(), actor_id.to_owned()))?;
    let running = item.running();
    let state = item.status.lock().ok()?;
    if !running && state.status != "error" {
        return None;
    }
    // A failed teardown retains an owned job whose execution is unverified.
    // Keep its error visible instead of projecting it as successfully stopped.
    Some(state.clone())
}

#[cfg(test)]
pub fn submit(home: &HomeLayout, group: &GroupDoc, actor: &Actor, event: &Event) -> bool {
    submit_batch(
        home,
        group,
        actor,
        std::slice::from_ref(event),
        &AtomicBool::new(false),
    )
}

pub fn submit_batch(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    source_events: &[Event],
    cancelled: &AtomicBool,
) -> bool {
    let Some(item) = lookup(&(group.group_id.clone(), actor.id.clone())) else {
        return false;
    };
    if !item.running() {
        return false;
    }
    let Some(delivery) = super::super::actor_delivery_render::render_batch_with_mail_context(
        home,
        group,
        &actor.id,
        source_events,
    ) else {
        return false;
    };
    if item.has_terminal() {
        return submit_with_startup_prompt(&item.startup_prompt, &delivery, |prepared| {
            super::super::actor_delivery::submit_terminal_text(
                &group.group_id,
                actor,
                prepared,
                cancelled,
            )
        });
    }
    false
}

fn submit_with_startup_prompt(
    startup_prompt: &Mutex<Option<String>>,
    delivery: &str,
    submit: impl FnOnce(&str) -> bool,
) -> bool {
    let Ok(mut startup_prompt) = startup_prompt.lock() else {
        return false;
    };
    let prepared = startup_prompt.as_ref().map_or_else(
        || delivery.to_owned(),
        |prompt| format!("{prompt}\n\n{delivery}"),
    );
    let accepted = submit(&prepared);
    if accepted {
        *startup_prompt = None;
    }
    accepted
}

#[cfg(test)]
fn render_turn(event: &Event) -> Option<(String, String)> {
    super::super::actor_delivery_render::render_batch(std::slice::from_ref(event)).map(|text| {
        (
            text,
            if event.kind == "system.notify" {
                "system_notify".into()
            } else {
                String::new()
            },
        )
    })
}

fn lookup(key: &Key) -> Option<Arc<Session>> {
    sessions().read().ok()?.get(key).cloned()
}

fn working_directory(group: &GroupDoc, actor: &Actor) -> io::Result<std::path::PathBuf> {
    let wanted = if actor.default_scope_key.is_empty() {
        &group.active_scope_key
    } else {
        &actor.default_scope_key
    };
    if wanted.is_empty() {
        return std::env::current_dir();
    }
    let scope = cccc_core::group_scope::resolve_attached_scope(group, wanted).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("scope not attached: {wanted}"),
        )
    })?;
    let path = std::path::PathBuf::from(&scope.url);
    if !path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("project root path does not exist: {}", path.display()),
        ));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::{GroupStore, Scope};

    #[test]
    fn invalid_scope_directory_is_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let missing = temp.path().join("missing");
        let mut group =
            GroupStore::new(HomeLayout::from_path(temp.path().join("home")).expect("home"))
                .expect("store")
                .create("cwd", "")
                .expect("group");
        group.scopes.push(Scope {
            scope_key: "missing".into(),
            url: missing.to_string_lossy().into_owned(),
            label: "missing".into(),
            git_remote: String::new(),
        });
        group.active_scope_key = "missing".into();

        let error = working_directory(&group, &Actor::new("actor")).expect_err("invalid scope");

        assert!(
            error
                .to_string()
                .contains("project root path does not exist")
        );
    }

    #[test]
    fn startup_prompt_is_sent_with_the_first_accepted_delivery_only() {
        let startup_prompt = Mutex::new(Some("startup context".to_owned()));
        let mut attempts = Vec::new();

        assert!(!submit_with_startup_prompt(
            &startup_prompt,
            "first delivery",
            |prepared| {
                attempts.push(prepared.to_owned());
                false
            },
        ));
        assert!(submit_with_startup_prompt(
            &startup_prompt,
            "first delivery",
            |prepared| {
                attempts.push(prepared.to_owned());
                true
            },
        ));
        assert!(submit_with_startup_prompt(
            &startup_prompt,
            "second delivery",
            |prepared| {
                attempts.push(prepared.to_owned());
                true
            },
        ));

        assert_eq!(
            attempts,
            [
                "startup context\n\nfirst delivery",
                "startup context\n\nfirst delivery",
                "second delivery",
            ]
        );
        assert_eq!(*startup_prompt.lock().expect("startup prompt"), None);
    }
}
#[test]
fn headless_turn_uses_complete_envelope_and_control_semantics() {
    let mut message = Event::new("chat.message", "g_demo");
    message.by = "user".into();
    message.data = serde_json::json!({
        "text":"review",
        "to":["architect"],
        "message_mode":"request_reply",
        "reply_to":"source-event",
        "quote_text":"quoted",
        "insight":"challenge the boundary",
        "attachments":[{"path":"state/blobs/abc","title":"spec.md","bytes":12}],
        "refs":[{"kind":"task_ref","task_id":"t_1","title":"Review"}],
    })
    .as_object()
    .cloned()
    .expect("message");
    let (rendered, control) = render_turn(&message).expect("turn");
    assert!(control.is_empty());
    for expected in [
        "reply_required",
        "(reply:source-e)",
        "quoted",
        "spec.md",
        "task_ref: Review",
        "challenge the boundary",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected}: {rendered}"
        );
    }

    let mut notify = Event::new("system.notify", "g_demo");
    notify.data = serde_json::json!({"kind":"nudge","message":"check status"})
        .as_object()
        .cloned()
        .expect("notify");
    assert_eq!(
        render_turn(&notify).expect("notify turn").1,
        "system_notify"
    );
}
