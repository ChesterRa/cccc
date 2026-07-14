use crate::RuntimeError;
use crate::output::HistoryPage;
use crate::session::{LaunchSpec, Session, SessionStatus};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

type Key = (String, String);

fn sessions() -> &'static Mutex<HashMap<Key, Session>> {
    static SESSIONS: OnceLock<Mutex<HashMap<Key, Session>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock() -> Result<MutexGuard<'static, HashMap<Key, Session>>, RuntimeError> {
    sessions().lock().map_err(|_| RuntimeError::Poisoned)
}

pub fn start(spec: LaunchSpec) -> Result<SessionStatus, RuntimeError> {
    let key = (spec.group_id.clone(), spec.actor_id.clone());
    let mut sessions = lock()?;
    if let Some(session) = sessions.get_mut(&key)
        && session.status().running
    {
        return Err(RuntimeError::AlreadyRunning(key.0, key.1));
    }
    sessions.remove(&key);
    let mut session = Session::start(spec)?;
    let status = session.status();
    sessions.insert(key, session);
    Ok(status)
}

pub fn status(group_id: &str, actor_id: &str) -> Result<SessionStatus, RuntimeError> {
    let mut sessions = lock()?;
    session(&mut sessions, group_id, actor_id).map(Session::status)
}

pub fn stop(group_id: &str, actor_id: &str) -> Result<SessionStatus, RuntimeError> {
    let key = (group_id.to_owned(), actor_id.to_owned());
    let mut sessions = lock()?;
    let status = session(&mut sessions, group_id, actor_id)?.stop()?;
    sessions.remove(&key);
    Ok(status)
}

pub fn stop_all() -> Result<Vec<SessionStatus>, RuntimeError> {
    let drained = {
        let mut sessions = lock()?;
        std::mem::take(&mut *sessions)
    };
    let mut stopped = Vec::with_capacity(drained.len());
    let mut first_error = None;
    for (_, mut session) in drained {
        match session.stop() {
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
    let mut sessions = lock()?;
    session(&mut sessions, group_id, actor_id)?.write(data)
}

pub fn resize(group_id: &str, actor_id: &str, cols: u16, rows: u16) -> Result<(), RuntimeError> {
    let mut sessions = lock()?;
    session(&mut sessions, group_id, actor_id)?.resize(cols, rows)
}

pub fn history(
    group_id: &str,
    actor_id: &str,
    before: Option<u64>,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    let mut sessions = lock()?;
    session(&mut sessions, group_id, actor_id)?.history(before, limit)
}

pub fn history_since(
    group_id: &str,
    actor_id: &str,
    after: u64,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    let mut sessions = lock()?;
    session(&mut sessions, group_id, actor_id)?.history_since(after, limit)
}

pub fn clear(group_id: &str, actor_id: &str) -> Result<(), RuntimeError> {
    let mut sessions = lock()?;
    session(&mut sessions, group_id, actor_id)?.clear()
}

pub fn bracketed_paste_enabled(group_id: &str, actor_id: &str) -> Result<bool, RuntimeError> {
    let mut sessions = lock()?;
    session(&mut sessions, group_id, actor_id)?.bracketed_paste_enabled()
}

pub fn reap() -> Result<Vec<SessionStatus>, RuntimeError> {
    let mut sessions = lock()?;
    let mut exited = Vec::new();
    sessions.retain(|_, session| {
        let status = session.status();
        if status.running {
            true
        } else {
            exited.push(status);
            false
        }
    });
    Ok(exited)
}

fn session<'a>(
    sessions: &'a mut HashMap<Key, Session>,
    group_id: &str,
    actor_id: &str,
) -> Result<&'a mut Session, RuntimeError> {
    sessions
        .get_mut(&(group_id.to_owned(), actor_id.to_owned()))
        .ok_or_else(|| RuntimeError::NotFound(group_id.into(), actor_id.into()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::{history, start, status, stop, stop_all};
    use crate::LaunchSpec;
    use cccc_contracts::RunnerKind;
    use std::collections::BTreeMap;
    use std::sync::{Mutex, MutexGuard, OnceLock};

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
}
