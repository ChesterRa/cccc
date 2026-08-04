use super::{HeadlessStatus, Session, Turn, events, poisoned, session, turn_channel};
use cccc_contracts::{Actor, ActorRuntime, Event, RunnerKind, utc_now};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};
use std::time::Duration;

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
    actor.runner == RunnerKind::Headless
        && matches!(actor.runtime, ActorRuntime::Codex | ActorRuntime::Claude)
}

pub fn start(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> io::Result<()> {
    if !supports(actor) {
        return Ok(());
    }
    let key = (group.group_id.clone(), actor.id.clone());
    let _start = StartGuard::acquire(&key)?;
    if lookup(&key).is_some_and(|item| item.running()) {
        return Ok(());
    }
    stop(&group.group_id, &actor.id);

    let cwd = working_directory(group, actor)?;
    let mut env = actor.env.clone();
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
    env.insert("CCCC_GROUP_ID".into(), group.group_id.clone());
    env.insert("CCCC_ACTOR_ID".into(), actor.id.clone());
    env.insert("CCCC_RUNNER".into(), "headless".into());
    super::super::codex_mcp::configure_actor_cli(&mut env);
    let model = model_from_command(&actor.command);
    let command = provider_command(home, group, actor, &mut env)?;
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty headless command"))?;
    let mut process = Command::new(program);
    process
        .args(args)
        .current_dir(&cwd)
        .envs(&env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process.spawn()?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("headless stdin unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("headless stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("headless stderr unavailable"))?;
    let pid = child.id();
    let (turns, receiver) = turn_channel();
    let item = Arc::new(Session {
        home: home.clone(),
        group_id: group.group_id.clone(),
        actor_id: actor.id.clone(),
        runtime: actor.runtime,
        child: Mutex::new(child),
        stdin: Mutex::new(stdin),
        status: Mutex::new(HeadlessStatus {
            status: "idle".into(),
            task_id: None,
            updated_at: utc_now(),
            pid: Some(pid),
        }),
        stopped: AtomicBool::new(false),
        next_request_id: AtomicU64::new(1),
        pending: Mutex::new(HashMap::new()),
        thread_id: Mutex::new(String::new()),
        active_event_id: Mutex::new(String::new()),
        completion: (Mutex::new(0), Condvar::new()),
        turns,
    });
    session::spawn_reader(Arc::clone(&item), stdout)?;
    session::spawn_stderr(stderr, &group.group_id, &actor.id)?;
    if actor.runtime == ActorRuntime::Codex {
        if let Err(error) = super::protocol::initialize_codex(&item, &cwd, &model) {
            item.stop();
            return Err(error);
        }
    } else {
        std::thread::sleep(Duration::from_millis(100));
        if !item.running() {
            return Err(io::Error::other(
                "claude headless process exited during startup",
            ));
        }
    }
    sessions()
        .write()
        .map_err(|_| poisoned())?
        .insert(key, Arc::clone(&item));
    session::spawn_worker(Arc::clone(&item), receiver)?;
    let prompt = cccc_core::system_prompt::render_session(home, group, actor);
    let bootstrap = format!(
        "[CCCC] Bootstrap this actor. Use the CCCC MCP tools for coordination and replies.\n\n{prompt}"
    );
    let _ = item.turns.try_send(Turn {
        text: bootstrap,
        event_id: String::new(),
        event_ts: String::new(),
        control: true,
    });
    super::output::emit(&item, "headless.session.started", Map::new());
    Ok(())
}

pub fn stop(group_id: &str, actor_id: &str) {
    let key = (group_id.to_owned(), actor_id.to_owned());
    if let Some(item) = sessions()
        .write()
        .ok()
        .and_then(|mut items| items.remove(&key))
    {
        item.stop();
    }
}

pub fn stop_group(group_id: &str) {
    let drained = sessions()
        .write()
        .map(|mut items| {
            let keys = items
                .keys()
                .filter(|key| key.0 == group_id)
                .cloned()
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| items.remove(&key))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    drained.iter().for_each(|item| item.stop());
}

pub fn stop_all() {
    let drained = sessions()
        .write()
        .map(|mut items| std::mem::take(&mut *items))
        .unwrap_or_default();
    drained.values().for_each(|item| item.stop());
}

#[must_use]
pub fn running(group_id: &str, actor_id: &str) -> bool {
    lookup(&(group_id.to_owned(), actor_id.to_owned())).is_some_and(|item| item.running())
}

#[must_use]
pub fn status(group_id: &str, actor_id: &str) -> Option<HeadlessStatus> {
    let item = lookup(&(group_id.to_owned(), actor_id.to_owned()))?;
    if !item.running() {
        return None;
    }
    item.status.lock().ok().map(|state| state.clone())
}

pub fn submit(home: &HomeLayout, group: &GroupDoc, actor: &Actor, event: &Event) -> bool {
    let Some(item) = lookup(&(group.group_id.clone(), actor.id.clone())) else {
        return false;
    };
    if !item.running() {
        return false;
    }
    let Some((delivery, control)) = render_turn(event) else {
        return false;
    };
    let queued = item
        .turns
        .try_send(Turn {
            text: delivery,
            event_id: event.id.clone(),
            event_ts: event.ts.clone(),
            control,
        })
        .is_ok();
    if queued {
        let _ = events::append(
            home,
            &group.group_id,
            &actor.id,
            "headless.turn.queued",
            Map::from_iter([("event_id".into(), json!(event.id))]),
        );
    }
    queued
}

fn render_turn(event: &Event) -> Option<(String, bool)> {
    super::super::actor_delivery_render::render_batch(std::slice::from_ref(event))
        .map(|text| (text, event.kind == "system.notify"))
}

fn lookup(key: &Key) -> Option<Arc<Session>> {
    sessions().read().ok()?.get(key).cloned()
}

fn provider_command(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    env: &mut BTreeMap<String, String>,
) -> io::Result<Vec<String>> {
    let base = if actor.command.is_empty() {
        cccc_runtime::default_command(actor.runtime)
    } else {
        actor.command.clone()
    };
    let provider_binary = base.first().is_some_and(|program| {
        std::path::Path::new(program)
            .file_stem()
            .and_then(|value| value.to_str())
            == Some(match actor.runtime {
                ActorRuntime::Codex => "codex",
                _ => "claude",
            })
    });
    if !provider_binary {
        return Ok(base);
    }
    if actor.runtime == ActorRuntime::Codex {
        let mut command = vec![base[0].clone()];
        command.extend(preserved_codex_args(&base[1..]));
        command.extend(["app-server".into(), "--listen".into(), "stdio://".into()]);
        super::super::codex_mcp::configure_mcp_only(
            home,
            &group.group_id,
            &actor.id,
            &mut command,
            env,
        );
        Ok(command)
    } else {
        let mut command = vec![base[0].clone()];
        command.extend(preserved_claude_args(&base[1..]));
        command.extend([
            "-p".into(),
            "--input-format".into(),
            "stream-json".into(),
            "--output-format".into(),
            "stream-json".into(),
            "--include-partial-messages".into(),
            "--include-hook-events".into(),
            "--verbose".into(),
            "--dangerously-skip-permissions".into(),
        ]);
        if let Some(executable) = super::super::codex_mcp::resolve_cccc_executable() {
            let config = json!({"mcpServers":{"cccc":{"command":executable,"args":["mcp"],"env":{"CCCC_HOME":home.root(),"CCCC_GROUP_ID":group.group_id,"CCCC_ACTOR_ID":actor.id}}}});
            command.extend(["--mcp-config".into(), config.to_string()]);
        }
        Ok(command)
    }
}

fn preserved_codex_args(args: &[String]) -> Vec<String> {
    let mut preserved = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "app-server" {
            index += 1;
        } else if arg == "--listen" {
            index += 2;
        } else if arg.starts_with("--listen=") {
            index += 1;
        } else {
            preserved.push(arg.clone());
            index += 1;
        }
    }
    preserved
}

fn preserved_claude_args(args: &[String]) -> Vec<String> {
    const VALUE_OPTIONS: &[&str] = &["--input-format", "--output-format"];
    const FLAGS: &[&str] = &[
        "-p",
        "--print",
        "--include-partial-messages",
        "--include-hook-events",
        "--verbose",
        "--dangerously-skip-permissions",
    ];
    let mut preserved = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if VALUE_OPTIONS.contains(&arg.as_str()) {
            index += 2;
        } else if VALUE_OPTIONS
            .iter()
            .any(|option| arg.starts_with(&format!("{option}=")))
            || FLAGS.contains(&arg.as_str())
        {
            index += 1;
        } else {
            preserved.push(arg.clone());
            index += 1;
        }
    }
    preserved
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

fn model_from_command(command: &[String]) -> String {
    command
        .windows(2)
        .find(|items| matches!(items[0].as_str(), "-m" | "--model"))
        .map(|items| items[1].clone())
        .or_else(|| {
            command
                .iter()
                .find_map(|item| item.strip_prefix("--model=").map(str::to_owned))
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::ActorRole;
    use cccc_core::{GroupStore, Scope};

    #[test]
    fn provider_commands_preserve_compatible_actor_arguments() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("commands", "")
            .expect("group");
        let mut env = BTreeMap::new();

        let mut codex = Actor::new("codex");
        codex.runtime = ActorRuntime::Codex;
        codex.runner = RunnerKind::Headless;
        codex.command = vec![
            "codex".into(),
            "-c".into(),
            "feature=true".into(),
            "--search".into(),
            "--model".into(),
            "gpt-test".into(),
        ];
        let codex_command =
            provider_command(&home, &group, &codex, &mut env).expect("codex command");
        assert!(
            codex_command
                .windows(2)
                .any(|args| args == ["-c", "feature=true"])
        );
        assert!(codex_command.iter().any(|arg| arg == "--search"));
        assert!(
            codex_command
                .windows(2)
                .any(|args| args == ["--model", "gpt-test"])
        );
        assert_eq!(
            codex_command
                .iter()
                .filter(|arg| arg.as_str() == "app-server")
                .count(),
            1
        );

        let mut claude = Actor::new("claude");
        claude.runtime = ActorRuntime::Claude;
        claude.runner = RunnerKind::Headless;
        claude.command = vec![
            "claude".into(),
            "--model".into(),
            "claude-test".into(),
            "--allowedTools".into(),
            "Read,Write".into(),
            "--mcp-config".into(),
            "custom.json".into(),
        ];
        let claude_command =
            provider_command(&home, &group, &claude, &mut env).expect("claude command");
        assert!(
            claude_command
                .windows(2)
                .any(|args| args == ["--model", "claude-test"])
        );
        assert!(
            claude_command
                .windows(2)
                .any(|args| args == ["--allowedTools", "Read,Write"])
        );
        assert!(
            claude_command
                .windows(2)
                .any(|args| args == ["--mcp-config", "custom.json"])
        );
    }

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

    #[cfg(unix)]
    #[test]
    fn concurrent_starts_create_one_provider_process() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let mut group = GroupStore::new(home.clone())
            .expect("store")
            .create("concurrent", "")
            .expect("group");
        group.scopes.push(Scope {
            scope_key: "s_project".into(),
            url: temp.path().to_string_lossy().into_owned(),
            label: "project".into(),
            git_remote: String::new(),
        });
        group.active_scope_key = "s_project".into();
        let starts_path = temp.path().join("starts");
        let mut actor = Actor::new("headless");
        actor.role = Some(ActorRole::Foreman);
        actor.runtime = ActorRuntime::Claude;
        actor.runner = RunnerKind::Headless;
        actor.command = vec![
            "sh".into(),
            "-c".into(),
            "printf x >> \"$1\"; while IFS= read -r line; do :; done".into(),
            "cccc-headless-test".into(),
            starts_path.to_string_lossy().into_owned(),
        ];
        group.actors.push(actor.clone());

        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut threads = Vec::new();
        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            let home = home.clone();
            let group = group.clone();
            let actor = actor.clone();
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                start(&home, &group, &actor)
            }));
        }
        barrier.wait();
        for thread in threads {
            thread.join().expect("start thread").expect("start actor");
        }

        assert_eq!(
            std::fs::read_to_string(&starts_path).expect("start count"),
            "x"
        );
        stop(&group.group_id, &actor.id);
    }
}
#[test]
fn headless_turn_uses_complete_envelope_and_control_semantics() {
    let mut message = Event::new("chat.message", "g_demo");
    message.by = "user".into();
    message.data = json!({
        "text":"review",
        "to":["architect"],
        "priority":"attention",
        "reply_required":true,
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
    assert!(!control);
    for expected in [
        "IMPORTANT",
        "REPLY REQUIRED",
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
    notify.data = json!({"kind":"nudge","message":"check status"})
        .as_object()
        .cloned()
        .expect("notify");
    assert!(render_turn(&notify).expect("notify turn").1);
}
