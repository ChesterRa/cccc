//! Receive leases are ephemeral. Current conversation identity comes from the
//! native lifecycle adapter's CCCC-owned metadata, including while the daemon is down.
use crate::dispatch::OpError;
use cccc_contracts::local_session::{LocalSessionRuntime, LocalSessionSource};
use cccc_core::{HomeLayout, local_sessions as native};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const LEASE: Duration = Duration::from_secs(30);
type Key = (PathBuf, String);

#[derive(Clone)]
pub(super) struct Receiver {
    pub source_id: String,
    pub runtime: LocalSessionRuntime,
    pub native_session_id: String,
    pub connection_id: String,
    pub cwd: String,
    pub title: String,
    pub revision: String,
}

impl Receiver {
    pub fn source_id(&self) -> String {
        self.source_id.clone()
    }
    pub fn source(&self) -> LocalSessionSource {
        LocalSessionSource {
            source_id: self.source_id.clone(),
            runtime: self.runtime,
            native_session_id: self.native_session_id.clone(),
            cwd: self.cwd.clone(),
            title: self.title.clone(),
            origin: None,
            connected: true,
        }
    }
}

#[derive(Clone)]
struct Lease {
    connection_id: String,
    at: Instant,
}
fn leases() -> &'static Mutex<HashMap<Key, Lease>> {
    static LEASES: OnceLock<Mutex<HashMap<Key, Lease>>> = OnceLock::new();
    LEASES.get_or_init(Mutex::default)
}

fn metadata(home: &HomeLayout, source_id: &str) -> Result<native::NativeSession, OpError> {
    let session = native::read(home, source_id).map_err(OpError::io)?;
    if !session.host_alive() {
        return Err(OpError::new(
            "local_session_closed",
            "The native session host has exited",
        ));
    }
    Ok(session)
}

fn make_receiver(session: native::NativeSession, connection_id: &str) -> Receiver {
    Receiver {
        source_id: session.source_id,
        runtime: session.runtime,
        native_session_id: session.native_session_id,
        connection_id: connection_id.into(),
        cwd: session.cwd,
        title: session.title,
        revision: session.revision,
    }
}

pub(super) fn attach(
    home: &HomeLayout,
    source_id: &str,
    connection_id: &str,
) -> Result<Receiver, OpError> {
    let session = metadata(home, source_id)?;
    if session.runtime == LocalSessionRuntime::Codex {
        return Err(OpError::new(
            "invalid_args",
            "Codex uses its official native queue rather than a receive lease",
        ));
    }
    let mut leases = leases().lock().map_err(|_| unavailable())?;
    leases.retain(|_, lease| lease.at.elapsed() < LEASE);
    let key = (home.root().to_owned(), source_id.to_owned());
    if leases
        .get(&key)
        .is_some_and(|lease| lease.connection_id != connection_id && lease.at.elapsed() < LEASE)
    {
        return Err(OpError::new(
            "local_session_receiver_conflict",
            "This native session already has a receiver",
        ));
    }
    leases.insert(
        key,
        Lease {
            connection_id: connection_id.into(),
            at: Instant::now(),
        },
    );
    Ok(make_receiver(session, connection_id))
}

pub(super) fn heartbeat(
    home: &HomeLayout,
    source_id: &str,
    connection_id: &str,
) -> Result<Receiver, OpError> {
    // Reconnect after daemon restart by the same physical receiver. The native
    // callback file supplies the current ID, never its old startup environment.
    attach(home, source_id, connection_id)
}

pub(super) fn receiver(
    home: &HomeLayout,
    source_id: &str,
    connection_id: &str,
) -> Result<Receiver, OpError> {
    let session = metadata(home, source_id)?;
    let leases = leases().lock().map_err(|_| unavailable())?;
    if !leases
        .get(&(home.root().to_owned(), source_id.into()))
        .is_some_and(|lease| lease.connection_id == connection_id && lease.at.elapsed() < LEASE)
    {
        return Err(OpError::new(
            "local_session_closed",
            "The receiver lease is no longer active",
        ));
    }
    Ok(make_receiver(session, connection_id))
}

pub(super) fn by_source(home: &HomeLayout, source_id: &str) -> Result<Option<Receiver>, OpError> {
    let session = match metadata(home, source_id) {
        Ok(session) => session,
        Err(_) => return Ok(None),
    };
    let leases = leases().lock().map_err(|_| unavailable())?;
    Ok(leases
        .get(&(home.root().to_owned(), source_id.into()))
        .filter(|lease| lease.at.elapsed() < LEASE)
        .map(|lease| make_receiver(session, &lease.connection_id)))
}

pub(super) fn sources(home: &HomeLayout) -> Result<Vec<LocalSessionSource>, OpError> {
    let (sessions, errors) = native::scan(home).map_err(OpError::io)?;
    for error in errors {
        tracing::warn!(%error,"Native session metadata is unavailable");
    }
    let leases = leases().lock().map_err(|_| unavailable())?;
    Ok(sessions
        .into_iter()
        .map(|session| {
            let connected = match session.runtime {
                LocalSessionRuntime::Codex => session.codex_can_receive,
                LocalSessionRuntime::Claude
                | LocalSessionRuntime::Pi
                | LocalSessionRuntime::Opencode => leases
                    .get(&(home.root().to_owned(), session.source_id.clone()))
                    .is_some_and(|lease| lease.at.elapsed() < LEASE),
            };
            session.source(connected)
        })
        .collect())
}

pub(super) fn connected(home: &HomeLayout, source_id: &str) -> bool {
    let Ok(session) = metadata(home, source_id) else {
        return false;
    };
    match session.runtime {
        LocalSessionRuntime::Codex => session.codex_can_receive,
        LocalSessionRuntime::Claude | LocalSessionRuntime::Pi | LocalSessionRuntime::Opencode => {
            by_source(home, source_id).is_ok_and(|value| value.is_some())
        }
    }
}

pub(super) fn detach(
    home: &HomeLayout,
    source_id: &str,
    connection_id: &str,
) -> Result<(), OpError> {
    let mut leases = leases().lock().map_err(|_| unavailable())?;
    let key = (home.root().to_owned(), source_id.into());
    if leases
        .get(&key)
        .is_some_and(|lease| lease.connection_id == connection_id)
    {
        leases.remove(&key);
    }
    drop(leases);
    super::transport::detach(home, source_id, connection_id)
}

fn unavailable() -> OpError {
    OpError::new(
        "local_session_unavailable",
        "Receiver registry is unavailable",
    )
}
