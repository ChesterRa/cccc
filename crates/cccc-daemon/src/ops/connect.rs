use super::{
    membership_account::AccountClient,
    operation::{
        Operation,
        Policy::{GlobalWrite, Read},
    },
};
use crate::dispatch::{OpError, OpResult, object};
use cccc_contracts::{DaemonRequest, connect::ConnectRegistration};
use cccc_core::{
    HomeLayout,
    connect::{self, ConnectSnapshot},
    instance_identity::InstanceIdentity,
    membership, settings,
};
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::task::JoinHandle;

#[cfg(test)]
#[path = "connect_tests.rs"]
mod tests;

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "connect_status" => Operation::new(Read, status),
        "connect_rename" => Operation::new(GlobalWrite, rename),
        _ => return None,
    })
}

fn rename(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    // Apply the same user-only boundary as directory status before contacting the account.
    status(home, request)?;
    let name = request
        .args
        .get("display_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim();
    if name.is_empty()
        || name.encode_utf16().count() > 60
        || name.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        return Err(OpError::new(
            "invalid_request",
            "instance name must be 1 to 60 characters without control characters",
        ));
    }
    let current = intent(home).map_err(OpError::io)?.ok_or_else(|| {
        OpError::new(
            "membership_not_logged_in",
            "link this instance before renaming it",
        )
    })?;
    let confirmed = AccountClient::new(&current.origin)
        .and_then(|client| client.rename_device(&current.token, name))
        .map_err(|error| OpError::new(error.code, error.message))?;
    // Name changes are account-owned. A refresh failure must not report an already
    // committed rename as failed; the existing service will refresh the directory.
    let _ = refresh(home, &current, &AtomicBool::new(false));
    object(json!({"display_name":confirmed}))
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    if request
        .args
        .get("by")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("user")
        != "user"
    {
        return Err(OpError::new(
            "permission_denied",
            "Connect workbench status requires user access",
        ));
    }
    let snapshot = connect::load(home).map_err(OpError::io)?;
    object(json!({"connect": snapshot}))
}

#[derive(Clone, PartialEq)]
struct Intent {
    origin: String,
    device_id: String,
    token: String,
    public_origin: Option<String>,
}

fn intent(home: &HomeLayout) -> std::io::Result<Option<Intent>> {
    let state = membership::load(home)?;
    if !state.logged_in || state.disabled {
        return Ok(None);
    }
    let Some(token) = state.device_token.filter(|token| !token.is_empty()) else {
        return Ok(None);
    };
    let Some(device_id) = state.device_id.filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    let Some(origin) = state
        .account_origin
        .map(|origin| membership::canonical_account_origin(&origin))
    else {
        return Ok(None);
    };
    let remote = settings::load(home)?.remote_access;
    let enabled = remote
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let provider = remote
        .get("provider")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("off");
    let public_origin = if !enabled || provider == "off" {
        None
    } else if provider == "reach" {
        state
            .hostname
            .and_then(|hostname| super::membership_account::canonical_reach_hostname(&hostname))
    } else {
        remote
            .get("web_public_url")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| connect::canonical_public_origin(value.trim_end_matches('/')))
    };
    Ok(Some(Intent {
        origin,
        device_id,
        token,
        public_origin,
    }))
}

pub(crate) struct ConnectService {
    cancelled: Arc<AtomicBool>,
    task: JoinHandle<()>,
    peers: JoinHandle<()>,
}

impl ConnectService {
    pub(crate) fn start(
        home: HomeLayout,
        locks: crate::dispatch_concurrency::DispatchLocks,
    ) -> Self {
        let peers = tokio::spawn(crate::connect_transport::run(home.clone(), locks));
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = tokio::spawn(async move {
            let mut previous = None;
            let mut next = tokio::time::Instant::now();
            let mut backoff = 5_u64;
            loop {
                let work_home = home.clone();
                let current = tokio::task::spawn_blocking(move || intent(&work_home)).await;
                if let Ok(Ok(Some(current))) = current {
                    if previous.as_ref() != Some(&current) || tokio::time::Instant::now() >= next {
                        previous = Some(current.clone());
                        let work_home = home.clone();
                        let cancelled = worker_cancelled.clone();
                        let result = tokio::task::spawn_blocking(move || {
                            refresh(&work_home, &current, &cancelled)
                        })
                        .await;
                        let delay = if matches!(result, Ok(Ok(()))) {
                            backoff = 5;
                            60
                        } else {
                            let delay = backoff;
                            backoff = (backoff * 2).min(60);
                            delay
                        };
                        next = tokio::time::Instant::now() + Duration::from_secs(delay);
                    }
                } else {
                    previous = None;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
        Self {
            cancelled,
            task,
            peers,
        }
    }
}

impl Drop for ConnectService {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.task.abort();
        self.peers.abort();
    }
}

fn refresh(home: &HomeLayout, requested: &Intent, cancelled: &AtomicBool) -> std::io::Result<()> {
    if cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let identity = match InstanceIdentity::load_or_create(home) {
        Ok(identity) => identity,
        Err(error) => {
            if cancelled.load(Ordering::Acquire) || intent(home)?.as_ref() != Some(requested) {
                return Ok(());
            }
            connect::save(
                home,
                &ConnectSnapshot {
                    account_origin: requested.origin.clone(),
                    device_id: requested.device_id.clone(),
                    checked_at: cccc_contracts::utc_now(),
                    error_code: Some("connect_identity_error".into()),
                    error_message: Some(format!(
                        "Could not load the persisted instance identity: {error}"
                    )),
                    ..Default::default()
                },
            )?;
            return Err(error);
        }
    };
    let mut registration = ConnectRegistration {
        instance_id: identity.peer_id.clone(),
        public_key: identity.public_key_b64.clone(),
        client_version: env!("CARGO_PKG_VERSION").into(),
        public_origin: requested.public_origin.clone(),
        issued_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        signature: String::new(),
    };
    registration.signature =
        identity.sign(&registration.signing_material(&requested.origin, &requested.device_id))?;
    let result = AccountClient::new(&requested.origin)
        .and_then(|client| client.register_connect(&requested.token, &registration));
    if cancelled.load(Ordering::Acquire) || intent(home)?.as_ref() != Some(requested) {
        return Ok(());
    }
    let mut snapshot = ConnectSnapshot {
        account_origin: requested.origin.clone(),
        device_id: requested.device_id.clone(),
        instance_id: identity.peer_id.clone(),
        checked_at: cccc_contracts::utc_now(),
        ..Default::default()
    };
    let error = match result {
        Ok(directory) => match connect::validate_directory(
            &directory,
            &requested.device_id,
            &identity.peer_id,
            Utc::now(),
        ) {
            Ok(()) => {
                snapshot.directory = Some(directory);
                None
            }
            Err(error) => Some(("connect_invalid_directory".into(), error.to_string())),
        },
        Err(error) => {
            // Only a transient transport problem may retain the unexpired previous grant.
            if matches!(
                error.code,
                "membership_network" | "membership_authorization_pending"
            ) {
                snapshot.directory = connect::load(home)?.and_then(|old| old.directory);
            }
            Some((error.code.to_owned(), error.message))
        }
    };
    if let Some((code, message)) = &error {
        snapshot.error_code = Some(code.clone());
        snapshot.error_message = Some(message.clone());
    }
    connect::save(home, &snapshot)?;
    if error.is_some() {
        Err(std::io::Error::other("Connect account refresh failed"))
    } else {
        Ok(())
    }
}
