use super::client::{self, Client};
use super::facade::OwnedTerminalExit;
use super::protocol::{
    ActorRef, ActorSnapshot, ExecutionFact, Failure, Mutation, OperationId, OperationSource,
    OperationState, PreparedLaunch,
};
use super::receipt::{AgentIdentity, ShutdownMode, ShutdownReport};
use super::transport::Paths;
use cccc_core::HomeLayout;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

type OwnerClients = BTreeMap<String, Arc<Client>>;

struct Controller {
    home: HomeLayout,
    clients: Mutex<OwnerClients>,
    members: Mutex<BTreeMap<(String, String), Arc<MemberRegistration>>>,
    construction: Mutex<()>,
    revision: AtomicU64,
    deliveries: Mutex<BTreeMap<OperationSource, OperationId>>,
    recoveries: Mutex<BTreeMap<DeliveryRecoveryKey, (String, bool)>>,
}

type DeliveryRecoveryKey = (String, String, String, String);

struct MemberRegistration {
    client: Arc<Client>,
    actor: ActorRef,
    // Retain the operation identity across promotion of a pending Start, so
    // a concurrent Stop can remove this registration without removing a new one.
    start: Option<OperationId>,
    start_pending: bool,
    terminal_exited: AtomicBool,
}

impl MemberRegistration {
    fn pending_start(&self) -> Option<&OperationId> {
        self.start.as_ref().filter(|_| self.start_pending)
    }
}

fn member_key(actor: &ActorRef) -> (String, String) {
    (actor.group_id.clone(), actor.actor_id.clone())
}

impl Controller {
    fn changed(&self) {
        self.revision.fetch_add(1, Ordering::Release);
    }

    fn registration(
        &self,
        group: &str,
        actor: &str,
    ) -> io::Result<Option<Arc<MemberRegistration>>> {
        Ok(self
            .members
            .lock()
            .map_err(poisoned)?
            .get(&(group.to_owned(), actor.to_owned()))
            .cloned())
    }

    fn remember_start(
        &self,
        client: &Arc<Client>,
        actor: &ActorRef,
        operation: &OperationId,
    ) -> io::Result<Arc<MemberRegistration>> {
        let entry = Arc::new(MemberRegistration {
            client: client.clone(),
            actor: actor.clone(),
            start: Some(operation.clone()),
            start_pending: true,
            terminal_exited: AtomicBool::new(false),
        });
        self.members
            .lock()
            .map_err(poisoned)?
            .insert(member_key(actor), entry.clone());
        self.changed();
        Ok(entry)
    }

