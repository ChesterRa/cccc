use super::client::{self, Client};
use super::protocol::{
    ActorRef, ActorSnapshot, ExecutionFact, Failure, Mutation, OperationId, OperationSource,
    OperationState, PreparedLaunch,
};
use super::receipt::{AgentIdentity, ShutdownMode, ShutdownReport};
use super::transport::Paths;
use cccc_core::HomeLayout;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

type OwnerClients = BTreeMap<String, Arc<Client>>;

struct Controller {
    home: HomeLayout,
    clients: Mutex<OwnerClients>,
    construction: Mutex<()>,
    deliveries: Mutex<BTreeMap<OperationSource, OperationId>>,
    recoveries: Mutex<BTreeMap<(String, String, String, String), String>>,
}

fn controllers() -> &'static Mutex<BTreeMap<PathBuf, Arc<Controller>>> {
    static CONTROLLERS: OnceLock<Mutex<BTreeMap<PathBuf, Arc<Controller>>>> = OnceLock::new();
    CONTROLLERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Discovery and takeover under the home lock. Incompatible or unavailable ownership is a
/// startup failure, never an empty registry that permits duplicate restoration.
pub(super) fn initialize(home: &HomeLayout) -> io::Result<Vec<AgentIdentity>> {
    let clients = discover(home)?;
    let mut retained = Vec::new();
    for client in clients.values() {
        client.take_control()?;
        let observed = client.owner_snapshot()?;
        retained.extend(
            observed
                .actors
                .iter()
                .filter(|snapshot| running(snapshot))
                .map(|snapshot| identity(&snapshot.actor, &client.address.owner_id)),
        );
        // Startup takeover proves resource retention, not that Web has
        // reconnected. Its current-epoch PagesSet supplies that later fact.
        retained.extend(
            observed
                .browsers
                .iter()
                .filter(|browser| {
                    browser.resource.state == super::browser::BrowserResourceState::Ready
                })
                .flat_map(|browser| browser.identities()),
        );
    }
    let controller = Arc::new(Controller {
        home: home.clone(),
        clients: Mutex::new(clients),
        construction: Mutex::new(()),
        deliveries: Mutex::new(BTreeMap::new()),
        recoveries: Mutex::new(BTreeMap::new()),
    });
    controllers()
        .lock()
        .map_err(poisoned)?
        .insert(home.root().to_path_buf(), controller.clone());
    retire_unused_factories(&controller);
    Ok(retained)
}

/// Only a positive Retired reply permits forgetting a now non-default owner.
/// The owner checks its resources and unsettled facts under admission fencing.
fn retire_unused_factory(controller: &Controller, client: &Arc<Client>) {
    let default: io::Result<String> = cccc_core::fs::read_json(
        &controller
            .home
            .daemon_dir()
            .join("session-owners/default.json"),
    );
    if !default.is_ok_and(|default| default != client.address.owner_id) {
        return;
    }
    if let Err(error) = client.retire() {
        tracing::debug!(owner_id = %client.address.owner_id, kind = ?error.kind(), "old session factory still retains resources or unconfirmed retirement");
        return;
    }
    if let Ok(mut clients) = controller.clients.lock()
        && clients
            .get(&client.address.owner_id)
            .is_some_and(|stored| Arc::ptr_eq(stored, client))
    {
        clients.remove(&client.address.owner_id);
    }
}

fn retire_unused_factories(controller: &Controller) {
    let clients = match controller.clients.lock() {
        Ok(clients) => clients.values().cloned().collect::<Vec<_>>(),
        Err(_) => return,
    };
    for client in clients {
        retire_unused_factory(controller, &client);
    }
}

/// A completed Stop or final delivery acknowledgement may release an old factory.
fn retire_after_acknowledgement(client: &Arc<Client>) {
    let controllers = match controllers().lock() {
        Ok(controllers) => controllers.values().cloned().collect::<Vec<_>>(),
        Err(_) => return,
    };
    for controller in controllers {
        let registered = controller.clients.lock().is_ok_and(|clients| {
            clients
                .get(&client.address.owner_id)
                .is_some_and(|stored| Arc::ptr_eq(stored, client))
        });
        if registered {
            retire_unused_factory(&controller, client);
        }
    }
}

pub(super) fn enabled(home: &HomeLayout) -> bool {
    controllers()
        .lock()
        .is_ok_and(|controllers| controllers.contains_key(home.root()))
}

pub(super) fn require_available(
    home: &HomeLayout,
    group: Option<&str>,
    actor: Option<&str>,
) -> io::Result<()> {
    let controller = for_home(home)?;
    let clients = controller
        .clients
        .lock()
        .map_err(poisoned)?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for client in clients {
        for snapshot in client.projected_snapshots()? {
            if group.is_none_or(|group| snapshot.actor.group_id == group)
                && actor.is_none_or(|actor| snapshot.actor.actor_id == actor)
                && let Some(error) = snapshot.observation_error
            {
                return Err(io::Error::other(error));
            }
        }
    }
    Ok(())
}

