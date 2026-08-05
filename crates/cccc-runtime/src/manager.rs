use crate::RuntimeError;
use crate::cancellation::wait_interruptibly;
use crate::output::HistoryPage;
use crate::session::{LaunchSpec, Session, SessionStatus};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

type Key = (String, String);

type SharedSession = Arc<Mutex<Session>>;

fn sessions() -> &'static RwLock<HashMap<Key, SharedSession>> {
    static SESSIONS: OnceLock<RwLock<HashMap<Key, SharedSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn lookup(group_id: &str, actor_id: &str) -> Result<SharedSession, RuntimeError> {
    sessions()
        .read()
        .map_err(|_| RuntimeError::Poisoned)?
        .get(&(group_id.to_owned(), actor_id.to_owned()))
        .cloned()
        .ok_or_else(|| RuntimeError::NotFound(group_id.into(), actor_id.into()))
}

fn with_session<T>(
    group_id: &str,
    actor_id: &str,
    operation: impl FnOnce(&mut Session) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    let session = lookup(group_id, actor_id)?;
    let mut session = session.lock().map_err(|_| RuntimeError::Poisoned)?;
    operation(&mut session)
}

pub fn start(spec: LaunchSpec) -> Result<SessionStatus, RuntimeError> {
    let key = (spec.group_id.clone(), spec.actor_id.clone());
    remove_exited_before_start(&key)?;
    let mut session = Session::start(spec)?;
    let status = session.status();
    let mut registry = sessions().write().map_err(|_| RuntimeError::Poisoned)?;
    if registry.contains_key(&key) {
        drop(registry);
        session.stop()?;
        return Err(RuntimeError::AlreadyRunning(key.0, key.1));
    }
    registry.insert(key, Arc::new(Mutex::new(session)));
    Ok(status)
}

fn remove_exited_before_start(key: &Key) -> Result<(), RuntimeError> {
    let Some(existing) = sessions()
        .read()
        .map_err(|_| RuntimeError::Poisoned)?
        .get(key)
        .cloned()
    else {
        return Ok(());
    };
    let running = {
        let mut session = existing.lock().map_err(|_| RuntimeError::Poisoned)?;
        session.status().running
    };
    if running {
        return Err(RuntimeError::AlreadyRunning(key.0.clone(), key.1.clone()));
    }
    let mut registry = sessions().write().map_err(|_| RuntimeError::Poisoned)?;
    if registry
        .get(key)
        .is_some_and(|registered| Arc::ptr_eq(registered, &existing))
    {
        registry.remove(key);
    }
    Ok(())
}

pub fn status(group_id: &str, actor_id: &str) -> Result<SessionStatus, RuntimeError> {
    with_session(group_id, actor_id, |session| Ok(session.status()))
}

pub fn stop(group_id: &str, actor_id: &str) -> Result<SessionStatus, RuntimeError> {
    let key = (group_id.to_owned(), actor_id.to_owned());
    let session = sessions()
        .write()
        .map_err(|_| RuntimeError::Poisoned)?
        .remove(&key)
        .ok_or_else(|| RuntimeError::NotFound(group_id.into(), actor_id.into()))?;
    let mut session = session.lock().map_err(|_| RuntimeError::Poisoned)?;
    session.stop()
}

pub fn stop_if_started_at(
    group_id: &str,
    actor_id: &str,
    expected_started_at: &str,
) -> Result<Option<SessionStatus>, RuntimeError> {
    let key = (group_id.to_owned(), actor_id.to_owned());
    let Ok(current) = lookup(group_id, actor_id) else {
        return Ok(None);
    };
    let mut session = current.lock().map_err(|_| RuntimeError::Poisoned)?;
    if session.status().started_at != expected_started_at {
        return Ok(None);
    }
    let status = session.stop()?;
    drop(session);
    let mut registry = sessions().write().map_err(|_| RuntimeError::Poisoned)?;
    if registry
        .get(&key)
        .is_some_and(|registered| Arc::ptr_eq(registered, &current))
    {
        registry.remove(&key);
    }
    Ok(Some(status))
}

