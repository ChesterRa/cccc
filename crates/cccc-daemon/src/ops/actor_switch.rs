//! Explicit runtime switch of one Actor: between native sessions, between a
//! native session and a CCCC-managed runtime, or between managed runtimes. The
//! member keeps its identity, tasks, Mail, requests, history and state; only
//! the owner that runs it changes.
//!
//! Order: preconditions; a delivery fence; stop the actual owner; settle its
//! handoffs; one Group commit that records the switch as pending; completion
//! (audit, handoff notice, private-env cleanup); then start the new owner.
//! While the record is pending the member receives no delivery and cannot be
//! started, switched or changed until completion succeeds.
use super::operation::{Operation, Policy::GlobalWrite};
use cccc_contracts::local_session::LocalSessionSource;
use cccc_contracts::{Actor, ActorSubmit, DaemonRequest, Event, GroupState, RuntimeMode, utc_now};
use cccc_core::runtime_switches::{self, Endpoint, SwitchRecord};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, Scope, actors, fs, group_scope};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, store};
use crate::ops::actor_delivery::{self, DeliveryFence, ShutdownReason};
use crate::ops::{actor_profile_runtime, actor_runtime, runtime_delivery};

mod completion;
#[cfg(all(test, unix))]
mod tests;

pub(crate) use completion::{complete_stranded, refuse_pending};

/// Managed configuration a switch target may set. Everything else, including
/// private values, is refused: they never travel with a switch request.
const MANAGED_PATCH_FIELDS: &[&str] = &[
    "runtime",
    "runtime_mode",
    "command",
    "env",
    "submit",
    "default_scope_key",
];

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "actor_runtime_switch" => Operation::new(GlobalWrite, switch),
        "actor_runtime_switch_complete" => Operation::new(GlobalWrite, complete),
        _ => return None,
    })
}

enum Target {
    Native {
        source_id: String,
        expected_native_session_id: String,
    },
    Managed {
        patch: Map<String, Value>,
        profile_id: String,
        profile_scope: String,
        profile_owner: String,
        expected_profile_revision: Option<u64>,
        start: bool,
    },
}

/// The member as it will be committed, and what the switch does around it.
struct Plan {
    actor: Actor,
    attach_scope: Option<Scope>,
    /// The native session as the user confirmed it; checked again at commit.
    source: Option<LocalSessionSource>,
    /// Completion removes the member's own private-env file: a discarded
    /// managed configuration, a native member's leftover, or a file dormant
    /// behind a Profile the member leaves. The member cannot start before
    /// completion, so the new configuration never sees those values, and a
    /// failure before the commit removes nothing.
    clear_private_env: bool,
}

/// Keeps other work off the member until its switch is committed and complete:
/// browser work in progress ends first, and work that starts next finds the new
/// owner. Released before anything starts, since a start takes these too.
struct Hold {
    _browser_work: std::fs::File,
    _fence: DeliveryFence,
}

fn switch(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    authorize(request)?;
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let expected_generation = required_arg(request, "expected_actor_generation")?;
    // Members saved before this field existed carry it empty; the value
    // must still be the one the caller read.
    let expected_updated_at = request
        .args
        .get("expected_actor_updated_at")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| OpError::new("invalid_args", "expected_actor_updated_at is required"))?;
    let target = parse_target(request)?;
    let store = store(home)?;
    // An earlier switch of this member must be completed, through Retry,
    // before another begins: completing it here would skip its start.
    refuse_pending(home, &group_id, &actor_id)?;
    let group = store.load(&group_id).map_err(OpError::not_found)?;
    let current = actors::find(&group, &actor_id)
        .cloned()
        .ok_or_else(|| OpError::new("actor_not_found", "actor not found"))?;
    if actors::generation_identity(&current) != expected_generation
        || current.updated_at != expected_updated_at
    {
        return Err(conflict(
            "This member changed after it was shown. Reload it and confirm the switch again.",
        ));
    }
    if !current.external_session_id.is_empty()
        && required_arg(request, "expected_source_id")? != current.external_session_id
    {
        return Err(conflict(
            "This member's session changed after it was shown. Reload it and confirm the switch again.",
        ));
    }
    let plan = match target {
        Target::Native {
            source_id,
            expected_native_session_id,
        } => plan_native(
            home,
            &group,
            &current,
            &source_id,
            &expected_native_session_id,
        )?,
        Target::Managed {
            patch,
            profile_id,
            profile_scope,
            profile_owner,
            expected_profile_revision,
            start,
        } => plan_managed(
            home,
            request,
            &group,
            &current,
            &patch,
            ProfileTarget {
                id: profile_id,
                scope: profile_scope,
                owner: profile_owner,
                revision: expected_profile_revision,
            },
            start,
        )?,
    };
    execute(home, request, &store, &group, &current, plan)
}