fn for_home(home: &HomeLayout) -> io::Result<Arc<Controller>> {
    controllers()
        .lock()
        .map_err(poisoned)?
        .get(home.root())
        .cloned()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotConnected,
                "session owner controller is not installed",
            )
        })
}

pub(super) fn clients_for_observation(home: &HomeLayout) -> io::Result<Vec<Arc<Client>>> {
    let controller = for_home(home)?;
    let clients = controller
        .clients
        .lock()
        .map_err(poisoned)?
        .values()
        .cloned()
        .collect();
    Ok(clients)
}

pub(super) fn browser_resources(
    home: &HomeLayout,
) -> io::Result<Vec<super::browser::BrowserResource>> {
    let mut resources = Vec::new();
    let mut profiles = std::collections::BTreeSet::new();
    for client in clients_for_observation(home)? {
        for resource in client.browsers()? {
            if resource.owner_id != client.address.owner_id
                || resource.instance != client.address.instance
                || !profiles.insert(resource.spec.profile.clone())
            {
                return Err(io::Error::other(
                    "browser resource identity is inconsistent or multiply held",
                ));
            }
            resources.push(resource);
        }
    }
    Ok(resources)
}

pub(super) fn ensure_browser(
    home: &HomeLayout,
    spec: super::browser::BrowserLaunchSpec,
) -> io::Result<super::browser::BrowserResource> {
    let controller = for_home(home)?;
    let construction = controller.construction.lock().map_err(poisoned)?;
    if let Some(resource) = browser_resources(home)?
        .into_iter()
        .find(|resource| resource.spec.profile == spec.profile)
    {
        match resource.state {
            super::browser::BrowserResourceState::Ready => {
                return if resource.spec.same_browser(&spec) {
                    Ok(resource)
                } else {
                    Err(io::Error::other(Failure::new(
                        "browser_settings_changed",
                        "the retained browser uses different launch settings; close it explicitly before changing them",
                    )))
                };
            }
            super::browser::BrowserResourceState::Exited => {
                // A confirmed exit is normal. Release the old holder's display
                // and profile lease before choosing the current launch factory.
                mutate_browser(
                    home,
                    &resource.instance,
                    Mutation::BrowserClose {
                        resource_id: resource.resource_id,
                        instance: resource.instance.clone(),
                    },
                )?;
            }
            _ => {
                return Err(io::Error::other(Failure::new(
                    "browser_unconfirmed",
                    resource.error.unwrap_or_else(|| {
                        "the requested browser profile is not yet confirmed".into()
                    }),
                )));
            }
        }
    }
    let (client, created) = current_factory(&controller)?;
    let operation = client.mutate(Mutation::BrowserEnsure { spec })?;
    drop(construction);
    let result = wait_browser_operation(&client, &operation);
    if created {
        retire_unused_factories(&controller);
    }
    serde_json::from_value(result?).map_err(io::Error::other)
}

pub(super) fn mutate_browser(
    home: &HomeLayout,
    instance: &str,
    mutation: Mutation,
) -> io::Result<serde_json::Value> {
    let closing = matches!(mutation, Mutation::BrowserClose { .. });
    let clients = clients_for_observation(home)?;
    // A failed launch remains closeable even when its resource cannot be
    // listed as positively observed. Find its actual owner by instance first.
    let client = clients
        .into_iter()
        .find(|client| client.address.instance == instance)
        .ok_or_else(|| {
            io::Error::other(Failure::new(
                "browser_instance_changed",
                "the browser holder instance is unavailable",
            ))
        })?;
    let operation = client.mutate(mutation)?;
    let result = wait_browser_operation(&client, &operation);
    if closing {
        retire_after_acknowledgement(&client);
    }
    result
}

fn wait_browser_operation(
    client: &Arc<Client>,
    operation: &OperationId,
) -> io::Result<serde_json::Value> {
    let result = client.wait(operation, Duration::from_secs(60));
    if matches!(client.query(operation), Ok(OperationState::Finished { .. })) {
        client.acknowledge(operation)?;
    }
    result
}

fn all_clients() -> io::Result<Vec<Arc<Client>>> {
    let controllers = controllers()
        .lock()
        .map_err(poisoned)?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    for controller in controllers {
        result.extend(
            controller
                .clients
                .lock()
                .map_err(poisoned)?
                .values()
                .cloned(),
        );
    }
    Ok(result)
}

pub(super) fn exited_terminals() -> io::Result<Vec<cccc_runtime::SessionStatus>> {
    let mut result = Vec::new();
    for client in all_clients()? {
        for snapshot in client.snapshots()? {
            if snapshot
                .managed_status
                .as_ref()
                .is_some_and(|status| status.pid.is_none())
            {
                continue;
            }
            if let Some(terminal) = snapshot.terminal.filter(|terminal| !terminal.running) {
                result.push(terminal);
            }
        }
    }
    Ok(result)
}