pub fn stop_all() -> Result<Vec<SessionStatus>, RuntimeError> {
    let drained = {
        let mut sessions = sessions().write().map_err(|_| RuntimeError::Poisoned)?;
        std::mem::take(&mut *sessions)
    };
    let mut stopped = Vec::with_capacity(drained.len());
    let mut first_error = None;
    for (_, session) in drained {
        let result = session.lock().map_err(|_| RuntimeError::Poisoned)?.stop();
        match result {
            Ok(status) => stopped.push(status),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(stopped),
    }
}

pub fn write(group_id: &str, actor_id: &str, data: &[u8]) -> Result<(), RuntimeError> {
    let gate = input_gate(group_id, actor_id)?;
    let _guard = gate.lock().map_err(|_| RuntimeError::Poisoned)?;
    write_locked(group_id, actor_id, data)
}

pub fn submit(
    group_id: &str,
    actor_id: &str,
    payload: &[u8],
    submit: &[u8],
    delay: Duration,
) -> Result<(), RuntimeError> {
    let cancelled = AtomicBool::new(false);
    submit_interruptible(group_id, actor_id, payload, submit, delay, &cancelled).map(|_| ())
}

pub fn submit_interruptible(
    group_id: &str,
    actor_id: &str,
    payload: &[u8],
    submit: &[u8],
    delay: Duration,
    cancelled: &AtomicBool,
) -> Result<bool, RuntimeError> {
    let submits = [submit];
    submit_sequence_interruptible(
        group_id,
        actor_id,
        payload,
        &submits,
        delay,
        Duration::ZERO,
        cancelled,
    )
}

pub fn submit_sequence_interruptible(
    group_id: &str,
    actor_id: &str,
    payload: &[u8],
    submits: &[&[u8]],
    initial_delay: Duration,
    repeat_delay: Duration,
    cancelled: &AtomicBool,
) -> Result<bool, RuntimeError> {
    if cancelled.load(Ordering::Acquire) {
        return Ok(false);
    }
    let gate = input_gate(group_id, actor_id)?;
    let _guard = gate.lock().map_err(|_| RuntimeError::Poisoned)?;
    if cancelled.load(Ordering::Acquire) {
        return Ok(false);
    }
    write_locked(group_id, actor_id, payload)?;
    for (index, submit) in submits
        .iter()
        .filter(|submit| !submit.is_empty())
        .enumerate()
    {
        let delay = if index == 0 {
            initial_delay
        } else {
            repeat_delay
        };
        if !wait_interruptibly(delay, cancelled) {
            return Ok(false);
        }
        write_locked(group_id, actor_id, submit)?;
    }
    Ok(true)
}

fn input_gate(
    group_id: &str,
    actor_id: &str,
) -> Result<std::sync::Arc<std::sync::Mutex<()>>, RuntimeError> {
    with_session(group_id, actor_id, |session| Ok(session.input_gate()))
}

fn write_locked(group_id: &str, actor_id: &str, data: &[u8]) -> Result<(), RuntimeError> {
    with_session(group_id, actor_id, |session| session.write(data))
}

pub fn resize(group_id: &str, actor_id: &str, cols: u16, rows: u16) -> Result<(), RuntimeError> {
    with_session(group_id, actor_id, |session| session.resize(cols, rows))
}

pub fn history(
    group_id: &str,
    actor_id: &str,
    before: Option<u64>,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    with_session(group_id, actor_id, |session| session.history(before, limit))
}

pub fn retained_history(group_id: &str, actor_id: &str) -> Result<HistoryPage, RuntimeError> {
    let output = with_session(group_id, actor_id, |session| Ok(session.output_handle()))?;
    let page = output
        .lock()
        .map_err(|_| RuntimeError::Poisoned)?
        .retained_page();
    Ok(page)
}

pub fn history_since(
    group_id: &str,
    actor_id: &str,
    after: u64,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    with_session(group_id, actor_id, |session| {
        session.history_since(after, limit)
    })
}

pub fn clear(group_id: &str, actor_id: &str) -> Result<(), RuntimeError> {
    with_session(group_id, actor_id, |session| session.clear())
}

pub fn bracketed_paste_enabled(group_id: &str, actor_id: &str) -> Result<bool, RuntimeError> {
    with_session(group_id, actor_id, |session| {
        session.bracketed_paste_enabled()
    })
}

pub fn reap() -> Result<Vec<SessionStatus>, RuntimeError> {
    let snapshot = sessions()
        .read()
        .map_err(|_| RuntimeError::Poisoned)?
        .iter()
        .map(|(key, session)| (key.clone(), Arc::clone(session)))
        .collect::<Vec<_>>();
    let mut exited = Vec::new();
    let mut remove = Vec::new();
    for (key, session) in snapshot {
        let status = session.lock().map_err(|_| RuntimeError::Poisoned)?.status();
        if !status.running {
            exited.push(status);
            remove.push((key, session));
        }
    }
    if !remove.is_empty() {
        let mut registry = sessions().write().map_err(|_| RuntimeError::Poisoned)?;
        for (key, session) in remove {
            if registry
                .get(&key)
                .is_some_and(|registered| Arc::ptr_eq(registered, &session))
            {
                registry.remove(&key);
            }
        }
    }
    Ok(exited)
}

#[cfg(all(test, unix))]
mod tests {
    use super::{
        history, start, status, stop, stop_all, stop_if_started_at, submit_interruptible,
        submit_sequence_interruptible,
    };
    use crate::LaunchSpec;
    use cccc_contracts::RunnerKind;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use std::time::Duration;

    fn test_guard() -> MutexGuard<'static, ()> {
        static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("test lock")
    }

    #[test]
    fn captures_process_output() {
        let _guard = test_guard();
        let temp = tempfile::tempdir().expect("tempdir");
        start(LaunchSpec {
            group_id: "g_test".into(),
            actor_id: "peer1".into(),
            runner: RunnerKind::Headless,
            command: vec![
                "sh".into(),
                "-c".into(),
                "printf runtime-ready; sleep 1".into(),
            ],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        })
        .expect("start");
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            history("g_test", "peer1", None, 1024)
                .expect("history")
                .data
                .contains("runtime-ready")
        );
        assert!(status("g_test", "peer1").expect("status").running);
        stop("g_test", "peer1").expect("stop");
    }

    #[test]
    fn stop_all_terminates_every_runtime() {
        let _guard = test_guard();
        let temp = tempfile::tempdir().expect("tempdir");
        for actor_id in ["peer1", "peer2"] {
            start(LaunchSpec {
                group_id: "g_stop_all".into(),
                actor_id: actor_id.into(),
                runner: RunnerKind::Headless,
                command: vec!["sh".into(), "-c".into(), "sleep 30".into()],
                cwd: temp.path().into(),
                env: BTreeMap::new(),
                cols: 80,
                rows: 24,
            })
            .expect("start");
        }
        assert_eq!(stop_all().expect("stop all").len(), 2);
        assert!(status("g_stop_all", "peer1").is_err());
        assert!(status("g_stop_all", "peer2").is_err());
    }

    #[test]
    fn conditional_stop_preserves_a_different_session() {
        let _guard = test_guard();
        let temp = tempfile::tempdir().expect("tempdir");
        start(LaunchSpec {
            group_id: "g_conditional_stop".into(),
            actor_id: "peer1".into(),
            runner: RunnerKind::Headless,
            command: vec!["sh".into(), "-c".into(), "sleep 30".into()],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        })
        .expect("start");

        assert!(
            stop_if_started_at("g_conditional_stop", "peer1", "stale-session")
                .expect("conditional stop")
                .is_none()
        );
        assert!(
            status("g_conditional_stop", "peer1")
                .expect("status")
                .running
        );
        stop("g_conditional_stop", "peer1").expect("cleanup");
    }

    #[test]
    fn restarts_a_naturally_exited_session_without_reap() {
        let _guard = test_guard();
        let temp = tempfile::tempdir().expect("tempdir");
        let spec = |command: &str| LaunchSpec {
            group_id: "g_restart_exited".into(),
            actor_id: "peer1".into(),
            runner: RunnerKind::Headless,
            command: vec!["sh".into(), "-c".into(), command.into()],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        };
        start(spec("exit 0")).expect("first start");
        for _ in 0..100 {
            if !status("g_restart_exited", "peer1").expect("status").running {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !status("g_restart_exited", "peer1")
                .expect("exited status")
                .running
        );

        start(spec("sleep 30")).expect("restart without reap");
        stop("g_restart_exited", "peer1").expect("cleanup");
    }

    #[test]
    fn submit_delay_stops_promptly_when_cancelled() {
        let _guard = test_guard();
        let temp = tempfile::tempdir().expect("tempdir");
        start(LaunchSpec {
            group_id: "g_cancel_submit".into(),
            actor_id: "peer1".into(),
            runner: RunnerKind::Headless,
            command: vec!["sh".into(), "-c".into(), "sleep 30".into()],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        })
        .expect("start");
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let started = std::time::Instant::now();
        let worker = std::thread::spawn(move || {
            submit_interruptible(
                "g_cancel_submit",
                "peer1",
                b"echo delayed",
                b"\r",
                Duration::from_secs(5),
                &worker_cancelled,
            )
            .expect("submit")
        });
        std::thread::sleep(Duration::from_millis(50));
        cancelled.store(true, Ordering::Release);

        assert!(!worker.join().expect("join submit"));
        assert!(started.elapsed() < Duration::from_millis(500));
        stop("g_cancel_submit", "peer1").expect("cleanup");
    }

    #[test]
    fn submit_sequence_writes_each_key_in_order() {
        let _guard = test_guard();
        let temp = tempfile::tempdir().expect("tempdir");
        start(LaunchSpec {
            group_id: "g_submit_sequence".into(),
            actor_id: "peer1".into(),
            runner: RunnerKind::Headless,
            command: vec![
                "sh".into(),
                "-c".into(),
                "stty raw -echo; dd bs=1 count=3 2>/dev/null | od -An -t x1".into(),
            ],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        })
        .expect("start");

        assert!(
            submit_sequence_interruptible(
                "g_submit_sequence",
                "peer1",
                b"x",
                &[b"\r", b"\r"],
                Duration::ZERO,
                Duration::ZERO,
                &AtomicBool::new(false),
            )
            .expect("submit sequence")
        );
        for _ in 0..100 {
            if !status("g_submit_sequence", "peer1")
                .expect("status")
                .running
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = history("g_submit_sequence", "peer1", None, 1024)
            .expect("history")
            .data;
        let output_tokens = output.split_ascii_whitespace().collect::<Vec<_>>();
        assert!(
            output_tokens
                .windows(3)
                .any(|tokens| tokens == ["78", "0a", "0a"]),
            "unexpected output: {output:?}"
        );
        stop("g_submit_sequence", "peer1").expect("cleanup");
    }
}