fn complete(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    authorize(request)?;
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let store = store(home)?;
    let group = store.load(&group_id).map_err(OpError::not_found)?;
    let actor = actors::find(&group, &actor_id)
        .ok_or_else(|| OpError::new("actor_not_found", "actor not found"))?;
    if runtime_switches::pending_for(&group, actor).is_none() {
        return object(json!({
            "group_id": group_id,
            "actor_id": actor_id,
            "switched": false,
            "completion": "done",
        }));
    }
    let completed = completion::complete_pending(home, &group_id, &actor_id)
        .map_err(completion::pending_error)?
        .ok_or_else(|| OpError::new("actor_switch_pending", "switch record changed"))?;
    let start = start_after_commit(
        home,
        request,
        &store,
        &group_id,
        &completed.record,
        completed.notice.as_ref(),
    );
    respond(
        &group_id,
        &actor_id,
        &completed.record,
        Some(&completed),
        start,
    )
}

fn authorize(request: &DaemonRequest) -> Result<(), OpError> {
    if request.args.contains_key("_cccc_web_binding") {
        return Err(OpError::new(
            "permission_denied",
            "Runtime switching is unavailable to cloud connectors",
        ));
    }
    super::profile_access::require_admin_user(request)
}

fn parse_target(request: &DaemonRequest) -> Result<Target, OpError> {
    let target = request
        .args
        .get("target")
        .and_then(Value::as_object)
        .ok_or_else(|| OpError::new("invalid_args", "target must be an object"))?;
    // A field of the wrong type is refused, never read as its default: a
    // malformed `start` must not start a runtime.
    for (name, valid) in [
        ("start", Value::is_boolean as fn(&Value) -> bool),
        ("expected_profile_revision", Value::is_u64),
        ("source_id", Value::is_string),
        ("expected_native_session_id", Value::is_string),
        ("profile_id", Value::is_string),
        ("profile_scope", Value::is_string),
        ("profile_owner", Value::is_string),
    ] {
        if target.get(name).is_some_and(|value| !valid(value)) {
            return Err(OpError::new(
                "invalid_args",
                format!("target.{name} has the wrong type"),
            ));
        }
    }
    let text = |name: &str| {
        target
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_owned()
    };
    match target.get("kind").and_then(Value::as_str) {
        Some("native") => {
            let source_id = text("source_id");
            let expected_native_session_id = text("expected_native_session_id");
            if source_id.is_empty() || expected_native_session_id.is_empty() {
                return Err(OpError::new(
                    "invalid_args",
                    "a native target needs source_id and expected_native_session_id",
                ));
            }
            Ok(Target::Native {
                source_id,
                expected_native_session_id,
            })
        }
        Some("managed") => {
            let patch = match target.get("patch") {
                None => Map::new(),
                Some(Value::Object(patch)) => patch.clone(),
                Some(_) => {
                    return Err(OpError::new(
                        "invalid_args",
                        "target.patch must be an object",
                    ));
                }
            };
            if let Some(field) = patch
                .keys()
                .find(|key| !MANAGED_PATCH_FIELDS.contains(&key.as_str()))
            {
                return Err(OpError::new(
                    "invalid_args",
                    format!("target.patch cannot set {field}"),
                ));
            }
            let profile_id = text("profile_id");
            let expected_profile_revision = target
                .get("expected_profile_revision")
                .and_then(Value::as_u64);
            if !profile_id.is_empty() && expected_profile_revision.is_none() {
                return Err(OpError::new(
                    "invalid_args",
                    "a Profile target needs expected_profile_revision",
                ));
            }
            let profile_scope = match text("profile_scope") {
                scope if scope.is_empty() => "global".into(),
                scope => scope,
            };
            Ok(Target::Managed {
                patch,
                profile_id,
                profile_scope,
                profile_owner: text("profile_owner"),
                expected_profile_revision,
                start: target.get("start").and_then(Value::as_bool).unwrap_or(true),
            })
        }
        _ => Err(OpError::new(
            "invalid_args",
            "target.kind must be native or managed",
        )),
    }
}