pub(super) fn provider_exits(home: &HomeLayout, group: &str) -> io::Result<Vec<AgentIdentity>> {
    let controller = for_home(home)?;
    let clients = controller
        .clients
        .lock()
        .map_err(poisoned)?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let mut exits = Vec::new();
    for client in clients {
        exits.extend(
            client
                .snapshots()?
                .iter()
                .filter(|snapshot| snapshot.provider_exited && snapshot.actor.group_id == group)
                .map(|snapshot| identity(&snapshot.actor, &client.address.owner_id)),
        );
    }
    Ok(exits)
}

pub(super) fn snapshot(group: &str, actor: &str) -> io::Result<Option<ActorSnapshot>> {
    let mut found = None;
    for client in all_clients()? {
        for snapshot in client.projected_snapshots()? {
            if snapshot.actor.group_id == group && snapshot.actor.actor_id == actor {
                if let Some(error) = &snapshot.observation_error {
                    return Err(io::Error::other(error.clone()));
                }
                if found.is_some() {
                    return Err(io::Error::other(
                        "member is registered with multiple session owners",
                    ));
                }
                found = Some(snapshot);
            }
        }
    }
    Ok(found)
}

pub(super) fn client_for(actor: &ActorRef) -> io::Result<Arc<Client>> {
    for client in all_clients()? {
        if client.projected_snapshots()?.iter().any(|snapshot| {
            snapshot.actor.group_id == actor.group_id
                && snapshot.actor.actor_id == actor.actor_id
                && snapshot.actor.actor_created_at == actor.actor_created_at
        }) {
            return Ok(client);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "member has no session owner",
    ))
}

/// The caller holds construction through the first mutation admission.
fn current_factory(controller: &Controller) -> io::Result<(Arc<Client>, bool)> {
    let home = &controller.home;
    let default_path = home.daemon_dir().join("session-owners/default.json");
    let mut owner_id: String = match cccc_core::fs::read_json(&default_path) {
        Ok(id) => id,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let id = uuid::Uuid::new_v4().simple().to_string();
            cccc_core::fs::write_json(&default_path, &id)?;
            id
        }
        Err(error) => return Err(error),
    };
    let existing = controller
        .clients
        .lock()
        .map_err(poisoned)?
        .get(&owner_id)
        .cloned();
    let available = match existing {
        Some(client) if client.current_start_factory()? => Some(client),
        Some(_) => {
            // Preserve existing members on their old code; new launches use
            // this execution source. Closed factories also need a replacement.
            owner_id = uuid::Uuid::new_v4().simple().to_string();
            cccc_core::fs::write_json(&default_path, &owner_id)?;
            None
        }
        None => None,
    };
    let created_factory = available.is_none();
    let client = match available {
        Some(client) => client,
        None => {
            let client = Arc::new(client::start(home, &owner_id)?);
            controller
                .clients
                .lock()
                .map_err(poisoned)?
                .insert(owner_id, client.clone());
            client
        }
    };
    Ok((client, created_factory))
}

pub(super) fn start(
    home: &HomeLayout,
    actor: ActorRef,
    launch: PreparedLaunch,
) -> io::Result<ActorSnapshot> {
    let controller = for_home(home)?;
    let _construction = controller.construction.lock().map_err(poisoned)?;
    // Changing the default owner cannot authorize replacing old resources.
    // Their registration must first be removed by a confirmed native Stop.
    if snapshot(&actor.group_id, &actor.actor_id)?.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "member is still registered; confirm its stop before starting it again",
        ));
    }
    let (client, created_factory) = current_factory(&controller)?;
    // Publish and reserve its first Start before another constructor can make
    // this factory non-default and retire it as empty. Only admission is under
    // this lock; provider initialization still waits outside it.
    let operation = client.mutate(Mutation::Start {
        actor,
        launch: Box::new(launch),
    })?;
    drop(_construction);
    let result = client.wait(&operation, Duration::from_secs(60));
    if matches!(
        client.query(&operation),
        Ok(OperationState::Finished { .. })
    ) {
        // Lifecycle results need no Group-delivery append. A failed launch's
        // actual holding registration still remains in the owner snapshot.
        client.acknowledge(&operation)?;
        retire_unused_factory(&controller, &client);
    }
    let mut snapshot: ActorSnapshot = serde_json::from_value(result?).map_err(io::Error::other)?;
    if let Some(terminal) = &mut snapshot.terminal {
        terminal.launch = snapshot.terminal_launch;
    }
    // A new factory is ready. Failure to retire an old one must not undo Start.
    if created_factory {
        retire_unused_factories(&controller);
    }
    Ok(snapshot)
}

