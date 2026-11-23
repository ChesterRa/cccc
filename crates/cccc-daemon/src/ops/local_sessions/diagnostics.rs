//! Explicit bounded checks. Directory polling only reads this bounded cache.
use super::source_control;
use crate::dispatch::{OpError, OpResult, object, required_arg};
use cccc_contracts::{
    DaemonRequest,
    source_control::{ReceiveState, SourceDiagnostic},
};
use cccc_core::{
    HomeLayout,
    local_sessions::{self, NativeSession},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const TTL: Duration = Duration::from_secs(15);
const CAPACITY: usize = 128;
static SLOT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
struct Entry {
    source: NativeSession,
    diagnostic: SourceDiagnostic,
    at: Instant,
}
type Key = (PathBuf, String);
fn cache() -> &'static Mutex<BTreeMap<Key, Entry>> {
    static CACHE: OnceLock<Mutex<BTreeMap<Key, Entry>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

pub(super) fn cached(home: &HomeLayout, source: &NativeSession) -> SourceDiagnostic {
    if let Ok(cache) = cache().lock()
        && let Some(entry) = cache.get(&(home.root().to_owned(), source.source_id.clone()))
        && entry.source == *source
    {
        return entry.diagnostic.clone();
    }
    SourceDiagnostic {
        state: if source.codex_can_receive && source.host_alive() {
            ReceiveState::Ready
        } else {
            ReceiveState::Unknown
        },
        checked_at: None,
        origin: source
            .origin
            .clone()
            .filter(|origin| matches!(origin.as_str(), "cli" | "vscode")),
    }
}

pub(super) fn recheck(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    source_control::administrator(request)?;
    let id = required_arg(request, "source_id")?;
    source_control::local_source(home, &id)?;
    let source = local_sessions::read(home, &id).map_err(|_| {
        OpError::new(
            "permission_denied",
            "Source is ended, disabled or removed; no provider check was started",
        )
    })?;
    let key = (home.root().to_owned(), id.clone());
    if let Ok(cache) = cache().lock()
        && let Some(entry) = cache.get(&key)
        && entry.source == source
        && entry.at.elapsed() < TTL
    {
        return object(
            json!({"source_id":id,"diagnostic":entry.diagnostic,"check_status":"cached","retry_after_ms":TTL.saturating_sub(entry.at.elapsed()).as_millis() as u64}),
        );
    }
    let Ok(_slot) = SLOT.try_acquire() else {
        return object(
            json!({"source_id":id,"diagnostic":cached(home,&source),"check_status":"busy","retry_after_ms":15000}),
        );
    };
    let state = if source.runtime == cccc_contracts::local_session::LocalSessionRuntime::Codex {
        match (&source.codex_program, &source.codex_home) {
            (Some(program), Some(provider)) => super::codex::runtime()
                .map_err(OpError::io)?
                .block_on(cccc_runtime::native_codex_receive_state(
                    program,
                    provider,
                    std::path::Path::new(&source.cwd),
                    &source.native_session_id,
                )),
            _ => ReceiveState::Unknown,
        }
    } else if super::store::connected(home, &id) {
        ReceiveState::Ready
    } else {
        ReceiveState::Unknown
    };
    if matches!(
        state,
        ReceiveState::Ready | ReceiveState::SharedBackendMissing | ReceiveState::ThreadNotLoaded
    ) && source.runtime == cccc_contracts::local_session::LocalSessionRuntime::Codex
    {
        local_sessions::set_codex_receive_ready(home, &source, state == ReceiveState::Ready)
            .map_err(OpError::io)?;
    }
    let current = local_sessions::read(home, &id)
        .map_err(|_| OpError::new("permission_denied", "Source authority changed during check"))?;
    let mut expected = source.clone();
    expected.codex_can_receive = current.codex_can_receive;
    if current != expected {
        return Err(OpError::new(
            "source_diagnostic_conflict",
            "Source changed during check; read current state before retrying",
        ));
    }
    let diagnostic = SourceDiagnostic {
        state,
        checked_at: Some(cccc_contracts::utc_now()),
        origin: cached(home, &current).origin,
    };
    let mut cache = cache()
        .lock()
        .map_err(|_| OpError::new("source_diagnostic_busy", "Diagnostic cache unavailable"))?;
    if cache.len() >= CAPACITY
        && !cache.contains_key(&key)
        && let Some(old) = cache.keys().next().cloned()
    {
        cache.remove(&old);
    }
    cache.insert(
        key,
        Entry {
            source: current,
            diagnostic: diagnostic.clone(),
            at: Instant::now(),
        },
    );
    object(
        json!({"source_id":id,"diagnostic":diagnostic,"check_status":"checked","retry_after_ms":15000}),
    )
}