    fn confirm_stop(&self, registration: &Arc<MemberRegistration>) -> io::Result<()> {
        let mut members = self.members.lock().map_err(poisoned)?;
        let key = member_key(&registration.actor);
        if members.get(&key).is_some_and(|member| {
            member.pending_start().is_none()
                && (Arc::ptr_eq(member, registration)
                    || (registration.start.is_some() && member.start == registration.start))
        }) {
            members.remove(&key);
            self.changed();
        }
        Ok(())
    }
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
    let mut members: BTreeMap<_, Arc<MemberRegistration>> = BTreeMap::new();
    for client in clients.values() {
        client.take_control()?;
        let observed = client.owner_snapshot()?;
        for snapshot in &observed.actors {
            let actor = snapshot.actor.clone();
            let key = member_key(&actor);
            if let Some(previous) = members.get(&key) {
                return Err(io::Error::other(format!(
                    "member {}/{} is registered with both session owners {} and {}",
                    actor.group_id,
                    actor.actor_id,
                    previous.client.address.owner_id,
                    client.address.owner_id,
                )));
            }
            members.insert(
                key,
                Arc::new(MemberRegistration {
                    client: client.clone(),
                    actor,
                    start: None,
                    start_pending: false,
                    terminal_exited: AtomicBool::new(terminal_exited(snapshot)),
                }),
            );
        }
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
        members: Mutex::new(members),
        construction: Mutex::new(()),
        revision: AtomicU64::new(0),
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
        if let Ok(mut members) = controller.members.lock() {
            members.retain(|_, member| !Arc::ptr_eq(&member.client, client));
        }
        controller.changed();
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
    if let Some((group, actor)) = group.zip(actor)
        && super::projection::captured_home().as_deref() == Some(home.root())
    {
        return snapshot(group, actor).map(drop);
    }
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

/// An observation made without the Group write permit. Its captured scope
/// permits local decisions only, even if an owner was added after capture.
#[derive(Default)]
pub(crate) struct OwnerObservations {
    source: ObservationSource,
    snapshots: Arc<super::projection::Snapshots>,
    exited_terminals: Vec<OwnedTerminalExit>,
    provider_exits: Vec<AgentIdentity>,
}

#[derive(Default)]
enum ObservationSource {
    #[default]
    Local,
    Captured {
        controller: Arc<Controller>,
        revision: u64,
    },
    Unavailable(PathBuf),
}

impl OwnerObservations {
    pub(crate) fn unavailable(home: &HomeLayout) -> Self {
        Self {
            source: ObservationSource::Unavailable(home.root().to_path_buf()),
            ..Self::default()
        }
    }

    pub(crate) fn is_current(&self) -> bool {
        match &self.source {
            ObservationSource::Local => true,
            ObservationSource::Unavailable(_) => false,
            ObservationSource::Captured {
                controller,
                revision,
            } => {
                controller.revision.load(Ordering::Acquire) == *revision
                    && for_home(&controller.home)
                        .is_ok_and(|current| Arc::ptr_eq(&current, controller))
            }
        }
    }

    pub(crate) fn with_scope<T>(&self, read: impl FnOnce() -> T) -> T {
        let home = match &self.source {
            ObservationSource::Local => return read(),
            ObservationSource::Captured { controller, .. } => controller.home.root(),
            ObservationSource::Unavailable(home) => home.as_path(),
        };
        let snapshots = if self.is_current() {
            self.snapshots.clone()
        } else {
            Arc::default()
        };
        super::projection::captured(home, snapshots, read)
    }

    pub(crate) fn exited_terminals(&self) -> &[OwnedTerminalExit] {
        &self.exited_terminals
    }

    pub(crate) fn provider_exits(&self, group_id: &str) -> Vec<AgentIdentity> {
        self.provider_exits
            .iter()
            .filter(|exit| exit.group_id == group_id)
            .cloned()
            .collect()
    }

    pub(crate) fn exit_group_ids(&self) -> Vec<String> {
        self.exited_terminals
            .iter()
            .map(|exit| exit.status.group_id.clone())
            .chain(self.provider_exits.iter().map(|exit| exit.group_id.clone()))
            .collect()
    }
}

pub(super) fn observe_owners_for_maintenance(home: &HomeLayout) -> io::Result<OwnerObservations> {
    let controller = for_home(home)?;
    let revision = controller.revision.load(Ordering::Acquire);
    let mut observed = OwnerObservations {
        source: ObservationSource::Captured {
            controller: controller.clone(),
            revision,
        },
        ..Default::default()
    };
    for client in clients_for_observation(home)? {
        let snapshots = client.snapshots();
        match &snapshots {
            Ok(members) => {
                let starts = controller
                    .members
                    .lock()
                    .map_err(poisoned)?
                    .values()
                    .filter(|entry| Arc::ptr_eq(&client, &entry.client) && entry.start_pending)
                    .cloned()
                    .collect::<Vec<_>>();
                for start in starts {
                    let _ = settle_start(&controller, &start);
                }
                for snapshot in members
                    .iter()
                    .filter(|member| member.observation_error.is_none())
                {
                    if terminal_exited(snapshot)
                        && let Some(entry) = controller
                            .registration(&snapshot.actor.group_id, &snapshot.actor.actor_id)?
                        && Arc::ptr_eq(&client, &entry.client)
                        && !entry.start_pending
                        && entry.actor == snapshot.actor
                    {
                        // The exact terminal cannot run again. A replacement
                        // has its own registration, even under the same name.
                        entry.terminal_exited.store(true, Ordering::Release);
                    }
                    if snapshot.provider_exited {
                        observed
                            .provider_exits
                            .push(identity(&snapshot.actor, &client.address.owner_id));
                    }
                    if let Some(status) =
                        snapshot.terminal.as_ref().filter(|status| !status.running)
                    {
                        if let Some(managed) = &snapshot.managed_status {
                            // Losing a viewer is not a provider exit. Apply the
                            // detachment to its original holder outside the Group permit.
                            if managed.pid.is_some() {
                                let detach = Mutation::DetachViewer {
                                    actor: snapshot.actor.clone(),
                                };
                                if let Err(error) = wait_viewer_detach(&client, detach) {
                                    tracing::debug!(%error, "managed terminal detachment remains unconfirmed");
                                }
                            }
                        } else {
                            observed.exited_terminals.push(OwnedTerminalExit {
                                status: status.clone(),
                                actor_created_at: snapshot.actor.actor_created_at.clone(),
                            });
                        }
                    }
                }
            }
            Err(error) => {
                tracing::debug!(owner_id = %client.address.owner_id, kind = ?error.kind(), "session owner could not be observed for maintenance")
            }
        }
        Arc::make_mut(&mut observed.snapshots).insert(
            client.address.instance.clone(),
            super::projection::capture(snapshots),
        );
    }
    Ok(observed)
}

fn wait_viewer_detach(client: &Client, detach: Mutation) -> io::Result<()> {
    let operation = client.mutate(detach)?;
    let result = client.wait(&operation, Duration::from_secs(5));
    if matches!(
        client.query(&operation),
        Ok(OperationState::Finished { .. })
    ) {
        client.acknowledge(&operation)?;
    }
    result.map(drop)
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
    programs: Option<super::browser::BrowserPrograms>,
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
    let operation = client.mutate(Mutation::BrowserEnsure { spec, programs })?;
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
    if let Some(home) = super::projection::captured_home() {
        let controller = controllers().lock().map_err(poisoned)?.get(&home).cloned();
        return controller.map_or_else(
            || {
                Err(io::Error::other(
                    "captured owner controller is no longer available",
                ))
            },
            |controller| clients_for_observation(&controller.home),
        );
    }
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

#[cfg(test)]
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

pub(super) fn snapshot(group: &str, actor: &str) -> io::Result<Option<ActorSnapshot>> {
    if let Some(home) = super::projection::captured_home() {
        let controller = controllers().lock().map_err(poisoned)?.get(&home).cloned();
        if let Some(controller) = controller
            && let Some(member) = controller.registration(group, actor)?
        {
            if member.start_pending {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "member start is unconfirmed",
                ));
            }
            return snapshot_from([member.client.clone()], group, actor);
        }
    }
    snapshot_from(all_clients()?, group, actor)
}

fn member_snapshot<'a>(
    snapshots: &'a [ActorSnapshot],
    group: &str,
    actor: &str,
) -> io::Result<Option<&'a ActorSnapshot>> {
    let mut matches = snapshots
        .iter()
        .filter(|snapshot| snapshot.actor.group_id == group && snapshot.actor.actor_id == actor);
    let Some(snapshot) = matches.next() else {
        return Ok(None);
    };
    if let Some(error) = &snapshot.observation_error {
        return Err(io::Error::other(error.clone()));
    }
    if matches.next().is_some() {
        return Err(io::Error::other(
            "member is registered with multiple session owners",
        ));
    }
    Ok(Some(snapshot))
}