pub(super) fn perform(actor: &ActorRef, mutation: Mutation) -> io::Result<serde_json::Value> {
    let stopping = matches!(&mutation, Mutation::Stop { .. });
    let client = client_for(actor)?;
    let operation = client.mutate(mutation)?;
    let result = client.wait(&operation, Duration::from_secs(60));
    if matches!(
        client.query(&operation),
        Ok(OperationState::Finished { .. })
    ) {
        client.acknowledge(&operation)?;
        if stopping {
            retire_after_acknowledgement(&client);
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
pub(super) fn submit(
    home: &HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &cccc_contracts::Actor,
    events: &[cccc_contracts::Event],
    text: &str,
    cancelled: &std::sync::atomic::AtomicBool,
    started: &std::sync::atomic::AtomicBool,
) -> io::Result<serde_json::Value> {
    use std::sync::atomic::Ordering;
    let controller = for_home(home)?;
    let snapshot = snapshot(&group.group_id, &actor.id)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "member has no owner"))?;
    let source = OperationSource {
        actor: snapshot.actor.clone(),
        event_ids: events.iter().map(|event| event.id.clone()).collect(),
        input_sha256: format!("{:x}", Sha256::digest(text.as_bytes())),
    };
    let source_key = source.clone();
    let client = client_for(&snapshot.actor)?;
    let known = controller
        .deliveries
        .lock()
        .map_err(poisoned)?
        .get(&source_key)
        .cloned();
    let operation = match known {
        Some(operation) => {
            started.store(true, Ordering::Release);
            operation
        }
        None => {
            let existing = client.find(&source)?;
            if existing.len() > 1 {
                return Err(io::Error::other(
                    "multiple executions match the same delivery",
                ));
            }
            if let Some(operation) = existing.into_iter().next() {
                started.store(true, Ordering::Release);
                controller
                    .deliveries
                    .lock()
                    .map_err(poisoned)?
                    .insert(source_key.clone(), operation.clone());
                operation
            } else {
                if cancelled.load(Ordering::Acquire) {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "controller is stopping before submission",
                    ));
                }
                let mutation = Mutation::Submit {
                    source,
                    group: Box::new(group.clone()),
                    actor: Box::new(actor.clone()),
                    events: events.to_vec(),
                    text: text.into(),
                };
                let admitted = client.submit(mutation, |operation| {
                    let mut deliveries = controller.deliveries.lock().map_err(poisoned)?;
                    if deliveries.len() >= 4096 {
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "controller delivery fact capacity is full",
                        ));
                    }
                    deliveries.insert(source_key.clone(), operation.clone());
                    started.store(true, Ordering::Release);
                    Ok(())
                });
                match admitted {
                    Ok(operation) => operation,
                    Err(error) => {
                        // A framed admission rejection is definitive; a broken
                        // exchange is not. Retain its pre-write operation ID.
                        if error.get_ref().is_some_and(|cause| cause.is::<Failure>()) {
                            controller
                                .deliveries
                                .lock()
                                .map_err(poisoned)?
                                .remove(&source_key);
                            started.store(false, Ordering::Release);
                        }
                        return Err(error);
                    }
                }
            }
        }
    };
    // If an earlier exchange broke, fence its old buffered stream before asking
    // whether its known sequence was ever admitted. Never submit a replacement.
    client.take_control()?;
    loop {
        match client.query(&operation)? {
            OperationState::Finished { result } => {
                let result = result.map_err(io::Error::other)?;
                let not_started = result.get("started").and_then(serde_json::Value::as_bool)
                    == Some(false)
                    && matches!(
                        result.get("outcome").and_then(serde_json::Value::as_str),
                        Some("Deferred" | "Incomplete")
                    );
                let not_written = result.get("backend").and_then(serde_json::Value::as_str)
                    == Some("pty")
                    && result.get("outcome").and_then(serde_json::Value::as_str)
                        == Some("not_written");
                if not_started || not_written {
                    for event in events {
                        crate::ops::runtime_delivery::append_state(
                            home,
                            &group.group_id,
                            &actor.id,
                            &actor.created_at,
                            &event.id,
                            match result.get("backend").and_then(serde_json::Value::as_str) {
                                Some("deepseek") => "deepseek",
                                Some("managed") => "managed_session",
                                _ => "pty",
                            },
                            crate::ops::runtime_delivery::DeliveryOutcome::Unsubmitted(
                                "owner operation ended before native input was written",
                            ),
                        )
                        .map_err(|error| io::Error::other(error.message))?;
                    }
                }
                return Ok(result);
            }
            OperationState::NotAdmitted => {
                controller
                    .deliveries
                    .lock()
                    .map_err(poisoned)?
                    .remove(&source_key);
                started.store(false, Ordering::Release);
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "prior operation was terminally not admitted",
                ));
            }
            OperationState::Unknown => {
                return Err(io::Error::other(
                    "delivery outcome is unknown; input was not replayed",
                ));
            }
            OperationState::Pending => {}
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "controller stopped observing retained execution",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(super) fn find_event(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    created: &str,
    event: &str,
) -> io::Result<Vec<(Arc<Client>, ExecutionFact)>> {
    let controller = for_home(home)?;
    let clients = controller
        .clients
        .lock()
        .map_err(poisoned)?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let mut facts = Vec::new();
    for client in clients {
        facts.extend(
            client
                .find_event(group, actor, created, event)?
                .into_iter()
                .map(|fact| (client.clone(), fact)),
        );
    }
    Ok(facts)
}