fn plan_native(
    home: &HomeLayout,
    group: &GroupDoc,
    current: &Actor,
    source_id: &str,
    expected_native_session_id: &str,
) -> Result<Plan, OpError> {
    if current.external_session_id == source_id {
        return Err(unsupported("This member already uses that session."));
    }
    let source: LocalSessionSource = super::local_sessions::switch_source(home, source_id)?;
    if source.native_session_id != expected_native_session_id {
        return Err(OpError::new(
            "local_session_source_changed",
            "That session now shows a different conversation. Reload the list and choose again.",
        ));
    }
    if let Some(other) = group
        .actors
        .iter()
        .find(|actor| actor.id != current.id && actor.external_session_id == source_id)
    {
        let mut error = OpError::new(
            "local_session_source_in_use",
            format!(
                "That session already runs member {} in this Group.",
                other.id
            ),
        );
        error.details.insert("actor_id".into(), json!(other.id));
        return Err(error);
    }
    let scope = super::local_sessions::switch_scope(&source, &group.scopes)?;
    let mut actor = current.clone();
    let was_managed = actor.external_session_id.is_empty();
    // A managed configuration is discarded, not kept dormant: a native member
    // must not carry a launch command, environment or Profile link.
    clear_managed_configuration(&mut actor);
    if actor.title.is_empty() || automatic_title(home, current) {
        actor.title.clone_from(&source.title);
    }
    actor.external_session_id = source.source_id.clone();
    actor.runtime = super::local_sessions::provider(source.runtime);
    actor.default_scope_key.clone_from(&scope.scope_key);
    actor.normalize_runtime_constraints();
    actor.updated_at = utc_now();
    Ok(Plan {
        actor,
        attach_scope: Some(scope),
        source: Some(source),
        clear_private_env: was_managed,
    })
}

struct ProfileTarget {
    id: String,
    scope: String,
    owner: String,
    revision: Option<u64>,
}