fn snapshot_from(
    clients: impl IntoIterator<Item = Arc<Client>>,
    group: &str,
    actor: &str,
) -> io::Result<Option<ActorSnapshot>> {
    Ok(observe_member(clients, group, actor)?.map(|(_, snapshot)| snapshot))
}

fn observe_member(
    clients: impl IntoIterator<Item = Arc<Client>>,
    group: &str,
    actor: &str,
) -> io::Result<Option<(Arc<Client>, ActorSnapshot)>> {
    let mut found = None;
    for client in clients {
        let snapshots = client.projected_snapshots()?;
        if let Some(snapshot) = member_snapshot(&snapshots, group, actor)? {
            if found.is_some() {
                return Err(io::Error::other(
                    "member is registered with multiple session owners",
                ));
            }
            found = Some((client, snapshot.clone()));
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
            controller.changed();
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
    resolve_start(&controller, &actor)?;
    // Changing the default owner cannot authorize replacing old resources.
    // Their registration must first be removed by a confirmed native Stop.
    if snapshot_from(
        clients_for_observation(home)?,
        &actor.group_id,
        &actor.actor_id,
    )?
    .is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "member is still registered; confirm its stop before starting it again",
        ));
    }
    let (client, created_factory) = current_factory(&controller)?;
    // Publish and reserve its first Start before another constructor can make
    // this factory non-default and retire it as empty. Only admission is under
    // this lock; provider initialization still waits outside it.
    let mut tracked = None;
    let admission = client.mutate_tracked(
        Mutation::Start {
            actor: actor.clone(),
            launch: Box::new(launch),
        },
        |operation| {
            tracked = Some(controller.remember_start(&client, &actor, operation)?);
            Ok(())
        },
    );
    if let Err(error) = &admission
        && error.get_ref().is_some_and(|cause| cause.is::<Failure>())
        && let Some(registration) = &tracked
    {
        replace_start(&controller, registration, None)?;
    }
    let operation = admission?;
    drop(_construction);
    let result = client.wait(&operation, Duration::from_secs(60));
    if let Some(registration) = &tracked {
        if let Ok(value) = &result {
            let snapshot: ActorSnapshot =
                serde_json::from_value(value.clone()).map_err(io::Error::other)?;
            if same_member(&snapshot.actor, &actor) {
                replace_start(&controller, registration, Some(snapshot.actor))?;
            }
            client.acknowledge(&operation)?;
        } else {
            // An execution error can leave an occupied reservation. Resolve
            // what the original owner can prove without replaying the Start.
            let _ = settle_start(&controller, registration);
        }
    }
    let mut snapshot: ActorSnapshot = serde_json::from_value(result?).map_err(io::Error::other)?;
    if let Some(terminal) = &mut snapshot.terminal {
        terminal.launch = snapshot.terminal_launch;
    }
    // A new factory is ready. Failure to retire an old one must not undo Start.
    if created_factory {
        retire_unused_factories(&controller);
    } else {
        retire_unused_factory(&controller, &client);
    }
    Ok(snapshot)
}

fn same_member(first: &ActorRef, second: &ActorRef) -> bool {
    first.group_id == second.group_id
        && first.actor_id == second.actor_id
        && first.actor_created_at == second.actor_created_at
}

fn replace_start(
    controller: &Controller,
    previous: &Arc<MemberRegistration>,
    actor: Option<ActorRef>,
) -> io::Result<()> {
    let mut members = controller.members.lock().map_err(poisoned)?;
    let key = member_key(&previous.actor);
    if members
        .get(&key)
        .is_some_and(|current| Arc::ptr_eq(current, previous))
    {
        if let Some(actor) = actor {
            members.insert(
                key,
                Arc::new(MemberRegistration {
                    client: previous.client.clone(),
                    actor,
                    start: previous.start.clone(),
                    start_pending: false,
                    terminal_exited: AtomicBool::new(false),
                }),
            );
        } else {
            members.remove(&key);
        }
        controller.changed();
    }
    Ok(())
}

fn resolve_start(controller: &Controller, actor: &ActorRef) -> io::Result<()> {
    if let Some(registration) = controller.registration(&actor.group_id, &actor.actor_id)?
        && registration.start_pending
    {
        registration.client.take_control()?;
        settle_start(controller, &registration)?;
    }
    Ok(())
}