pub(super) fn event_facts(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    created: &str,
    event: &str,
) -> io::Result<Vec<(Arc<Client>, ExecutionFact)>> {
    let controller = for_home(home)?;
    let known = controller
        .deliveries
        .lock()
        .map_err(poisoned)?
        .iter()
        .filter(|(source, _)| {
            source.actor.group_id == group
                && source.actor.actor_id == actor
                && source.actor.actor_created_at == created
                && source.event_ids.iter().any(|id| id == event)
        })
        .map(|(source, operation)| (source.clone(), operation.clone()))
        .collect::<Vec<_>>();
    if known.is_empty() {
        return find_event(home, group, actor, created, event);
    }
    let clients = controller
        .clients
        .lock()
        .map_err(poisoned)?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    known
        .into_iter()
        .map(|(source, operation)| {
            let client = clients
                .iter()
                .find(|client| client.address.instance == operation.instance)
                .cloned()
                .ok_or_else(|| io::Error::other("known execution owner is unavailable"))?;
            let state = client.query(&operation)?;
            Ok((
                client,
                ExecutionFact {
                    source,
                    operation,
                    state,
                },
            ))
        })
        .collect()
}

pub(super) fn acknowledge_event(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    created: &str,
    event: &str,
) -> io::Result<()> {
    let document = cccc_core::GroupStore::new(home.clone())?.load(group)?;
    if !document
        .actors
        .iter()
        .any(|candidate| candidate.id == actor && candidate.created_at == created)
    {
        return Ok(());
    }
    for (client, fact) in event_facts(home, group, actor, created, event)? {
        if !matches!(fact.state, OperationState::Finished { .. }) {
            continue;
        }
        let settled = fact.source.event_ids.iter().all(|id| {
            cccc_core::inbox::delivery_states(home, &document, &[actor.into()], id).is_ok_and(
                |states| {
                    states.get(actor).is_some_and(|(state, _)| {
                        matches!(
                            state.as_str(),
                            "accepted" | "failed" | "withdrawn" | "ambiguous"
                        )
                    })
                },
            )
        });
        if settled {
            client.acknowledge(&fact.operation)?;
            for controller in controllers().lock().map_err(poisoned)?.values() {
                controller
                    .deliveries
                    .lock()
                    .map_err(poisoned)?
                    .retain(|_, operation| operation != &fact.operation);
            }
            retire_after_acknowledgement(&client);
        }
    }
    Ok(())
}

pub(super) fn finish(
    home: &HomeLayout,
    mode: ShutdownMode,
    request_id: String,
) -> io::Result<ShutdownReport> {
    let controller = for_home(home)?;
    let clients = controller
        .clients
        .lock()
        .map_err(poisoned)?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let report = finish_clients(clients, mode, request_id)?;
    // Do not remove on failure before collecting the report; a failed Stop
    // still owns resources and must remain discoverable to an offline Stop.
    controllers().lock().map_err(poisoned)?.remove(home.root());
    Ok(report)
}

