use cccc_contracts::RunnerKind;
use cccc_core::HomeLayout;
use cccc_runtime::CommandSession;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const MAX_SESSIONS_PER_HOME: usize = 64;
const FINISHED_RETENTION: Duration = Duration::from_secs(600);

struct LocalSession {
    home: PathBuf,
    group_id: String,
    command: CommandSession,
    io: tokio::sync::Mutex<()>,
    cursor: Mutex<u64>,
    closed: AtomicBool,
    timed_out: AtomicBool,
    cleanup_error: Mutex<Option<String>>,
}

type Sessions = HashMap<String, Arc<LocalSession>>;

pub async fn start(
    home: &HomeLayout,
    root: &Path,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let wait = wait_time(args)?;
    let limit = output_limit(args)?;
    let timeout = Duration::from_secs(integer(args, "timeout_s", 600, 1, 600)?);
    let session_id = format!("s_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let group_id = args
        .get("group_id")
        .and_then(Value::as_str)
        .ok_or("group_id is required")?
        .to_owned();
    let spec = cccc_runtime::LaunchSpec {
        group_id: group_id.clone(),
        actor_id: session_id.clone(),
        runner: RunnerKind::Headless,
        command: super::local_tools::command(args)?,
        cwd: super::local_tools::command_cwd(root, args)?,
        env: super::local_tools::command_env(args)?,
        cols: 120,
        rows: 40,
    };
    let session = {
        let mut owned = sessions().lock().map_err(|_| "session lock poisoned")?;
        if owned.values().filter(|s| s.home == home.root()).count() >= MAX_SESSIONS_PER_HOME {
            return Err(
                "local command session limit reached; finish or terminate an existing session"
                    .into(),
            );
        }
        // Registration and expiration are installed before the first await, so
        // cancelling a start request cannot leave an unowned, unbounded child.
        let session = Arc::new(LocalSession {
            home: home.root().to_owned(),
            group_id,
            command: CommandSession::start(spec).map_err(|e| e.to_string())?,
            io: tokio::sync::Mutex::new(()),
            cursor: Mutex::new(0),
            closed: AtomicBool::new(false),
            timed_out: AtomicBool::new(false),
            cleanup_error: Mutex::new(None),
        });
        owned.insert(session_id.clone(), Arc::clone(&session));
        schedule_expiration(session_id.clone(), &session, timeout);
        session
    };
    poll(&session_id, session, wait, limit, None, false).await
}

pub async fn write(home: &HomeLayout, args: &Map<String, Value>) -> Result<Value, String> {
    let wait = wait_time(args)?;
    let limit = output_limit(args)?;
    let (id, session) = session(home, args)?;
    let chars = args
        .get("chars")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let terminate = args
        .get("terminate")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    poll(&id, session, wait, limit, chars, terminate).await
}

async fn poll(
    id: &str,
    session: Arc<LocalSession>,
    wait: Duration,
    limit: usize,
    chars: Option<String>,
    terminate: bool,
) -> Result<Value, String> {
    // Stop must not queue behind a long poll or blocked stdin write.
    if terminate {
        let command = session.command.clone();
        blocking(move || command.stop().map_err(|e| e.to_string())).await?;
        *session
            .cleanup_error
            .lock()
            .map_err(|_| "session lock poisoned")? = None;
    }
    let _io = session.io.lock().await;
    if session.closed.load(Ordering::Acquire) {
        if terminate {
            let command = session.command.clone();
            let status = blocking(move || command.status().map_err(|e| e.to_string())).await?;
            return Ok(
                json!({"session_id":id,"status":status,"output":"","closed":true,
                "cursor":*session.cursor.lock().map_err(|_| "session lock poisoned")?,
                "has_more":false,"cursor_expired":false,"timed_out":session.timed_out.load(Ordering::Acquire)}),
            );
        }
        return Err("session is closed".into());
    }
    if !terminate && let Some(chars) = chars {
        let command = session.command.clone();
        blocking(move || command.write(chars.as_bytes()).map_err(|e| e.to_string())).await?;
    }
    let deadline = Instant::now() + wait;
    loop {
        if let Some(error) = session
            .cleanup_error
            .lock()
            .map_err(|_| "session lock poisoned")?
            .as_ref()
        {
            return Err(format!("command cleanup failed: {error}"));
        }
        let cursor = *session.cursor.lock().map_err(|_| "session lock poisoned")?;
        let command = session.command.clone();
        let (status, page) = blocking(move || {
            let status = command.status().map_err(|e| e.to_string())?;
            let page = command
                .history_since(cursor, limit)
                .map_err(|e| e.to_string())?;
            Ok((status, page))
        })
        .await?;
        if !page.data.is_empty() || !status.running || Instant::now() >= deadline {
            *session.cursor.lock().map_err(|_| "session lock poisoned")? = page.end_cursor;
            let closed = !status.running && !page.has_more;
            if closed {
                session.closed.store(true, Ordering::Release);
                remove_session(id)?;
            }
            return Ok(
                json!({"session_id":id,"status":status,"output":page.data,"cursor":page.end_cursor,
                "has_more":page.has_more,"cursor_expired":page.cursor_expired,"closed":closed,
                "timed_out":session.timed_out.load(Ordering::Acquire)}),
            );
        }
        tokio::time::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(25)),
        )
        .await;
    }
}