fn plan_managed(
    home: &HomeLayout,
    request: &DaemonRequest,
    group: &GroupDoc,
    current: &Actor,
    patch: &Map<String, Value>,
    profile: ProfileTarget,
    start: bool,
) -> Result<Plan, OpError> {
    let leaving_native = !current.external_session_id.is_empty();
    let leaving_profile = !current.profile_id.is_empty() && profile.id.is_empty();
    let mut base = current.clone();
    if leaving_native {
        if automatic_title(home, current) {
            base.title.clear();
        }
        base.external_session_id.clear();
    }
    if !base.profile_id.is_empty() {
        // The Profile controlled the command, environment, submit key and
        // mode; their stored values are dormant and must not come back. The
        // target names the whole configuration, linked or custom.
        clear_managed_configuration(&mut base);
    }
    let mut patch = patch.clone();
    if let Some(value) = patch.get("default_scope_key") {
        let reference = value
            .as_str()
            .ok_or_else(|| OpError::new("invalid_args", "default_scope_key must be a string"))?
            .trim();
        // Empty follows the Group's active directory, as for `actor_update`.
        let scope = if reference.is_empty() {
            String::new()
        } else {
            group_scope::resolve_attached_scope(group, reference)
                .map(|scope| scope.scope_key.clone())
                .ok_or_else(|| {
                    OpError::new(
                        "scope_not_attached",
                        "The selected working directory is not attached to this Group",
                    )
                })?
        };
        patch.insert("default_scope_key".into(), Value::String(scope));
    }
    let mut actor = apply_patch(&base, &patch)?;
    if !profile.id.is_empty() {
        actor.profile_scope.clone_from(&profile.scope);
        actor.profile_owner.clone_from(&profile.owner);
        let mut linked = actor.clone();
        linked.profile_id.clone_from(&profile.id);
        super::profile_access::require_actor_link(home, request, &linked)?;
        let stored = cccc_core::profiles::ProfileStore::new(home.clone())
            .map_err(OpError::io)?
            .get_ref(&profile.id, &profile.scope, &profile.owner)
            .map_err(OpError::io)?
            .ok_or_else(|| OpError::new("profile_not_found", "profile not found"))?;
        if stored.get("revision").and_then(Value::as_u64) != profile.revision {
            return Err(conflict(
                "The selected Profile changed after it was shown. Reload it and confirm the switch again.",
            ));
        }
        actor = actor_profile_runtime::link(home, &actor, &profile.id)?;
    }
    actor
        .runtime_mode
        .validate(actor.runtime)
        .map_err(OpError::invalid)?;
    actor.normalize_runtime_constraints();
    // An explicit start enables a stopped managed member, as "save and
    // restart" did; a native member whose access was paused stays paused
    // until the user resumes it.
    actor.enabled = start && (current.enabled || !leaving_native);
    // Refused here: at the start, the old owner has already stopped. A stopped
    // member that follows the Group's directory does not need one yet, and a
    // Web Model runs nothing in one.
    if !actor.runtime.is_web_model()
        && (actor.enabled || !actor.default_scope_key.trim().is_empty())
    {
        actor_runtime::working_directory(group, &actor)?;
    }
    actor.updated_at = utc_now();
    Ok(Plan {
        actor,
        attach_scope: None,
        source: None,
        clear_private_env: leaving_native || leaving_profile,
    })
}

fn execute(
    home: &HomeLayout,
    request: &DaemonRequest,
    store: &GroupStore,
    group: &GroupDoc,
    current: &Actor,
    plan: Plan,
) -> OpResult {
    let group_id = group.group_id.clone();
    let actor_id = current.id.clone();
    let record = SwitchRecord {
        op_id: uuid::Uuid::new_v4().simple().to_string(),
        actor_id: actor_id.clone(),
        actor_generation: actors::generation_identity(current),
        from: Endpoint::of(current),
        to: Endpoint::of(&plan.actor),
        at: utc_now(),
        pending: true,
        clear_private_env: plan.clear_private_env,
    };
    let hold = Hold {
        _browser_work: actor_runtime::hold_browser_work(home, &group_id, &actor_id)?,
        _fence: actor_delivery::fence_actor(&group_id, &actor_id),
    };
    actor_delivery::shutdown_actor_for(&group_id, &actor_id, ShutdownReason::Replace);
    let was_running = match actor_runtime::stop_owner_confirmed(home, group, &actor_id) {
        Ok(was_running) => was_running,
        // An owner that cannot be confirmed stopped keeps the member; its
        // state is unknown, so it is neither reported stopped nor restarted.
        Err(error) => return Err(incomplete(home, request, current, None, hold, error)),
    };
    if let Err(error) = super::runtime_state::settle_active_turn(home, group, current)
        .and_then(|()| runtime_delivery::settle_replaced_owner_claims(home, group, &actor_id))
    {
        return Err(incomplete(
            home,
            request,
            current,
            Some(was_running),
            hold,
            error,
        ));
    }
    match commit(home, store, &group_id, current, &plan, &record) {
        Ok(Ok(())) => {}
        // Nothing was written: the session changed after the user confirmed.
        Ok(Err(rejected)) => {
            return Err(incomplete(
                home,
                request,
                current,
                Some(was_running),
                hold,
                rejected,
            ));
        }
        Err(error) => match store.load(&group_id) {
            Ok(doc)
                if runtime_switches::records(&doc)
                    .iter()
                    .any(|stored| stored.op_id == record.op_id) => {}
            Ok(doc) if actors::find(&doc, &actor_id) == Some(current) => {
                return Err(incomplete(
                    home,
                    request,
                    current,
                    Some(was_running),
                    hold,
                    OpError::io(error),
                ));
            }
            _ => {
                drop(hold);
                return Err(OpError::new(
                    "actor_switch_unknown",
                    format!(
                        "The switch result could not be confirmed ({error}). Reload the member before trying again."
                    ),
                ));
            }
        },
    }
    let completed = completion::complete_pending(home, &group_id, &actor_id);
    drop(hold);
    match completed {
        Ok(completed) => {
            let notice = completed.as_ref().and_then(|done| done.notice.as_ref());
            let start = start_after_commit(home, request, store, &group_id, &record, notice);
            respond(&group_id, &actor_id, &record, completed.as_ref(), start)
        }
        Err(error) => {
            tracing::warn!(
                group_id = %group_id,
                actor_id = %actor_id,
                message = %error.message,
                "runtime switch committed; completion is pending"
            );
            respond(&group_id, &actor_id, &record, None, StartReport::held())
        }
    }
}