fn finish_clients(
    clients: Vec<Arc<Client>>,
    mode: ShutdownMode,
    request_id: String,
) -> io::Result<ShutdownReport> {
    let mut report = ShutdownReport {
        v: 1,
        daemon_pid: std::process::id(),
        request_id,
        mode,
        confirmed_stopped: Vec::new(),
        retained: Vec::new(),
        unconfirmed: Vec::new(),
    };
    let mut first_error = None;
    for client in clients {
        let result = (|| -> io::Result<()> {
            match mode {
                ShutdownMode::PreserveAgents => {
                    client.release()?;
                    // Verify after releasing control; the returned identities are
                    // observation at this point, not a promise of future liveness.
                    let observed = client.owner_snapshot()?;
                    for snapshot in observed.actors {
                        if running(&snapshot) {
                            report
                                .retained
                                .push(identity(&snapshot.actor, &client.address.owner_id));
                        } else if snapshot.owner.as_ref().is_none_or(|owner| {
                            owner.state != crate::ops::actor_runtime::OwnerState::Exited
                        }) {
                            report
                                .unconfirmed
                                .push(identity(&snapshot.actor, &client.address.owner_id));
                        }
                    }
                    for browser in observed.browsers {
                        match browser.resource.state {
                            super::browser::BrowserResourceState::Ready => {
                                report.retained.extend(browser.identities())
                            }
                            super::browser::BrowserResourceState::Exited => {}
                            _ => report.unconfirmed.extend(browser.identities()),
                        }
                    }
                }
                ShutdownMode::Stop => {
                    // A failed observation must not prevent the explicit stop.
                    let before = client.owner_snapshot();
                    let operation = client.mutate(Mutation::StopAll)?;
                    match client.wait(&operation, Duration::from_secs(60)) {
                        Ok(result) => {
                            let stopped: Vec<ActorRef> = serde_json::from_value(
                                result
                                    .get("stopped")
                                    .cloned()
                                    .ok_or_else(|| io::Error::other("missing stopped set"))?,
                            )
                            .map_err(io::Error::other)?;
                            let unconfirmed: Vec<ActorRef> = serde_json::from_value(
                                result
                                    .get("unconfirmed")
                                    .cloned()
                                    .ok_or_else(|| io::Error::other("missing unconfirmed set"))?,
                            )
                            .map_err(io::Error::other)?;
                            report.confirmed_stopped.extend(
                                stopped
                                    .iter()
                                    .map(|actor| identity(actor, &client.address.owner_id)),
                            );
                            report.unconfirmed.extend(
                                unconfirmed
                                    .iter()
                                    .map(|actor| identity(actor, &client.address.owner_id)),
                            );
                            if result
                                .get("browsers_stopped")
                                .and_then(serde_json::Value::as_bool)
                                == Some(false)
                            {
                                return Err(io::Error::other(
                                    "browser resources remain unconfirmed; the owner retains their handles",
                                ));
                            }
                            if let Err(error) = client.acknowledge(&operation) {
                                tracing::debug!(pid = client.address.pid, kind = ?error.kind(), "stopped owner result acknowledgement is pending");
                            }
                            if unconfirmed.is_empty() {
                                if let Err(error) = client.retire() {
                                    tracing::warn!(pid = client.address.pid, kind = ?error.kind(), "members stopped; empty owner retirement is pending");
                                }
                            }
                        }
                        Err(error) => match before {
                            Ok(before) => {
                                report
                                    .unconfirmed
                                    .extend(before.actors.iter().map(|snapshot| {
                                        identity(&snapshot.actor, &client.address.owner_id)
                                    }));
                                report.unconfirmed.extend(
                                    before
                                        .browsers
                                        .iter()
                                        .flat_map(|browser| browser.identities()),
                                );
                            }
                            Err(_) => return Err(error),
                        },
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            tracing::warn!(pid = client.address.pid, kind = ?error.kind(), "owner cleanup incomplete; continuing with other owners");
            first_error.get_or_insert(error);
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(report)
}

pub(super) fn stop_offline(home: &HomeLayout) -> anyhow::Result<ShutdownReport> {
    let _lock = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))?
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "daemon still owns its home; cannot stop retained agents offline",
            )
        })?;
    let (clients, discovery_error) = discover_all(home)?;
    let stopped = finish_clients(
        clients.into_values().collect(),
        ShutdownMode::Stop,
        uuid::Uuid::new_v4().simple().to_string(),
    );
    if let Some(error) = discovery_error {
        return Err(error.into());
    }
    let report = stopped?;
    super::receipt::write_shutdown_report(home, &report)?;
    Ok(report)
}

fn discover(home: &HomeLayout) -> io::Result<BTreeMap<String, Arc<Client>>> {
    let (clients, error) = discover_all(home)?;
    match error {
        Some(error) => Err(error),
        None => Ok(clients),
    }
}

// Discovery failures do not erase verified clients: an explicit offline Stop
// still reclaims every owner it can contact before returning an unknown result.
fn discover_all(home: &HomeLayout) -> io::Result<(OwnerClients, Option<io::Error>)> {
    let directory = home.daemon_dir().join("session-owners");
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((BTreeMap::new(), None));
        }
        Err(error) => return Err(error),
    };
    let mut clients = BTreeMap::new();
    let mut first_error = None;
    for entry in entries {
        let result = (|| -> io::Result<Option<(String, Arc<Client>)>> {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                return Ok(None);
            }
            let owner_id = entry.file_name().to_string_lossy().into_owned();
            let paths = Paths::new(home, &owner_id)?;
            // A free instance lock proves only that this owner is dead. Its guard
            // ledger is retained for the next authorized construction/recovery.
            let lock = cccc_core::fs::try_exclusive_lock(&paths.lock)?;
            if lock.is_some() {
                if paths.address.exists() {
                    let reconciled = super::recovery::reconcile(&paths);
                    if matches!(reconciled, Ok(true)) {
                        return Ok(None);
                    }
                    let pid = client::load(home, &owner_id)
                        .ok()
                        .map(|address| address.pid.to_string())
                        .unwrap_or_else(|| "unknown".into());
                    return Err(io::Error::other(format!(
                        "session owner {owner_id} (last PID {pid}) has unconfirmed old work; no members were automatically restarted; run `cccc daemon process-recovery --owner session --owner-id {owner_id} list` to review, then retry verified cleanup or explicitly accept remaining risk; address {}, ownership evidence {}",
                        paths.address.display(),
                        paths.directory.join("owned-process-groups.json").display(),
                    )));
                }
                return Ok(None);
            }
            let address = client::load(home, &owner_id)?;
            Ok(Some((owner_id, Arc::new(Client::connect(address)?))))
        })();
        match result {
            Ok(Some((id, client))) => {
                clients.insert(id, client);
            }
            Ok(None) => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    Ok((clients, first_error))
}