fn settle_start(controller: &Controller, registration: &Arc<MemberRegistration>) -> io::Result<()> {
    let Some(operation) = registration.pending_start() else {
        return Ok(());
    };
    let client = &registration.client;
    let unresolved = || {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            "previous member start is unconfirmed",
        )
    };
    let (resolved, finished) = match client.query(operation)? {
        OperationState::NotAdmitted => (None, false),
        OperationState::Finished { .. } => {
            let actor = &registration.actor;
            let observed = client.snapshots()?.into_iter().find(|snapshot| {
                snapshot.actor.group_id == actor.group_id
                    && snapshot.actor.actor_id == actor.actor_id
            });
            match observed {
                Some(snapshot) if same_member(&snapshot.actor, actor) => {
                    (Some(snapshot.actor), true)
                }
                None => (None, true),
                _ => return Err(unresolved()),
            }
        }
        OperationState::Pending | OperationState::Unknown => return Err(unresolved()),
    };
    let released = resolved.is_none();
    replace_start(controller, registration, resolved)?;
    if finished {
        match client.acknowledge(operation) {
            Ok(()) if released => retire_unused_factory(controller, client),
            Ok(()) => {}
            Err(error) => {
                tracing::debug!(%error, "completed member start acknowledgement is pending")
            }
        }
    }
    Ok(())
}

pub(crate) struct OwnedTarget {
    controller: Arc<Controller>,
    registration: Arc<MemberRegistration>,
    snapshot: ActorSnapshot,
    observed: Vec<ActorSnapshot>,
}

impl OwnedTarget {
    pub(crate) fn observe<T>(&self, read: impl FnOnce() -> T) -> T {
        super::projection::captured(
            self.controller.home.root(),
            Arc::new(BTreeMap::from([(
                self.registration.client.address.instance.clone(),
                Ok(vec![self.snapshot.clone()]),
            )])),
            read,
        )
    }

    fn require_registration(&self) -> io::Result<()> {
        let actor = &self.snapshot.actor;
        let registration = self
            .controller
            .registration(&actor.group_id, &actor.actor_id)?;
        if for_home(&self.controller.home)
            .is_ok_and(|current| Arc::ptr_eq(&current, &self.controller))
            && registration.is_some_and(|entry| Arc::ptr_eq(&entry, &self.registration))
        {
            return Ok(());
        }
        Err(io::Error::other(
            "member registration changed before the operation",
        ))
    }

    pub(crate) fn stop(&self) -> io::Result<Option<cccc_runtime::SessionStatus>> {
        let actor = &self.snapshot.actor;
        self.require_registration()?;
        let client = &self.registration.client;
        let operation = client.mutate(Mutation::Stop {
            actor: actor.clone(),
        })?;
        client.wait(&operation, Duration::from_secs(60))?;
        self.controller.confirm_stop(&self.registration)?;
        if let Err(error) = client.acknowledge(&operation) {
            tracing::debug!(kind = ?error.kind(), "confirmed member stop acknowledgement is pending");
        } else {
            retire_unused_factory(&self.controller, client);
        }
        Ok(self.snapshot.terminal.clone().map(|mut status| {
            status.running = false;
            status
        }))
    }

    pub(crate) fn member_running(
        &self,
        group_id: &str,
        actor: &cccc_contracts::Actor,
    ) -> Option<bool> {
        let Some(entry) = self.controller.registration(group_id, &actor.id).ok()? else {
            return Some(false);
        };
        if entry.start_pending || entry.actor.actor_created_at != actor.created_at {
            return None;
        }
        if entry.terminal_exited.load(Ordering::Acquire) {
            return Some(false);
        }
        if !Arc::ptr_eq(&entry.client, &self.registration.client) {
            return None;
        }
        let snapshot = self.observed.iter().find(|observed| {
            same_member(&observed.actor, &entry.actor) && observed.observation_error.is_none()
        })?;
        use crate::ops::actor_runtime::OwnerState;
        match snapshot.owner.as_ref().map(|owner| owner.state) {
            Some(OwnerState::Running) if !snapshot.provider_exited => Some(true),
            Some(OwnerState::Exited) if entry.actor.generation.is_some() => {
                (snapshot.actor == entry.actor).then_some(false)
            }
            None if snapshot
                .terminal
                .as_ref()
                .is_some_and(|terminal| terminal.running) =>
            {
                Some(true)
            }
            _ => None,
        }
    }
}

pub(super) fn running_target(
    home: &HomeLayout,
    group_id: &str,
    actor: &cccc_contracts::Actor,
) -> io::Result<Option<OwnedTarget>> {
    let controller = for_home(home)?;
    let Some(entry) = controller.registration(group_id, &actor.id)? else {
        return Ok(None);
    };
    let replaced = || {
        io::Error::other(Failure::new(
            "actor_replaced",
            "member creation identity changed",
        ))
    };
    if entry.actor.actor_created_at != actor.created_at {
        return Err(replaced());
    }
    if entry.start_pending {
        return Ok(None);
    }
    let observed = entry.client.projected_snapshots()?;
    let Some(snapshot) = member_snapshot(&observed, group_id, &actor.id)?.cloned() else {
        return Ok(None);
    };
    if !same_member(&entry.actor, &snapshot.actor) {
        return Err(replaced());
    }
    if snapshot.actor.generation.is_none()
        || !running(&snapshot)
        || snapshot.owner.as_ref().is_none_or(|owner| {
            owner.state != crate::ops::actor_runtime::OwnerState::Running
                || !entry.client.supports_target(owner)
        })
    {
        return Ok(None);
    }
    Ok(Some(OwnedTarget {
        controller,
        registration: entry,
        snapshot,
        observed,
    }))
}