fn commit(
    home: &HomeLayout,
    store: &GroupStore,
    group_id: &str,
    current: &Actor,
    plan: &Plan,
    record: &SwitchRecord,
) -> std::io::Result<Result<(), OpError>> {
    // CLI processes do not take the daemon's dispatch permits; native binding
    // changes also hold the lifecycle file lock they use.
    fs::with_exclusive_lock(&home.root().join("state/local_sessions.lock"), || {
        // Registrations change only under this lock: the session must still
        // show the conversation and directory the user confirmed.
        if let Some(confirmed) = &plan.source
            && let Err(error) = still_confirmed(home, confirmed)
        {
            return Ok(Err(error));
        }
        store.mutate(group_id, |doc| {
            let index = doc
                .actors
                .iter()
                .position(|actor| actor == current)
                .ok_or_else(|| std::io::Error::other("the member changed during the switch"))?;
            let source = &plan.actor.external_session_id;
            if !source.is_empty()
                && doc
                    .actors
                    .iter()
                    .any(|actor| actor.id != current.id && &actor.external_session_id == source)
            {
                return Err(std::io::Error::other("the session joined another member"));
            }
            if let Some(scope) = &plan.attach_scope
                && !doc
                    .scopes
                    .iter()
                    .any(|known| known.scope_key == scope.scope_key)
            {
                doc.scopes.push(scope.clone());
            }
            doc.actors[index] = plan.actor.clone();
            if current.runtime != plan.actor.runtime {
                actors::retire_web_model_browser_target(doc, &current.id)?;
            }
            replace_conversation_source(doc, &current.external_session_id, source);
            let mut records = runtime_switches::records(doc);
            if !source.is_empty() {
                // Returning to a source the member left lifts that record.
                records.retain(|stored| {
                    !(stored.actor_id == current.id
                        && stored.left_native()
                        && &stored.from.source_id == source)
                });
            }
            records.push(record.clone());
            runtime_switches::set_records(doc, &records);
            runtime_switches::prune(doc);
            if !plan.actor.enabled && current.enabled {
                // As for an explicit stop: no enabled member, no running Group.
                doc.running = doc.running && doc.actors.iter().any(|actor| actor.enabled);
            }
            Ok(())
        })?;
        Ok(Ok(()))
    })
}

fn still_confirmed(home: &HomeLayout, confirmed: &LocalSessionSource) -> Result<(), OpError> {
    let source = super::local_sessions::switch_source(home, &confirmed.source_id)?;
    if source.native_session_id != confirmed.native_session_id || source.cwd != confirmed.cwd {
        return Err(OpError::new(
            "local_session_source_changed",
            "That session changed while the switch was in progress. Reload the list and choose again.",
        ));
    }
    Ok(())
}

