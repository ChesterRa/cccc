use super::{
    membership_account::AccountClient,
    operation::{
        Operation,
        Policy::{Read, RemoteAccess},
    },
};
use crate::dispatch::{OpError, OpResult, object};
use cccc_contracts::{DaemonRequest, connect::ConnectRegistration};
use cccc_core::{
    HomeLayout,
    connect::{self, ConnectGroupSync, ConnectSnapshot},
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
        "connect_rename" => Operation::new(RemoteAccess, rename),
        "connect_group_status" => Operation::new(Read, group_status),
        "connect_group_select" => Operation::new(Read, group_select),
        _ => return None,
    })
}

fn group_status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let group = super::connect_peer::authorize_local_source(home, request)?;
    let snapshot = connect::load(home).map_err(OpError::io)?;
    let links = cccc_core::connect_groups::load(home).map_err(OpError::io)?;
    let current = intent(home).map_err(OpError::io)?;
    let sync = snapshot.as_ref().and_then(|s| s.group_sync.as_ref());
    let error_code = snapshot
        .as_ref()
        .and_then(|s| s.error_code.as_ref())
        .or_else(|| sync.and_then(|s| s.error_code.as_ref()));
    let error_message = snapshot
        .as_ref()
        .filter(|s| s.error_code.is_some())
        .and_then(|s| s.error_message.as_ref())
        .or_else(|| sync.and_then(|s| s.error_message.as_ref()));
    let state = if current.is_none() {
        "not_linked"
    } else if error_code.is_some() || (sync.is_some() && links.is_none()) {
        "unavailable"
    } else if links.is_some() {
        "ready"
    } else {
        "syncing"
    };
    let external = links
        .as_ref()
        .map(|set| {
            set.links
                .iter()
                .filter(|link| {
                    cccc_core::connect_groups::local_endpoint(set, link).is_some_and(
                        |(local, _)| {
                            local.group_id == group
                                && cccc_core::connect_groups::resource_current(home, local)
                                    .unwrap_or(false)
                        },
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let direct = cccc_core::direct::load(home).map_err(OpError::io)?;
    let direct_routes = external
        .iter()
        .filter_map(|link| {
            let set = links.as_ref()?;
            let (local, remote) = cccc_core::connect_groups::local_endpoint(set, link)?;
            direct
                .relations
                .iter()
                .any(|r| {
                    r.local.group_id == local.group_id
                        && r.remote.as_ref().is_some_and(|p| {
                            p.instance_id == remote.instance.instance_id
                                && p.group_id == remote.group_id
                        })
                })
                .then_some(&link.id)
        })
        .collect::<Vec<_>>();
    object(
        json!({"status":state,"error_code":error_code,"error_message":error_message,
        "checked_at":sync.map(|s|&s.checked_at),
        "links":external,"direct_routes":direct_routes,"expires_at":links.as_ref().map(|l|&l.expires_at),
        "account_origin":current.as_ref().map(|s|&s.origin),
        "account_id":snapshot.as_ref().and_then(|s|s.directory.as_ref()).map(|d|&d.account_id)}),
    )
}

fn group_select(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let group = super::connect_peer::authorize_local_source(home, request)?;
    let ticket = cccc_core::connect_groups::ticket(home, &group)
        .map_err(|e| OpError::new("connect_group_unavailable", e.to_string()))?;
    let mut url = url::Url::parse(&format!("{}/connect/select", ticket.account_origin))
        .map_err(OpError::invalid)?;
    use base64::Engine;
    url.query_pairs_mut().append_pair(
        "ticket",
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&ticket).map_err(OpError::invalid)?),
    );
    if let Some(id) = request
        .args
        .get("invitation")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
    {
        uuid::Uuid::parse_str(id).map_err(OpError::invalid)?;
        url.query_pairs_mut().append_pair("invitation", id);
    }
    object(json!({"url":url.as_str()}))
}

fn rename(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    // Apply the same user-only boundary as directory status before contacting the account.
    require_user(request)?;
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

fn require_user(request: &DaemonRequest) -> Result<(), OpError> {
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
    Ok(())
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let snapshot = connect::load(home).map_err(OpError::io)?;
    let member = membership::load(home).map_err(OpError::io)?;
    object(
        json!({"connect": snapshot, "account_label": member.account_label.filter(|_| member.logged_in && !member.disabled),
        "group_connections": group_connection_summary(home).unwrap_or(serde_json::Value::Null)}),
    )
}

// A bounded display projection from the existing expiring grant; it grants no access.
fn group_connection_summary(home: &HomeLayout) -> Result<serde_json::Value, OpError> {
    let links = cccc_core::connect_groups::load(home).map_err(OpError::io)?;
    let direct = cccc_core::direct::load(home).map_err(OpError::io)?;
    let mut counts = std::collections::BTreeMap::<String, Option<usize>>::new();
    let mut resources = std::collections::HashMap::new();
    for (links, link) in links
        .iter()
        .flat_map(|links| links.links.iter().map(move |link| (links, link)))
    {
        if let Some((local, remote)) = cccc_core::connect_groups::local_endpoint(links, link) {
            if direct.relations.iter().any(|r| {
                r.local.group_id == local.group_id
                    && r.remote.as_ref().is_some_and(|p| {
                        p.instance_id == remote.instance.instance_id
                            && p.group_id == remote.group_id
                    })
            }) {
                continue;
            }
            let current = resources
                .entry((local.group_id.clone(), local.group_generation.clone()))
                .or_insert_with(|| cccc_core::connect_groups::resource_current(home, local).ok());
            match current {
                Some(true) => {
                    let count = counts.entry(local.group_id.clone()).or_insert(Some(0));
                    if let Some(value) = count {
                        *value += 1;
                    }
                }
                None => {
                    counts.insert(local.group_id.clone(), None);
                }
                Some(false) => {}
            }
        }
    }
    for relation in direct.relations {
        if let Some(remote) = &relation.remote
            && cccc_core::direct::binding(home, &remote.instance_id, &relation.id).is_ok()
        {
            if let Some(count) = counts.entry(relation.local.group_id).or_insert(Some(0)) {
                *count += 1;
            }
        }
    }
    if counts.is_empty() && links.is_none() {
        return Ok(serde_json::Value::Null);
    }
    let expiry = links
        .map(|links| links.expires_at)
        .unwrap_or_else(|| (Utc::now() + chrono::Duration::seconds(120)).to_rfc3339());
    Ok(json!({"counts": counts, "expires_at": expiry}))
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

fn update_account_label(
    home: &HomeLayout,
    requested: &Intent,
    label: Option<String>,
) -> std::io::Result<()> {
    if membership::load(home)?.account_label == label {
        return Ok(());
    }
    membership::update(home, |current| {
        if current.logged_in
            && !current.disabled
            && current.device_token.as_deref() == Some(&requested.token)
            && current.device_id.as_deref() == Some(&requested.device_id)
            && current
                .account_origin
                .as_deref()
                .map(membership::canonical_account_origin)
                .as_deref()
                == Some(&requested.origin)
        {
            current.account_label = label;
        }
        Ok(())
    })
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
        group_sync: connect::load(home)?.and_then(|old| old.group_sync),
        ..Default::default()
    };
    let error = match result {
        Ok((directory, account_label)) => match connect::validate_directory(
            &directory,
            &requested.device_id,
            &identity.peer_id,
            Utc::now(),
        ) {
            Ok(()) => {
                update_account_label(home, requested, account_label)?;
                snapshot.directory = Some(directory);
                None
            }
            Err(error) => Some(("connect_invalid_directory".into(), error.to_string())),
        },
        Err(error) => {
            if matches!(
                error.code,
                "membership_disabled" | "membership_not_logged_in"
            ) {
                update_account_label(home, requested, None)?;
            }
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
    if error.is_none() && !cancelled.load(Ordering::Acquire) {
        // Optional extension: a server without Group sharing must not break the account directory.
        // Save validates the current membership again after network work, including rebinding races.
        let group_result = (|| -> Result<(), (String, String)> {
            let mut resource_error = None;
            let invalidated = cccc_core::connect_groups::load(home)
                .map_err(|e| ("connect_groups_cache_error".into(), e.to_string()))?
                .map(|links| {
                    links
                        .links
                        .iter()
                        .filter_map(|link| {
                            cccc_core::connect_groups::local_endpoint(&links, link).and_then(
                                |(local, _)| {
                                    match cccc_core::connect_groups::resource_current(home, local) {
                                        Ok(false) => Some(link.id.clone()),
                                        Ok(true) => None,
                                        Err(_) => {
                                            resource_error.get_or_insert_with(|| format!(
                                                "Could not read local Group {}; restore its configuration to resume sharing", local.group_id
                                            ));
                                            None
                                        }
                                    }
                                },
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let links = AccountClient::new(&requested.origin)
                .and_then(|client| client.connect_groups(&requested.token, &invalidated))
                .map_err(|e| (e.code.to_owned(), e.message))?;
            if cancelled.load(Ordering::Acquire)
                || intent(home)
                    .map_err(|e| ("connect_groups_cache_error".into(), e.to_string()))?
                    .as_ref()
                    != Some(requested)
            {
                return Ok(());
            }
            cccc_core::connect_groups::save(home, &links)
                .map_err(|e| ("connect_groups_cache_error".into(), e.to_string()))?;
            // A bad local resource cannot revoke its grant or stop healthy Groups renewing.
            match resource_error {
                Some(message) => Err(("connect_group_resource_unavailable".into(), message)),
                None => Ok(()),
            }
        })();
        if cancelled.load(Ordering::Acquire) || intent(home)?.as_ref() != Some(requested) {
            return Ok(());
        }
        let (error_code, error_message) = match group_result {
            Ok(()) => (None, None),
            Err((code, message)) => {
                if matches!(
                    code.as_str(),
                    "membership_disabled" | "membership_not_logged_in"
                ) {
                    update_account_label(home, requested, None)?;
                }
                (Some(code), Some(message))
            }
        };
        // A concurrent rename/refresh may already have installed a newer directory.
        // Sharing diagnostics must not overwrite that account snapshot with this one.
        let Some(mut latest) = connect::load(home)? else {
            return Ok(());
        };
        if latest.checked_at != snapshot.checked_at {
            return Ok(());
        }
        latest.group_sync = Some(ConnectGroupSync {
            checked_at: cccc_contracts::utc_now(),
            error_code,
            error_message,
        });
        connect::save(home, &latest)?;
    }
    if error.is_some() {
        Err(std::io::Error::other("Connect account refresh failed"))
    } else {
        Ok(())
    }
}
