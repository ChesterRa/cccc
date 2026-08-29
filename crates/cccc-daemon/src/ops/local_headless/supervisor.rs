use super::{HeadlessStatus, Session, Turn, events, poisoned, provider_cli, session, turn_channel};
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

// Claude may need time to initialize MCP servers before a startup exit can be
// observed reliably under load.
const CLAUDE_STARTUP_GRACE: Duration = Duration::from_secs(1);

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
    let (mut command, session_command) = provider_command(home, group, actor, &mut env)?;
    let claude_session = if actor.runtime == ActorRuntime::Claude {
        let prepared = super::super::runtime_session::prepare_claude_headless_session(
            home,
            &group.group_id,
            &actor.id,
            &cwd,
            &session_command,
        )?;
        if let Some((session_id, resumed)) = &prepared {
            command.splice(
                1..1,
                [
                    if *resumed { "--resume" } else { "--session-id" }.into(),
                    session_id.clone(),
                ],
            );
        }
        prepared
    } else {
        None
    };
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
        resumed_provider_session_id: Mutex::new(
            claude_session
                .as_ref()
                .filter(|(_, resumed)| *resumed)
                .map(|(session_id, _)| session_id.clone())
                .unwrap_or_default(),
        ),
        active_turn: Mutex::new(None),
        completion: (Mutex::new(0), Condvar::new()),
        turns,
    });
    session::spawn_reader(Arc::clone(&item), stdout)?;
    session::spawn_stderr(stderr, &group.group_id, &actor.id)?;
    if actor.runtime == ActorRuntime::Codex {
        if let Err(error) = super::protocol::initialize_codex(&item, &cwd, &model, &session_command)
        {
            item.stop();
            return Err(error);
        }
    } else {
        let resumed = claude_session.as_ref().is_some_and(|(_, resumed)| *resumed);
        // Serialize the startup liveness check and metadata refresh with the
        // stdout reader's resume invalidation. Otherwise an exit between the
        // check and record could be marked failed and then overwritten as usable.
        let resume_record_guard = if resumed {
            Some(
                item.resumed_provider_session_id
                    .lock()
                    .map_err(|_| poisoned())?,
            )
        } else {
            None
        };
        std::thread::sleep(CLAUDE_STARTUP_GRACE);
        if !item.running() {
            let error = if resumed {
                "claude headless resume process exited during startup"
            } else {
                "claude headless process exited during startup"
            };
            drop(resume_record_guard);
            if resumed {
                item.stop_after_invalidate(|| {
                    session::invalidate_pending_claude_resume(&item, error);
                });
            }
            return Err(io::Error::other(error));
        }
        if let Some((session_id, resumed)) = claude_session
            && let Err(error) = super::super::runtime_session::record_claude_headless_session(
                home,
                &group.group_id,
                &actor.id,
                &cwd,
                &session_command,
                &session_id,
                resumed,
            )
        {
            tracing::warn!(
                %error,
                group_id = %group.group_id,
                actor_id = %actor.id,
                "failed to persist Claude headless session"
            );
        }
        drop(resume_record_guard);
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
        control_kind: "bootstrap".into(),
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
    let Some((delivery, control_kind)) = render_turn_with_mail_context(home, group, actor, event)
    else {
        return false;
    };
    let queued = item
        .turns
        .try_send(Turn {
            text: delivery,
            event_id: event.id.clone(),
            control_kind,
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

fn render_turn_with_mail_context(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    event: &Event,
) -> Option<(String, String)> {
    super::super::actor_delivery_render::render_batch_with_mail_context(
        home,
        group,
        &actor.id,
        std::slice::from_ref(event),
    )
    .map(|text| {
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

fn provider_command(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    env: &mut BTreeMap<String, String>,
) -> io::Result<(Vec<String>, Vec<String>)> {
    let base = provider_cli::base_command(actor);
    if !provider_cli::is_provider_binary(actor, &base) {
        return Ok((base.clone(), base));
    }
    if actor.runtime == ActorRuntime::Codex {
        let mut command = vec![base[0].clone()];
        command.extend(preserved_codex_args(&base[1..]));
        command.extend(["app-server".into(), "--listen".into(), "stdio://".into()]);
        let session_command = command.clone();
        super::super::codex_mcp::configure_mcp_only(
            home,
            &group.group_id,
            &actor.id,
            &mut command,
            env,
        );
        Ok((command, session_command))
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
        let session_command = command.clone();
        if let Some(executable) = super::super::codex_mcp::resolve_cccc_executable() {
            let config = json!({"mcpServers":{"cccc":{"command":executable,"args":["mcp"],"env":{"CCCC_HOME":home.root(),"CCCC_GROUP_ID":group.group_id,"CCCC_ACTOR_ID":actor.id}}}});
            command.extend(["--mcp-config".into(), config.to_string()]);
        }
        Ok((command, session_command))
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
        let (codex_command, codex_session_command) =
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
        assert_eq!(
            codex_session_command,
            vec![
                "codex",
                "-c",
                "feature=true",
                "--search",
                "--model",
                "gpt-test",
                "app-server",
                "--listen",
                "stdio://",
            ]
        );
        assert!(
            !codex_session_command
                .iter()
                .any(|arg| arg.contains("mcp_servers.cccc"))
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
        let (claude_command, claude_session_command) =
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
        assert_eq!(
            claude_session_command,
            vec![
                "claude",
                "--model",
                "claude-test",
                "--allowedTools",
                "Read,Write",
                "--mcp-config",
                "custom.json",
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--include-partial-messages",
                "--include-hook-events",
                "--verbose",
                "--dangerously-skip-permissions",
            ]
        );
        assert!(
            !claude_session_command
                .iter()
                .any(|arg| arg.contains("mcpServers"))
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

    #[cfg(unix)]
    #[test]
    fn claude_resume_exit_after_startup_grace_is_invalidated_before_retry() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let mut group = GroupStore::new(home.clone())
            .expect("store")
            .create("stale Claude resume", "")
            .expect("group");
        group.scopes.push(Scope {
            scope_key: "s_project".into(),
            url: temp.path().to_string_lossy().into_owned(),
            label: "project".into(),
            git_remote: String::new(),
        });
        group.active_scope_key = "s_project".into();

        let bin_dir = temp.path().join("bin");
        std::fs::create_dir(&bin_dir).expect("bin dir");
        let executable = bin_dir.join("claude");
        std::fs::write(
            &executable,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$CCCC_TEST_ARGS"
if [ ! -e "$CCCC_TEST_FIRST_LAUNCH" ]; then
  : > "$CCCC_TEST_FIRST_LAUNCH"
  sleep 2
  exit 2
fi
while IFS= read -r line; do :; done
"#,
        )
        .expect("fake Claude");
        let mut permissions = std::fs::metadata(&executable)
            .expect("fake Claude metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).expect("fake Claude executable");

        let args_log = temp.path().join("claude-args.log");
        let first_launch = temp.path().join("claude-first-launch");
        let mut actor = Actor::new("headless");
        actor.role = Some(ActorRole::Foreman);
        actor.runtime = ActorRuntime::Claude;
        actor.runner = RunnerKind::Headless;
        actor.command = vec![executable.to_string_lossy().into_owned()];
        actor.env.insert(
            "CCCC_TEST_ARGS".into(),
            args_log.to_string_lossy().into_owned(),
        );
        actor.env.insert(
            "CCCC_TEST_FIRST_LAUNCH".into(),
            first_launch.to_string_lossy().into_owned(),
        );
        group.actors.push(actor.clone());

        let mut command_env = actor.env.clone();
        let (_, session_command) =
            provider_command(&home, &group, &actor, &mut command_env).expect("provider command");
        let stale_session = "42e9ef0c-3b75-43a0-9056-eef13dd1061d";
        crate::ops::runtime_session::record_claude_headless_session(
            &home,
            &group.group_id,
            &actor.id,
            temp.path(),
            &session_command,
            stale_session,
            false,
        )
        .expect("seed stale session");

        let session_path = GroupStore::new(home.clone())
            .expect("store")
            .state_dir(&group.group_id)
            .expect("state dir")
            .join("runtime_sessions")
            .join(format!("{}.json", actor.id));
        if let Err(error) = start(&home, &group, &actor) {
            assert!(error.to_string().contains("resume process exited"));
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let rejected = loop {
            let document: serde_json::Value =
                cccc_core::fs::read_json(&session_path).expect("rejected session metadata");
            if !running(&group.group_id, &actor.id) && document["status"] == "resume_failed" {
                break document;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "stale resume process or metadata did not settle: {document}"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(rejected["status"], "resume_failed");
        assert_eq!(rejected["resume_eligible"], false);
        assert_eq!(rejected["failure_count"], 1);
        assert_eq!(rejected["provider_session_id"], stale_session);

        start(&home, &group, &actor).expect("explicit retry");
        let recovered: serde_json::Value =
            cccc_core::fs::read_json(&session_path).expect("recovered session metadata");
        let recovered_session = recovered["provider_session_id"]
            .as_str()
            .expect("recovered session id");
        assert_ne!(recovered_session, stale_session);
        assert!(uuid::Uuid::parse_str(recovered_session).is_ok());
        assert_eq!(recovered["status"], "usable");
        assert_eq!(recovered["resume_eligible"], true);

        let launches = std::fs::read_to_string(&args_log).expect("Claude argv log");
        let lines = launches.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(&format!("--resume {stale_session}")));
        assert!(lines[1].contains(&format!("--session-id {recovered_session}")));
        stop(&group.group_id, &actor.id);
    }

    #[cfg(unix)]
    #[test]
    fn delayed_claude_resume_rejection_is_invalidated_before_retry() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let mut group = GroupStore::new(home.clone())
            .expect("store")
            .create("delayed stale Claude resume", "")
            .expect("group");
        group.scopes.push(Scope {
            scope_key: "s_project".into(),
            url: temp.path().to_string_lossy().into_owned(),
            label: "project".into(),
            git_remote: String::new(),
        });
        group.active_scope_key = "s_project".into();

        let executable = temp.path().join("claude");
        std::fs::write(
            &executable,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$CCCC_TEST_ARGS"
case " $* " in
  *' --resume '*)
    IFS= read -r line || exit 0
    printf '{"type":"result","subtype":"error_during_execution","is_error":true,"result":"No conversation found for stale session"}\n'
    while IFS= read -r line; do :; done
    ;;
  *)
    while IFS= read -r line; do
      printf '{"type":"result","subtype":"success","is_error":false}\n'
    done
    ;;
esac
"#,
        )
        .expect("fake Claude");
        let mut permissions = std::fs::metadata(&executable)
            .expect("fake Claude metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).expect("fake Claude executable");

        let args_log = temp.path().join("claude-args.log");
        let mut actor = Actor::new("headless");
        actor.role = Some(ActorRole::Foreman);
        actor.runtime = ActorRuntime::Claude;
        actor.runner = RunnerKind::Headless;
        actor.command = vec![executable.to_string_lossy().into_owned()];
        actor.env.insert(
            "CCCC_TEST_ARGS".into(),
            args_log.to_string_lossy().into_owned(),
        );
        group.actors.push(actor.clone());

        let mut command_env = actor.env.clone();
        let (_, session_command) =
            provider_command(&home, &group, &actor, &mut command_env).expect("provider command");
        let stale_session = "42e9ef0c-3b75-43a0-9056-eef13dd1061d";
        crate::ops::runtime_session::record_claude_headless_session(
            &home,
            &group.group_id,
            &actor.id,
            temp.path(),
            &session_command,
            stale_session,
            false,
        )
        .expect("seed stale session");

        start(&home, &group, &actor).expect("provider survives startup grace");
        let session_path = GroupStore::new(home.clone())
            .expect("store")
            .state_dir(&group.group_id)
            .expect("state dir")
            .join("runtime_sessions")
            .join(format!("{}.json", actor.id));
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let rejected = loop {
            let document: serde_json::Value =
                cccc_core::fs::read_json(&session_path).expect("runtime session metadata");
            if document["status"] == "resume_failed" {
                break document;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "delayed resume rejection was not persisted: {document}"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(rejected["resume_eligible"], false);
        assert_eq!(rejected["provider_session_id"], stale_session);
        assert!(!running(&group.group_id, &actor.id));
        let events_path = GroupStore::new(home.clone())
            .expect("store")
            .state_dir(&group.group_id)
            .expect("state dir")
            .join("headless/events.jsonl");
        let event_deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let events = std::fs::read_to_string(&events_path).expect("headless events");
            if events.contains("headless.session.resume_failed") {
                break;
            }
            assert!(
                std::time::Instant::now() < event_deadline,
                "resume rejection event was not persisted: {events}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        start(&home, &group, &actor).expect("fresh retry");
        let recovered: serde_json::Value =
            cccc_core::fs::read_json(&session_path).expect("recovered session metadata");
        let recovered_session = recovered["provider_session_id"]
            .as_str()
            .expect("recovered session id");
        assert_ne!(recovered_session, stale_session);
        assert_eq!(recovered["status"], "usable");
        assert_eq!(recovered["resume_eligible"], true);
        let launches = std::fs::read_to_string(&args_log).expect("Claude argv log");
        let lines = launches.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(&format!("--resume {stale_session}")));
        assert!(lines[1].contains(&format!("--session-id {recovered_session}")));
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
    assert_eq!(
        render_turn(&notify).expect("notify turn").1,
        "system_notify"
    );
}