/// The direct-conversation reuse index names the native sources a Group was
/// created for. A member's switch keeps it truthful.
fn replace_conversation_source(doc: &mut GroupDoc, old: &str, new: &str) {
    if old.is_empty() {
        return;
    }
    let Some(ids) = doc
        .extra
        .get_mut("local_session_conversation")
        .and_then(|value| value.get_mut("source_ids"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    if new.is_empty() {
        ids.retain(|id| id.as_str() != Some(old));
    } else {
        for id in ids.iter_mut() {
            if id.as_str() == Some(old) {
                *id = Value::String(new.to_owned());
            }
        }
        ids.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        ids.dedup();
    }
}

/// Restore what can be restored after a failure before the commit, and say
/// exactly what happened to the previous owner.
fn incomplete(
    home: &HomeLayout,
    request: &DaemonRequest,
    current: &Actor,
    was_running: Option<bool>,
    hold: Hold,
    cause: OpError,
) -> OpError {
    drop(hold);
    let group = store(home).and_then(|store| {
        store
            .load(&lifecycle_group_id(request))
            .map_err(OpError::io)
    });
    // An owner that could not be confirmed stopped leaves delivery as it is,
    // even for a member saved as native.
    let old_owner = match (group, was_running) {
        (Err(_), _) | (_, None) => "unknown",
        (Ok(group), _) if !current.external_session_id.is_empty() => {
            actor_delivery::dispatch_unread(home, &group, &current.id);
            "native"
        }
        (Ok(_), Some(true)) => {
            match super::actors::lifecycle(home, &lifecycle_request(request), "actor.start") {
                Ok(_) => "restarted",
                Err(_) => "restart_failed",
            }
        }
        // No process says how it stands: its conversation may still be working,
        // and the credentials it held are already gone.
        (Ok(_), Some(false)) if current.runtime.is_web_model() => "web_model",
        (Ok(_), Some(false)) => "not_running",
    };
    let mut error = OpError::new(
        "actor_switch_incomplete",
        format!(
            "The runtime was not switched and the member's configuration is unchanged: {}",
            cause.message
        ),
    );
    error.details.insert("old_owner".into(), json!(old_owner));
    error.details.insert("cause".into(), json!(cause.code));
    error
}

enum StartKind {
    Started,
    Pending,
    Stopped,
    Error,
    Native,
    Held,
}

struct StartReport {
    kind: StartKind,
    error: Option<OpError>,
    access_paused: bool,
}

impl StartReport {
    const fn held() -> Self {
        Self {
            kind: StartKind::Held,
            error: None,
            access_paused: false,
        }
    }

    const fn of(kind: StartKind) -> Self {
        Self {
            kind,
            error: None,
            access_paused: false,
        }
    }
}

/// The switch is committed and stays committed: a failure from here on is
/// reported as a start that did not happen, never as a switch that did not.
fn start_after_commit(
    home: &HomeLayout,
    request: &DaemonRequest,
    store: &GroupStore,
    group_id: &str,
    record: &SwitchRecord,
    notice: Option<&Event>,
) -> StartReport {
    match store.load(group_id) {
        Ok(group) => start_new_owner(
            home,
            request,
            &group,
            &record.actor_id,
            &record.from,
            notice,
        ),
        Err(error) => StartReport {
            kind: StartKind::Error,
            error: Some(OpError::io(error)),
            access_paused: false,
        },
    }
}

fn start_new_owner(
    home: &HomeLayout,
    request: &DaemonRequest,
    group: &GroupDoc,
    actor_id: &str,
    from: &Endpoint,
    notice: Option<&Event>,
) -> StartReport {
    let Some(actor) = actors::find(group, actor_id) else {
        return StartReport {
            kind: StartKind::Error,
            error: Some(OpError::new("actor_not_found", "actor not found")),
            access_paused: false,
        };
    };
    if !actor.external_session_id.is_empty() {
        super::local_sessions::record_instruction_baseline(home, group, actor);
        if !actor.enabled {
            return StartReport {
                kind: StartKind::Native,
                error: None,
                access_paused: true,
            };
        }
        deliver_native(home, group, actor, notice);
        return StartReport::of(StartKind::Native);
    }
    if !actor.enabled {
        return StartReport::of(StartKind::Stopped);
    }
    // A member leaving a native session starts with its Group. A managed
    // member's switch is its "save and restart": it starts now, as an explicit
    // restart does, and runs its Group again.
    if from.is_native()
        && (!group.running || matches!(group.state, GroupState::Paused | GroupState::Stopped))
    {
        return StartReport::of(StartKind::Pending);
    }
    // The handoff goes ahead of the backlog the start dispatches.
    match super::actors::lifecycle_leading(home, &lifecycle_request(request), "actor.start", notice)
    {
        Ok(_) => StartReport::of(StartKind::Started),
        Err(error) => StartReport {
            kind: StartKind::Error,
            error: Some(error),
            access_paused: false,
        },
    }
}

/// The handoff comes first; the backlog follows by ordinary rules.
fn deliver_native(home: &HomeLayout, group: &GroupDoc, actor: &Actor, notice: Option<&Event>) {
    if let Some(notice) = notice {
        actor_delivery::dispatch_to(home, group, notice, std::slice::from_ref(actor), false);
    }
    actor_delivery::dispatch_unread(home, group, &actor.id);
}

fn respond(
    group_id: &str,
    actor_id: &str,
    record: &SwitchRecord,
    completed: Option<&completion::Completed>,
    start: StartReport,
) -> OpResult {
    let mut result = json!({
        "group_id": group_id,
        "actor_id": actor_id,
        "op_id": record.op_id,
        "switched": true,
        "from": record.from,
        "to": record.to,
        "completion": if completed.is_some() { "done" } else { "pending" },
        "start": match start.kind {
            StartKind::Started => "started",
            StartKind::Pending => "pending",
            StartKind::Stopped => "stopped",
            StartKind::Error => "error",
            StartKind::Native => "native",
            StartKind::Held => "held",
        },
    });
    if let Some(error) = start.error {
        result["start_error"] = json!({"code": error.code, "message": error.message});
    }
    if start.access_paused {
        result["access_paused"] = json!(true);
    }
    if let Some(notice) = completed.and_then(|done| done.notice.as_ref()) {
        result["notice_event_id"] = json!(notice.id);
    }
    object(result)
}

fn lifecycle_request(request: &DaemonRequest) -> DaemonRequest {
    let mut args = Map::new();
    for key in ["group_id", "actor_id", "by", "caller_id", "is_admin"] {
        if let Some(value) = request.args.get(key) {
            args.insert(key.into(), value.clone());
        }
    }
    DaemonRequest {
        v: request.v,
        op: "actor_start".into(),
        args,
    }
}

fn lifecycle_group_id(request: &DaemonRequest) -> String {
    request
        .args
        .get("group_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Apply managed fields the way `actor_update` does, without its rule that a
/// native binding never changes: the switch owns that change.
fn apply_patch(base: &Actor, patch: &Map<String, Value>) -> Result<Actor, OpError> {
    let mut value = serde_json::to_value(base).map_err(OpError::invalid)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| OpError::new("internal_error", "invalid actor"))?;
    for (key, item) in patch {
        object.insert(key.clone(), item.clone());
    }
    if patch
        .get("runtime")
        .is_some_and(|runtime| runtime != &json!(base.runtime))
        && !patch.contains_key("runtime_mode")
    {
        object.insert("runtime_mode".into(), json!(RuntimeMode::default()));
    }
    serde_json::from_value(value).map_err(OpError::invalid)
}

fn clear_managed_configuration(actor: &mut Actor) {
    actor.command.clear();
    actor.env.clear();
    actor.submit = ActorSubmit::default();
    actor.runtime_mode = RuntimeMode::default();
    actor.profile_id.clear();
    actor.profile_scope = "global".into();
    actor.profile_owner.clear();
    actor.profile_revision_applied = 0;
}

/// A native member's title that still equals its session's own title.
fn automatic_title(home: &HomeLayout, actor: &Actor) -> bool {
    !actor.external_session_id.is_empty()
        && super::local_sessions::source_title(home, &actor.external_session_id)
            .is_some_and(|title| title == actor.title)
}

fn conflict(message: &str) -> OpError {
    OpError::new("actor_switch_conflict", message)
}

fn unsupported(message: &str) -> OpError {
    OpError::new("actor_switch_unsupported", message)
}
