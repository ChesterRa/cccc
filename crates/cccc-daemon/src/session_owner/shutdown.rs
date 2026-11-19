use super::{AgentIdentity, ShutdownMode};
use cccc_core::HomeLayout;
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

#[derive(Clone)]
pub(crate) struct ShutdownRequest {
    pub mode: ShutdownMode,
    pub request_id: String,
}

struct State {
    selected: Option<ShutdownRequest>,
    implicit_stop: bool,
    retained: Vec<AgentIdentity>,
}

fn states() -> &'static Mutex<BTreeMap<PathBuf, State>> {
    static STATES: OnceLock<Mutex<BTreeMap<PathBuf, State>>> = OnceLock::new();
    STATES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Called with the exclusive home lock, before publishing this daemon's PID.
/// Clearing the previous receipt prevents a reused PID from reading an old exit.
pub(crate) fn prepare_shutdown(home: &HomeLayout, retained: Vec<AgentIdentity>) -> io::Result<()> {
    super::receipt::clear_shutdown_report(home)?;
    states()
        .lock()
        .map_err(|_| io::Error::other("shutdown state poisoned"))?
        .insert(
            home.root().to_path_buf(),
            State {
                selected: None,
                implicit_stop: false,
                retained,
            },
        );
    Ok(())
}

pub(crate) fn select_shutdown(
    home: &HomeLayout,
    mode: ShutdownMode,
    request_id: String,
) -> io::Result<ShutdownRequest> {
    let mut states = states()
        .lock()
        .map_err(|_| io::Error::other("shutdown state poisoned"))?;
    let selected = ShutdownRequest { mode, request_id };
    if let Some(state) = states.get_mut(home.root()) {
        // A host preselects Stop before its IPC request can wait in the queue.
        // That request may bind its own receipt ID once; it cannot change the
        // intent to maintenance or supersede another explicit request.
        if state.selected.is_some() && !(state.implicit_stop && mode == ShutdownMode::Stop) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "daemon shutdown is already in progress",
            ));
        }
        state.selected = Some(selected.clone());
        state.implicit_stop = false;
    }
    Ok(selected)
}

pub(crate) fn selected_shutdown(home: &HomeLayout) -> io::Result<Option<ShutdownRequest>> {
    Ok(states()
        .lock()
        .map_err(|_| io::Error::other("shutdown state poisoned"))?
        .get(home.root())
        .and_then(|state| state.selected.clone()))
}

/// A normal signal/watch-channel exit is Stop unless an explicit maintenance
/// request was already selected. A cancelled host without this step preserves
/// remote resources instead of mistaking a failure for user stop intent.
pub(crate) fn begin_shutdown(home: &HomeLayout) {
    if let Ok(mut states) = states().lock()
        && let Some(state) = states.get_mut(home.root())
        && state.selected.is_none()
    {
        state.selected = Some(ShutdownRequest {
            mode: ShutdownMode::Stop,
            request_id: uuid::Uuid::new_v4().simple().to_string(),
        });
        state.implicit_stop = true;
    }
}

pub(crate) fn shutdown_request(home: &HomeLayout) -> ShutdownRequest {
    states()
        .lock()
        .ok()
        .and_then(|states| {
            states
                .get(home.root())
                .and_then(|state| state.selected.clone())
        })
        .unwrap_or_else(|| ShutdownRequest {
            mode: ShutdownMode::PreserveAgents,
            request_id: uuid::Uuid::new_v4().simple().to_string(),
        })
}

/// Only an explicitly selected maintenance exit keeps delivery claims open.
/// The lifecycle's conservative fallback on an unexpected Drop is not intent.
pub(crate) fn maintenance_selected(home: &HomeLayout) -> bool {
    states().lock().is_ok_and(|states| {
        states
            .get(home.root())
            .and_then(|state| state.selected.as_ref())
            .is_some_and(|request| request.mode == ShutdownMode::PreserveAgents)
    })
}

pub fn startup_retained(home: &HomeLayout) -> Option<Vec<AgentIdentity>> {
    states()
        .lock()
        .ok()?
        .get(home.root())
        .map(|state| state.retained.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_stop_can_bind_one_explicit_stop_request_without_becoming_maintenance() {
        let temp = tempfile::tempdir().expect("isolated test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("isolated home");
        home.initialize().expect("initialize home");
        prepare_shutdown(&home, Vec::new()).expect("prepare shutdown");
        crate::session_owner::begin_owned_stop(&home);
        assert_eq!(shutdown_request(&home).mode, ShutdownMode::Stop);
        assert!(select_shutdown(&home, ShutdownMode::PreserveAgents, "keep".into()).is_err());
        let request = select_shutdown(&home, ShutdownMode::Stop, "host-stop".into())
            .expect("bind host stop receipt ID");
        assert_eq!(request.request_id, "host-stop");
        assert_eq!(request.mode, ShutdownMode::Stop);
        assert!(select_shutdown(&home, ShutdownMode::Stop, "second-stop".into()).is_err());
        assert_eq!(shutdown_request(&home).request_id, "host-stop");

        prepare_shutdown(&home, Vec::new()).expect("prepare another shutdown");
        select_shutdown(&home, ShutdownMode::PreserveAgents, "maintenance".into())
            .expect("select maintenance");
        crate::session_owner::begin_owned_stop(&home);
        assert!(maintenance_selected(&home));
        assert_eq!(shutdown_request(&home).request_id, "maintenance");
    }

    #[test]
    fn an_unselected_exit_fallback_does_not_make_member_stops_maintenance() {
        let temp = tempfile::tempdir().expect("isolated test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("resolve isolated home");
        home.initialize().expect("initialize fixture state");
        prepare_shutdown(&home, Vec::new()).expect("prepare shutdown state");
        assert_eq!(shutdown_request(&home).mode, ShutdownMode::PreserveAgents);
        assert!(!maintenance_selected(&home));
        select_shutdown(&home, ShutdownMode::PreserveAgents, "maintenance".into())
            .expect("select maintenance intent");
        assert!(maintenance_selected(&home));
    }
}