pub(super) fn identity(actor: &ActorRef, owner_id: &str) -> AgentIdentity {
    AgentIdentity {
        group_id: actor.group_id.clone(),
        actor_id: actor.actor_id.clone(),
        actor_created_at: actor.actor_created_at.clone(),
        generation: actor.generation.clone(),
        owner_id: Some(owner_id.into()),
    }
}
pub(super) fn running(snapshot: &ActorSnapshot) -> bool {
    snapshot.observation_error.is_none()
        && !snapshot.provider_exited
        && snapshot
            .owner
            .as_ref()
            .is_some_and(|owner| owner.state == crate::ops::actor_runtime::OwnerState::Running)
}

pub(super) fn track_recovery(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    created: &str,
    event: &str,
    transport: &str,
) -> io::Result<()> {
    let controller = for_home(home)?;
    let mut recoveries = controller.recoveries.lock().map_err(poisoned)?;
    if recoveries.len() >= 4096 {
        return Err(io::Error::other(
            "retained delivery recovery capacity is full",
        ));
    }
    recoveries.insert(
        (group.into(), actor.into(), created.into(), event.into()),
        transport.into(),
    );
    Ok(())
}

pub(super) fn recovery_groups() -> Vec<String> {
    let mut groups = std::collections::BTreeSet::new();
    if let Ok(controllers) = controllers().lock() {
        for controller in controllers.values() {
            if let Ok(recoveries) = controller.recoveries.lock() {
                groups.extend(recoveries.keys().map(|(group, _, _, _)| group.clone()));
            }
        }
    }
    groups.into_iter().collect()
}

pub(super) fn recoveries(
    home: &HomeLayout,
    group: &str,
) -> io::Result<Vec<(String, String, String, String)>> {
    let controller = for_home(home)?;
    Ok(controller
        .recoveries
        .lock()
        .map_err(poisoned)?
        .iter()
        .filter(|((group_id, _, _, _), _)| group_id == group)
        .map(|((_, actor, created, event), transport)| {
            (
                actor.clone(),
                created.clone(),
                event.clone(),
                transport.clone(),
            )
        })
        .collect())
}

pub(super) fn forget_recovery(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    created: &str,
    event: &str,
) -> io::Result<()> {
    for_home(home)?
        .recoveries
        .lock()
        .map_err(poisoned)?
        .remove(&(group.into(), actor.into(), created.into(), event.into()));
    Ok(())
}
fn poisoned<T>(_: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other("session owner controller lock poisoned")
}

#[cfg(all(test, target_os = "linux", debug_assertions))]
mod tests {
    use super::*;