pub(super) fn perform(actor: &ActorRef, mutation: Mutation) -> io::Result<serde_json::Value> {
    let stopping = matches!(&mutation, Mutation::Stop { .. });
    let client = client_for(actor)?;
    let stopping_registration = if stopping {
        let controllers = controllers().lock().map_err(poisoned)?;
        let mut target = None;
        for controller in controllers.values() {
            if let Some(entry) = controller.registration(&actor.group_id, &actor.actor_id)?
                && Arc::ptr_eq(&entry.client, &client)
                && same_member(&entry.actor, actor)
            {
                target = Some((controller.clone(), entry));
                break;
            }
        }
        target
    } else {
        None
    };
    let operation = client.mutate(mutation)?;
    let result = client.wait(&operation, Duration::from_secs(60));
    if result.is_ok()
        && let Some((controller, entry)) = stopping_registration
    {
        // Stop waits for an admitted launch to leave the backend. Its execution
        // receipt may still be publishing, so keep an unresolved entry for maintenance.
        let _ = settle_start(&controller, &entry);
        controller.confirm_stop(&entry)?;
    }
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
    target: Option<&OwnedTarget>,
) -> io::Result<serde_json::Value> {
    let controller = for_home(home)?;
    let (client, snapshot) = match target {
        Some(target) => {
            if !Arc::ptr_eq(&controller, &target.controller)
                || target.snapshot.actor.group_id != group.group_id
                || target.snapshot.actor.actor_id != actor.id
                || target.snapshot.actor.actor_created_at != actor.created_at
            {
                return Err(io::Error::other(
                    "delivery target no longer matches the member",
                ));
            }
            target.require_registration()?;
            (target.registration.client.clone(), target.snapshot.clone())
        }
        None => observe_member(clients_for_observation(home)?, &group.group_id, &actor.id)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "member has no owner"))?,
    };
    let source = OperationSource {
        actor: snapshot.actor.clone(),
        event_ids: events.iter().map(|event| event.id.clone()).collect(),
        input_sha256: format!("{:x}", Sha256::digest(text.as_bytes())),
    };
    let source_key = source.clone();
    let known = {
        let deliveries = controller.deliveries.lock().map_err(poisoned)?;
        let known = deliveries.get(&source_key).cloned();
        if target.is_some()
            && ((known
                .as_ref()
                .is_some_and(|operation| operation.instance != client.address.instance))
                || deliveries.keys().any(|previous| {
                    previous != &source_key
                        && same_member(&previous.actor, &source.actor)
                        && previous
                            .event_ids
                            .iter()
                            .any(|id| source.event_ids.contains(id))
                }))
        {
            started.store(true, Ordering::Release);
            return Err(io::Error::other(
                "an earlier delivery attempt still owns this source",
            ));
        }
        known
    };
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
                if snapshot
                    .terminal
                    .as_ref()
                    .is_some_and(|terminal| terminal.running && terminal.startup_input_pending)
                {
                    return Ok(
                        serde_json::json!({"backend":"pty","outcome":"not_written","reason":"waiting for terminal startup input"}),
                    );
                }
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
                let admitted = client.mutate_tracked(mutation, |operation| {
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
                        use crate::ops::runtime_delivery::{
                            DeliveryOutcome, OwnerAcknowledgement, append_state_with_owner,
                        };
                        append_state_with_owner(
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
                            DeliveryOutcome::Unsubmitted(
                                "owner operation ended before native input was written",
                            ),
                            OwnerAcknowledgement::KnownAttempt,
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
    allow_lookup: bool,
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
        return if allow_lookup {
            find_event(home, group, actor, created, event)
        } else {
            Ok(Vec::new())
        };
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

pub(super) enum DeliveryLookup {
    AllOwners,
    KnownAttempt,
    RetryableSource,
}

pub(super) fn acknowledge_event(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    created: &str,
    event: &str,
    lookup: DeliveryLookup,
) -> io::Result<()> {
    let document = cccc_core::GroupStore::new(home.clone())?.load(group)?;
    if !document
        .actors
        .iter()
        .any(|candidate| candidate.id == actor && candidate.created_at == created)
    {
        return Ok(());
    }
    let mut facts = event_facts(
        home,
        group,
        actor,
        created,
        event,
        matches!(lookup, DeliveryLookup::AllOwners),
    )?;
    if facts.is_empty() && matches!(lookup, DeliveryLookup::RetryableSource) {
        // The durable receipt already proves no native input. Reclaim a retained
        // fact on the next target in case it would otherwise match this retry.
        // A different owner cannot match the new target's execution source.
        let controller = for_home(home)?;
        if let Some(entry) = controller.registration(group, actor)?
            && !entry.start_pending
            && entry.actor.actor_created_at == created
        {
            let client = &entry.client;
            facts.extend(
                client
                    .find_event(group, actor, created, event)?
                    .into_iter()
                    .map(|fact| (client.clone(), fact)),
            );
        } else {
            facts = find_event(home, group, actor, created, event)?;
        }
    }
    for (client, fact) in facts {
        if matches!(fact.state, OperationState::Pending) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "the original delivery attempt is still executing",
            ));
        }
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
    let mut held = BTreeSet::new();
    let mut report = finish_clients(clients, mode, request_id, &mut held)?;
    if mode == ShutdownMode::PreserveAgents {
        let remote = super::remote::read(
            home,
            std::time::Instant::now() + Duration::from_secs(5),
            &held,
        )?;
        if !remote.refusals.is_empty() {
            return Err(io::Error::other(
                "saved remote member resources could not be confirmed",
            ));
        }
        report.unconfirmed.extend(remote.active);
    }
    // Do not remove on failure before collecting the report; a failed Stop
    // still owns resources and must remain discoverable to an offline Stop.
    controllers().lock().map_err(poisoned)?.remove(home.root());
    Ok(report)
}

fn finish_clients(
    clients: Vec<Arc<Client>>,
    mode: ShutdownMode,
    request_id: String,
    held: &mut BTreeSet<super::remote::MemberKey>,
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
                        // A local launch overrides its next-start Runtime settings.
                        // Collect only typed member processes: a leftover browser
                        // page cannot hide the member's current remote work.
                        held.insert((
                            snapshot.actor.group_id.clone(),
                            snapshot.actor.actor_id.clone(),
                            snapshot.actor.actor_created_at.clone(),
                        ));
                        if super::preservation::positively_exited(&snapshot) {
                            continue;
                        }
                        if running(&snapshot) {
                            report
                                .retained
                                .push(identity(&snapshot.actor, &client.address.owner_id));
                        } else {
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
                                // An empty result cannot describe a failed stop of a login-only browser.
                                if (before.actors.is_empty() && before.browsers.is_empty())
                                    || before.browsers.iter().any(|browser| {
                                        browser.resource.state
                                            != super::browser::BrowserResourceState::Exited
                                            && browser.identities().next().is_none()
                                    })
                                {
                                    return Err(error);
                                }
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

pub(super) fn stop_offline(home: &HomeLayout) -> anyhow::Result<Option<ShutdownReport>> {
    let _lock = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))?
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "daemon still owns its home; cannot stop retained agents offline",
            )
        })?;
    // An ordinary stop withdraws an earlier Keep even when it fails, so that
    // receipt no longer hands its agents to an update.
    super::receipt::clear_shutdown_report(home)?;
    let (clients, discovery_error) = discover_all(home)?;
    // A holder can own a login window with no member in the shutdown report.
    if clients.is_empty() && discovery_error.is_none() {
        return Ok(None);
    }
    let stopped = finish_clients(
        clients.into_values().collect(),
        ShutdownMode::Stop,
        uuid::Uuid::new_v4().simple().to_string(),
        &mut BTreeSet::new(),
    );
    if let Some(error) = discovery_error {
        return Err(error.into());
    }
    let report = stopped?;
    super::receipt::write_shutdown_report(home, &report)?;
    Ok(Some(report))
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