fn schedule_expiration(id: String, session: &Arc<LocalSession>, timeout: Duration) {
    let weak = Arc::downgrade(session);
    tokio::spawn(async move {
        tokio::time::sleep(timeout).await;
        let Some(session) = weak.upgrade() else {
            return;
        };
        let result = blocking(move || {
            if session.command.status().map_err(|e| e.to_string())?.running {
                session.timed_out.store(true, Ordering::Release);
            }
            session.command.stop().map_err(|e| e.to_string())?;
            Ok(())
        })
        .await;
        if let Some(session) = weak.upgrade() {
            if let Err(error) = result {
                if let Ok(mut stored) = session.cleanup_error.lock() {
                    *stored = Some(error);
                }
            }
        }
        // Bound abandoned completed results; no polling creates new runtime work.
        tokio::time::sleep(FINISHED_RETENTION).await;
        if let Some(session) = weak.upgrade() {
            session.closed.store(true, Ordering::Release);
            let _ = remove_session(&id);
        }
    });
}

fn remove_session(id: &str) -> Result<(), String> {
    sessions()
        .lock()
        .map_err(|_| "session lock poisoned")?
        .remove(id);
    Ok(())
}

fn session(
    home: &HomeLayout,
    args: &Map<String, Value>,
) -> Result<(String, Arc<LocalSession>), String> {
    let id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or("session_id is required")?
        .to_owned();
    let owner = sessions()
        .lock()
        .map_err(|_| "session lock poisoned")?
        .get(&id)
        .cloned()
        .ok_or("session not found")?;
    if owner.home != home.root() {
        return Err("session does not belong to the requested home".into());
    }
    if args
        .get("group_id")
        .and_then(Value::as_str)
        .is_some_and(|group| group != owner.group_id)
    {
        return Err("session does not belong to the requested group".into());
    }
    Ok((id, owner))
}

fn sessions() -> &'static Mutex<Sessions> {
    static SESSIONS: OnceLock<Mutex<Sessions>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn integer(
    args: &Map<String, Value>,
    name: &str,
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64, String> {
    let Some(value) = args.get(name) else {
        return Ok(default);
    };
    value
        .as_u64()
        .filter(|v| (min..=max).contains(v))
        .ok_or_else(|| format!("{name} must be an integer between {min} and {max}"))
}
fn wait_time(args: &Map<String, Value>) -> Result<Duration, String> {
    Ok(Duration::from_millis(integer(
        args,
        "yield_time_ms",
        1000,
        0,
        30000,
    )?))
}
fn output_limit(args: &Map<String, Value>) -> Result<usize, String> {
    Ok(integer(args, "max_output_bytes", 200000, 1, 1000000)? as usize)
}
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

/// End only this host/Home's commands, never Actor or Analyst sessions.
pub fn shutdown(home: &HomeLayout) -> Result<(), String> {
    let owned = {
        let mut sessions = sessions().lock().map_err(|_| "session lock poisoned")?;
        let ids = sessions
            .iter()
            .filter(|(_, s)| s.home == home.root())
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| sessions.remove(&id))
            .collect::<Vec<_>>()
    };
    let mut errors = Vec::new();
    for session in owned {
        session.closed.store(true, Ordering::Release);
        if let Err(error) = session.command.stop() {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(all(test, unix))]
#[path = "local_sessions_tests.rs"]
mod tests;