    #[test]
    fn a_closed_empty_factory_retires_after_a_new_factory_starts() {
        let temp = tempfile::tempdir().expect("isolated test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("resolve isolated home");
        home.initialize().expect("initialize fixture state");
        let _home = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))
            .expect("check fixture lock")
            .expect("home lock is available");
        let old_id = uuid::Uuid::new_v4().simple().to_string();
        let old = client::start(&home, &old_id).expect("start session owner");
        let stopping = old
            .mutate(Mutation::StopAll)
            .expect("submit owner mutation");
        old.wait(&stopping, Duration::from_secs(5))
            .expect("wait for owner operation");
        assert!(
            !old.current_start_factory()
                .expect("read owner start admission")
        );
        let default_path = home.daemon_dir().join("session-owners/default.json");
        cccc_core::fs::write_json(&default_path, &old_id).expect("write recovery fixture");
        initialize(&home).expect("initialize fixture state");
        let actor = ActorRef {
            group_id: "g_fresh_owner".into(),
            actor_id: "peer".into(),
            actor_created_at: "created".into(),
            generation: None,
        };
        let result = start(
            &home,
            actor.clone(),
            PreparedLaunch::Pty {
                spec: cccc_runtime::LaunchSpec {
                    group_id: actor.group_id.clone(),
                    actor_id: actor.actor_id.clone(),
                    runner: cccc_contracts::RunnerKind::Pty,
                    command: vec!["sleep".into(), "60".into()],
                    cwd: temp.path().into(),
                    env: Default::default(),
                    cols: 80,
                    rows: 24,
                },
                history: cccc_runtime::HistoryConfig {
                    path: temp.path().join("history"),
                    max_bytes: 4096,
                    hot_bytes: 4096,
                    persist: false,
                },
                launch: cccc_runtime::ActorLaunch {
                    runtime: cccc_contracts::ActorRuntime::Custom,
                    submit: cccc_contracts::Actor::new("peer").submit,
                },
                preamble: String::new(),
            },
        );
        let new_id: String =
            cccc_core::fs::read_json(&default_path).expect("read saved owner identity");
        assert_ne!(new_id, old_id);
        let old_address = Paths::new(&home, &old_id)
            .expect("resolve old owner")
            .address;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while old_address.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !old_address.exists(),
            "a confirmed empty old factory retires"
        );
        let stopped = finish(&home, ShutdownMode::Stop, "stop-all".into())
            .expect("finish confirmed shutdown");
        assert!(
            result
                .expect("start member on fresh owner")
                .terminal
                .expect("PTY snapshot after Start")
                .running
        );
        assert_eq!(stopped.confirmed_stopped.len(), 1);
        assert!(stopped.unconfirmed.is_empty());
    }

    #[test]
    fn an_unavailable_owner_does_not_prevent_stopping_the_other_owners() {
        let temp = tempfile::tempdir().expect("isolated test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("resolve isolated home");
        home.initialize().expect("initialize fixture state");
        let _home = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))
            .expect("check fixture lock")
            .expect("home lock is available");
        let first = Arc::new(
            client::start(&home, &uuid::Uuid::new_v4().simple().to_string())
                .expect("start session owner"),
        );
        let second = Arc::new(
            client::start(&home, &uuid::Uuid::new_v4().simple().to_string())
                .expect("start session owner"),
        );
        first.retire().expect("retire empty owner");
        let first_path = Paths::new(&home, &first.address.owner_id).expect("resolve owner paths");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while first_path.address.exists() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        initialize(&home).expect("initialize fixture state");
        let controller = for_home(&home).expect("registered home controller");
        controller
            .clients
            .lock()
            .expect("fixture registry lock")
            .insert(first.address.owner_id.clone(), first.clone());
        let target = controller
            .clients
            .lock()
            .expect("fixture registry lock")
            .get(&second.address.owner_id)
            .expect("registered fixture entry")
            .clone();
        let store = cccc_core::GroupStore::new(home.clone()).expect("open isolated group store");
        let mut group = store
            .create("claim isolation", "")
            .expect("create fixture group");
        let managed = cccc_contracts::Actor::new("managed");
        let mut external = cccc_contracts::Actor::new("external");
        external.external_session_id = "user-owned-native-session".into();
        group.actors.extend([managed.clone(), external.clone()]);
        store.save(&group).expect("save fixture group");
        use crate::ops::runtime_delivery::{ClaimResult, DeliveryOutcome, append_state, claim};
        assert_eq!(
            claim(&home, &group, &managed, "new-input", "deepseek", false).expect("claim delivery"),
            ClaimResult::Claimed
        );
        // Use a completed lifecycle operation as an inert routing fixture. The
        // assertion is which owner gets queried/acknowledged, not native input.
        let completed = target
            .mutate(Mutation::StopAll)
            .expect("submit owner mutation");
        target
            .wait(&completed, Duration::from_secs(5))
            .expect("wait for owner operation");
        controller
            .deliveries
            .lock()
            .expect("fixture registry lock")
            .insert(
                OperationSource {
                    actor: ActorRef {
                        group_id: group.group_id.clone(),
                        actor_id: managed.id.clone(),
                        actor_created_at: managed.created_at.clone(),
                        generation: None,
                    },
                    event_ids: vec!["new-input".into()],
                    input_sha256: "routing-fixture".into(),
                },
                completed.clone(),
            );
        append_state(
            &home,
            &group.group_id,
            &managed.id,
            &managed.created_at,
            "new-input",
            "deepseek",
            DeliveryOutcome::Failed("definitive fixture failure"),
        )
        .expect("append delivery state");
        assert!(
            controller
                .deliveries
                .lock()
                .expect("fixture registry lock")
                .is_empty(),
            "the unrelated stale owner must not block the known owner's Ack"
        );
        assert!(matches!(
            target.query(&completed).expect("query admitted operation"),
            OperationState::Unknown
        ));
        assert_eq!(
            claim(
                &home,
                &group,
                &external,
                "native-input",
                "local_session",
                false
            )
            .expect("claim delivery"),
            ClaimResult::Claimed
        );
        append_state(
            &home,
            &group.group_id,
            &external.id,
            &external.created_at,
            "native-input",
            "local_session",
            DeliveryOutcome::Unsubmitted("native channel unavailable"),
        )
        .expect("append delivery state");
        assert_eq!(
            claim(
                &home,
                &group,
                &external,
                "native-input",
                "local_session",
                false
            )
            .expect("claim delivery"),
            ClaimResult::Claimed
        );
        controllers()
            .lock()
            .expect("fixture registry lock")
            .remove(home.root());
        assert!(
            finish_clients(
                vec![first, second.clone()],
                ShutdownMode::Stop,
                "stop-all".into()
            )
            .is_err()
        );
        let second_path = Paths::new(&home, &second.address.owner_id).expect("resolve owner paths");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while second_path.address.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "later owner was skipped after the first error"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