fn terminal_exited(snapshot: &ActorSnapshot) -> bool {
    use crate::ops::actor_runtime::{OwnerIdentity, OwnerState};
    snapshot.observation_error.is_none()
        && snapshot.owner.as_ref().is_some_and(|owner| {
            matches!(owner.identity, OwnerIdentity::Pty { .. }) && owner.state == OwnerState::Exited
        })
}

pub(super) fn track_recovery(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    created: &str,
    event: &str,
    transport: &str,
    known_only: bool,
) -> io::Result<()> {
    let controller = for_home(home)?;
    if known_only
        && !controller
            .deliveries
            .lock()
            .map_err(poisoned)?
            .keys()
            .any(|source| {
                source.actor.group_id == group
                    && source.actor.actor_id == actor
                    && source.actor.actor_created_at == created
                    && source.event_ids.iter().any(|id| id == event)
            })
    {
        return Ok(());
    }
    let mut recoveries = controller.recoveries.lock().map_err(poisoned)?;
    if recoveries.len() >= 4096 {
        return Err(io::Error::other(
            "retained delivery recovery capacity is full",
        ));
    }
    recoveries.insert(
        (group.into(), actor.into(), created.into(), event.into()),
        (transport.into(), known_only),
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
) -> io::Result<Vec<super::DeliveryRecovery>> {
    let controller = for_home(home)?;
    Ok(controller
        .recoveries
        .lock()
        .map_err(poisoned)?
        .iter()
        .filter(|((group_id, _, _, _), _)| group_id == group)
        .map(|((_, actor, created, event), (transport, known_only))| {
            (
                actor.clone(),
                created.clone(),
                event.clone(),
                transport.clone(),
                *known_only,
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

    fn test_pty(path: &std::path::Path, actor: &ActorRef) -> PreparedLaunch {
        PreparedLaunch::Pty {
            spec: cccc_runtime::LaunchSpec {
                group_id: actor.group_id.clone(),
                actor_id: actor.actor_id.clone(),
                runner: cccc_contracts::RunnerKind::Pty,
                command: vec!["sleep".into(), "60".into()],
                cwd: path.into(),
                env: Default::default(),
                cols: 80,
                rows: 24,
            },
            history: cccc_runtime::HistoryConfig {
                path: path.join("history"),
                max_bytes: 4096,
                hot_bytes: 4096,
                persist: false,
            },
            launch: cccc_runtime::ActorLaunch {
                runtime: cccc_contracts::ActorRuntime::Custom,
                submit: cccc_contracts::Actor::new(&actor.actor_id).submit,
            },
            preamble: String::new(),
        }
    }

    #[test]
    fn uncertain_start_is_released_only_by_fenced_non_admission_or_retained_registration() {
        let temp = tempfile::tempdir().expect("fixture directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let _home = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))
            .expect("lock")
            .expect("fixture home exclusive");
        initialize(&home).expect("controller");
        let controller = for_home(&home).expect("fixture controller");
        let (client, _) = current_factory(&controller).expect("fault fixture owner");
        let actor = ActorRef {
            group_id: "g_uncertain_start".into(),
            actor_id: "peer".into(),
            actor_created_at: "created".into(),
            generation: None,
        };
        let mutation = || Mutation::Start {
            actor: actor.clone(),
            launch: Box::new(test_pty(temp.path(), &actor)),
        };
        let route = || {
            controller
                .registration(&actor.group_id, &actor.actor_id)
                .expect("route")
        };

        let blocked = client.mutate_tracked(mutation(), |operation| {
            controller.remember_start(&client, &actor, operation)?;
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "fixture before writing the frame",
            ))
        });
        assert!(blocked.is_err());
        assert!(resolve_start(&controller, &actor).is_err());
        assert!(route().expect("unconfirmed reservation").start.is_some());
        client.release().expect("close first control stream");
        resolve_start(&controller, &actor).expect("prove not admitted after fencing");
        assert!(route().is_none());

        let first = client
            .mutate_tracked(mutation(), |operation| {
                controller.remember_start(&client, &actor, operation)?;
                Ok(())
            })
            .expect("admit Start");
        client
            .wait(&first, Duration::from_secs(10))
            .expect("owner finished despite lost controller receipt");
        let pending = route().expect("in-flight reservation");
        assert_eq!(pending.pending_start(), Some(&first));
        client
            .release()
            .expect("close control stream after admission");
        observe_owners_for_maintenance(&home)
            .expect("recover the original owner registration without another Start");
        let current = route().expect("registered member");
        assert!(current.pending_start().is_none() && current.actor.generation.is_some());
        assert_eq!(
            client.query(&first).expect("acknowledged operation"),
            OperationState::Unknown
        );
        assert!(Arc::ptr_eq(&current.client, &client));
        let Err(duplicate) = start(&home, actor.clone(), test_pty(temp.path(), &actor)) else {
            panic!("member was started twice");
        };
        assert_eq!(duplicate.kind(), io::ErrorKind::AlreadyExists);
        finish(&home, ShutdownMode::Stop, "fixture-shutdown".into()).expect("clean fixture owner");
    }

    #[test]
    fn captured_owner_observation_keeps_equal_member_keys_in_their_own_homes() {
        let temp = tempfile::tempdir().expect("fixture directory");
        let actor = ActorRef {
            group_id: "g_shared_fixture".into(),
            actor_id: "peer".into(),
            actor_created_at: "created".into(),
            generation: None,
        };
        let run = |name| {
            let home = HomeLayout::from_path(temp.path().join(name)).expect("isolated home");
            home.initialize().expect("initialize");
            let lock = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))
                .expect("lock")
                .expect("fixture home exclusive");
            initialize(&home).expect("controller");
            let observed =
                start(&home, actor.clone(), test_pty(home.root(), &actor)).expect("start member");
            let pid = observed.terminal.expect("PTY").pid.expect("running PID");
            (home, lock, pid)
        };
        let (first, _first_lock, first_pid) = run("first");
        let (second, _second_lock, second_pid) = run("second");
        assert_ne!(first_pid, second_pid);
        for (home, pid) in [(&first, first_pid), (&second, second_pid)] {
            let observed = observe_owners_for_maintenance(home).expect("owner observations");
            let terminal = observed.with_scope(|| {
                snapshot(&actor.group_id, &actor.actor_id)
                    .expect("member observation")
                    .expect("member")
                    .terminal
                    .expect("terminal")
            });
            assert_eq!(terminal.pid, Some(pid));
            let mut replaced = cccc_contracts::Actor::new(&actor.actor_id);
            replaced.created_at = "a-different-creation".into();
            assert!(running_target(home, &actor.group_id, &replaced).is_err());
            finish(home, ShutdownMode::Stop, "fixture-shutdown".into())
                .expect("clean fixture owner");
        }
    }

    fn with_unreachable_owner(home: &HomeLayout, check: impl FnOnce(&str)) {
        home.initialize().expect("initialize fixture home");
        let _home = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))
            .expect("check fixture lock")
            .expect("isolated home lock");
        let id = uuid::Uuid::new_v4().simple().to_string();
        let client = Arc::new(client::start(home, &id).expect("start fixture owner"));
        client.retire().expect("retire empty owner");
        let path = Paths::new(home, &id).expect("fixture owner paths").address;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while path.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "fixture owner must exit"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        initialize(home).expect("install fixture controller");
        struct RemoveController(PathBuf);
        impl Drop for RemoveController {
            fn drop(&mut self) {
                controllers().lock().expect("registry lock").remove(&self.0);
            }
        }
        let _cleanup = RemoveController(home.root().into());
        for_home(home)
            .expect("get fixture controller")
            .clients
            .lock()
            .expect("owner registry")
            .insert(id.clone(), client);
        check(&id);
    }

    #[test]
    fn member_stop_preserves_the_group_intent_when_a_peer_is_unobservable() {
        let temp = tempfile::tempdir().expect("fixture directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("isolated home");
        let store = cccc_core::GroupStore::new(home.clone()).expect("fixture group store");
        let mut group = store
            .create("stop with unavailable peer", "")
            .expect("group");
        group.running = true;
        group.state = cccc_contracts::GroupState::Active;
        group.actors = ["stopped", "peer"]
            .map(|id| {
                let mut actor = cccc_contracts::Actor::new(id);
                actor.runtime = cccc_contracts::ActorRuntime::Custom;
                actor.normalize_runtime_constraints();
                actor
            })
            .into();
        store
            .save(&group)
            .expect("save Group with both members enabled");

        with_unreachable_owner(&home, |_| {
            use crate::ops::actor_runtime::persist_lifecycle;
            persist_lifecycle(&home, &group, "stopped", false, None).expect("save confirmed Stop");
            let after = store.load(&group.group_id).expect("reload saved intent");
            assert!(!after.actors[0].enabled);
            assert!(after.actors[1].enabled);
            assert!(
                after.running,
                "unavailable owner is not proof that the peer stopped"
            );
            assert_eq!(after.state, group.state);
            persist_lifecycle(&home, &after, "peer", false, None).expect("save last member Stop");
            assert!(
                !store
                    .load(&group.group_id)
                    .expect("reload final intent")
                    .running
            );
        });
    }

    #[test]
    fn lifecycle_refusals_preserve_resume_state_and_pending_deliveries() {
        let temp = tempfile::tempdir().expect("fixture directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("isolated home");
        let store = cccc_core::GroupStore::new(home.clone()).expect("fixture group store");
        let mut group = store
            .create("unavailable lifecycle preflight", "")
            .expect("group");
        group.state = cccc_contracts::GroupState::Active;
        group.running = true;
        let mut actor = cccc_contracts::Actor::new("peer");
        actor.runtime = cccc_contracts::ActorRuntime::Codex;
        actor.normalize_runtime_constraints();
        group.actors.push(actor);
        store.save(&group).expect("save Group");
        let receipts = store
            .state_dir(&group.group_id)
            .expect("group state")
            .join("runtime_sessions");
        std::fs::create_dir_all(&receipts).expect("fixture receipt directory");
        let receipt = receipts.join("peer.json");
        let original = serde_json::json!({"runtime":"codex","transport":"codex_app_server",
                                           "status":"resume_failed","resume_eligible":false});
        cccc_core::fs::write_json(&receipt, &original).expect("record resume failure");

        with_unreachable_owner(&home, |owner_id| {
            let request = |op: &str, args: serde_json::Value| {
                let request =
                    serde_json::from_value(serde_json::json!({"v":1,"op":op,"args":args}))
                        .expect("lifecycle fixture request");
                crate::dispatch::dispatch(&home, &request)
            };
            let delivery = request(
                "send",
                serde_json::json!({
                    "group_id":group.group_id,"by":"user","text":"waiting on the owner",
                    "to":["peer"],"message_mode":"send",
                }),
            );
            assert!(
                delivery.ok,
                "ledger send must succeed during the outage: {delivery:?}"
            );
            let pending = || {
                crate::ops::automation_runtime::pending_delivery_group_ids()
                    .contains(&group.group_id)
            };
            struct ClearDeliveries(String);
            impl Drop for ClearDeliveries {
                fn drop(&mut self) {
                    crate::ops::actor_delivery::shutdown_group(&self.0);
                }
            }
            let _clear_deliveries = ClearDeliveries(group.group_id.clone());
            assert!(pending(), "unavailable owner left a delivery gap");
            for op in ["actor_restart", "actor_stop", "actor_update"] {
                let mut args = serde_json::json!({
                    "group_id":group.group_id,"by":"user","actor_id":"peer"
                });
                if op == "actor_update" {
                    args["patch"] = serde_json::json!({"enabled":false});
                }
                let response = request(op, args);
                let error = response.error.expect("refused by unavailable owner");
                assert_eq!(error.code, "io_error");
                assert!(error.message.contains(owner_id));
                assert!(
                    pending(),
                    "a rejected {op} must leave pending delivery alone"
                );
                let saved: serde_json::Value =
                    cccc_core::fs::read_json(&receipt).expect("reload resume state");
                assert_eq!(
                    saved, original,
                    "a rejected {op} cannot change resume eligibility"
                );
                let persisted = store.load(&group.group_id).expect("reload Group");
                assert!(persisted.running && persisted.actors[0].enabled);
            }
        });
    }

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
        let result = start(&home, actor.clone(), test_pty(temp.path(), &actor));
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
                "stop-all".into(),
                &mut BTreeSet::new(),
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
