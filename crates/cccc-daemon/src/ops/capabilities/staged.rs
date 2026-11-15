//! Short authorization/commit phases around external capability work. Workers
//! own immutable inputs and unpublished candidates; only a live coordinator can
//! publish, after validating the original caller again.

use cccc_contracts::{Actor, DaemonRequest};
use cccc_core::capabilities::{CapabilityStore, revocations};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg};
use crate::dispatch_concurrency::DispatchLocks;
use crate::ops::operation::validate_bindings;

use super::{credentials, package_install, recipe, target_install};

#[cfg(test)]
mod tests;

pub(crate) fn handles(request: &DaemonRequest) -> bool {
    request.op == "capability_enable"
        || (request.op == "capability_tool_call"
            && request
                .args
                .get("tool_name")
                .and_then(Value::as_str)
                .is_some_and(|name| !name.is_empty()))
}

pub(crate) async fn run(
    home: &HomeLayout,
    request: &DaemonRequest,
    locks: &DispatchLocks,
) -> OpResult {
    match request.op.as_str() {
        "capability_enable" => enable(home, request, locks).await,
        "capability_tool_call" => call(home, request, locks).await,
        _ => Err(OpError::new(
            "unknown_op",
            "not a staged capability operation",
        )),
    }
}

/// In-process callers share the production coordinator. A connection supplies
/// its shared locks through `run`; a synchronous caller owns no outer permit.
pub(super) fn run_sync(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let locks = DispatchLocks::default();
    crate::ops::local_headless::block_on_managed(run(home, request, &locks))
}

async fn worker<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, OpError> + Send + 'static,
) -> Result<T, OpError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| OpError::new("dispatch_failed", "capability worker did not complete"))?
}

async fn phase<T: Send + 'static>(
    home: &HomeLayout,
    request: &DaemonRequest,
    locks: &DispatchLocks,
    mut work: impl FnOnce(&HomeLayout, &DaemonRequest) -> Result<T, OpError> + Send + 'static,
) -> Result<T, OpError> {
    loop {
        let permit = locks.acquire_for(home, request).await;
        let owned_home = home.clone();
        let owned_request = request.clone();
        let attempt = worker(move || {
            let _permit = permit;
            validate_bindings(&owned_home, &owned_request)?;
            // Existing background launches hold this gate and may need MCP
            // reads. Never wait for one while retaining a dispatch permit.
            let Some(_start) =
                crate::runtime_start_gate::try_permit(&owned_home).map_err(|_| shutting_down())?
            else {
                return Ok(Err(work));
            };
            // The phase borrows the gate only to order its work before stop.
            // A request panic must not poison the unrelated start-gate map.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                work(&owned_home, &owned_request)
            }));
            drop(_start);
            match result {
                Ok(result) => Ok(Ok(result?)),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        })
        .await?;
        match attempt {
            Ok(result) => return Ok(result),
            Err(returned_work) => work = returned_work,
        }
        let owned_home = home.clone();
        worker(move || {
            // This wait owns no dispatch permit, and releases the gate before
            // the next acquisition. No authorization, I/O or write is replayed.
            let _start =
                crate::runtime_start_gate::permit(&owned_home).map_err(|_| shutting_down())?;
            Ok(())
        })
        .await?;
    }
}

fn shutting_down() -> OpError {
    OpError::new("daemon_shutting_down", "daemon is shutting down")
}

struct Authority {
    group: String,
    generation: String,
    created_at: String,
    actors: Vec<(String, Option<Actor>)>,
}

impl Authority {
    fn capture(group: &GroupDoc, ids: &[&str]) -> Self {
        Self {
            group: group.group_id.clone(),
            generation: group.generation.clone(),
            created_at: group.created_at.clone(),
            actors: ids
                .iter()
                .map(|id| {
                    (
                        (*id).to_owned(),
                        group.actors.iter().find(|a| a.id == *id).cloned(),
                    )
                })
                .collect(),
        }
    }

    fn check(&self, home: &HomeLayout) -> Result<GroupDoc, OpError> {
        let group = cccc_core::GroupStore::new(home.clone())
            .map_err(OpError::io)?
            .load(&self.group)
            .map_err(|_| denied())?;
        if group.generation != self.generation || group.created_at != self.created_at {
            return Err(denied());
        }
        for (id, before) in &self.actors {
            let now = group.actors.iter().find(|actor| &actor.id == id);
            if before.as_ref().map(|a| (&a.generation, &a.created_at))
                != now.map(|a| (&a.generation, &a.created_at))
            {
                return Err(denied());
            }
        }
        Ok(group)
    }

    fn configuration_matches(&self, group: &GroupDoc) -> bool {
        self.actors.iter().all(|(id, before)| {
            let now = group.actors.iter().find(|actor| &actor.id == id);
            // Activity timestamps do not change execution or authority. Keep
            // all actual member configuration, including runtime and profile.
            let stable = |actor: &Actor| {
                let mut actor = actor.clone();
                actor.updated_at.clear();
                actor
            };
            before.as_ref().map(stable) == now.map(stable)
        })
    }
}

fn denied() -> OpError {
    OpError::new(
        "permission_denied",
        "the original capability caller is no longer valid",
    )
}

fn conflict(reason: &str) -> OpError {
    let mut error = OpError::new(
        "capability_conflict",
        "capability preparation was superseded; review the current settings before trying again",
    );
    error.details.insert("reason".into(), json!(reason));
    error
}

fn recipe_id(record: &Value) -> Result<Option<String>, OpError> {
    if executable(record) {
        recipe::fingerprint(record).map(Some)
    } else {
        Ok(None)
    }
}

fn executable(record: &Value) -> bool {
    matches!(
        record["install_mode"].as_str(),
        Some("command" | "package" | "remote_only")
    )
}

fn execution_record(record: &Value, artifact: Option<&Value>) -> Result<Value, OpError> {
    // A declared Bearer remote recipe is public metadata only: prepare does not
    // connect or start a process for it. Project that same candidate now so the
    // credential HMAC uses exactly the later installed execution form, including
    // legacy install_spec.auth normalization.
    let candidate;
    let artifact = if artifact.is_none()
        && credentials::bearer(record)
        && record["install_mode"] == "remote_only"
    {
        candidate =
            package_install::prepare(record["capability_id"].as_str().unwrap_or(""), record)?;
        Some(&candidate)
    } else {
        artifact
    };
    let mut runtime = json!({});
    if let Some(artifact) = artifact {
        let id = record["capability_id"].as_str().unwrap_or("");
        let key = artifact["artifact_id"].as_str().unwrap_or("");
        runtime["artifacts"][key] = artifact.clone();
        runtime["capability_artifacts"][id] = json!(key);
    }
    Ok(credentials::execution_record(record, &runtime))
}

fn artifact_identity(artifact: Option<&Value>) -> Value {
    artifact.map_or(Value::Null, |a| {
        json!([a["artifact_id"], a["recipe_fingerprint"], a["invoker"]])
    })
}

struct Enable {
    id: String,
    authority: Authority,
    access: super::ScopeMutation,
    record: Value,
    fingerprint: Option<String>,
    installed: Option<Value>,
    revision: revocations::Snapshot,
    credential: Option<credentials::Identity>,
}

enum EnableStart {
    Done(Map<String, Value>),
    Prepare(Box<Enable>),
}

async fn enable(home: &HomeLayout, request: &DaemonRequest, locks: &DispatchLocks) -> OpResult {
    let start = phase(home, request, locks, |home, request| {
        if !bool_arg(request, "enabled", true) {
            return super::disable(home, request).map(EnableStart::Done);
        }
        let id = required_arg(request, "capability_id")?;
        let access = super::authorize_scope_mutation(home, request, "session")?;
        let store = CapabilityStore::new(home.clone());
        if let Some(blocked) = target_install::preflight_enable(
            &store,
            &access.actor.group_id,
            &access.actor.actor_id,
            &access.scope,
            &id,
        )? {
            return super::finish_enable(blocked, &id, &access, &action_id(), None)
                .map(EnableStart::Done);
        }
        store.require(&id).map_err(OpError::not_found)?;
        let record = if CapabilityStore::builtin(&id).is_some() {
            json!({})
        } else {
            store
                .resolved_record(&id)
                .map_err(OpError::io)?
                .unwrap_or(Value::Null)
        };
        let installed = package_install::installed_artifact(home, &id)?;
        let execution = execution_record(&record, installed.as_ref())?;
        let member_prepare = access.scope != "group"
            && bool_arg(request, "prepare", true)
            && request.args.get("by").and_then(Value::as_str) == Some(access.actor.by.as_str())
            && !matches!(access.actor.by.as_str(), "user" | "system")
            && access.actor.by == access.actor.actor_id
            && credentials::bearer(&execution);
        let credential = if member_prepare {
            Some(credentials::load_identity(
                home,
                &access.actor.group_id,
                &access.actor.by,
                &execution,
            )?)
        } else {
            None
        };
        let authority = Authority::capture(
            &access.actor.group,
            &[&access.actor.by, &access.actor.actor_id],
        );
        let revision =
            revocations::snapshot(home, &id, &access.actor.group_id, &access.actor.actor_id);
        Ok(EnableStart::Prepare(Box::new(Enable {
            id,
            authority,
            fingerprint: recipe_id(&record)?,
            access,
            record,
            installed,
            revision,
            credential,
        })))
    })
    .await?;
    let plan = match start {
        EnableStart::Done(result) => return Ok(result),
        EnableStart::Prepare(plan) => plan,
    };
    let work_home = home.clone();
    // Move the plan with the worker, so cancelled coordinators cannot retain a
    // second publisher. The worker only returns data; it never writes state.
    let (plan, candidate, discovery) = worker(move || {
        let candidate = if plan.installed.is_none() && executable(&plan.record) {
            Some(package_install::prepare_guarded(
                &work_home,
                &plan.id,
                &plan.record,
            )?)
        } else {
            None
        };
        let discovery = plan
            .credential
            .as_ref()
            .map(|identity| credentials::discover_unpublished(identity, &plan.id));
        Ok((plan, candidate, discovery))
    })
    .await?;
    phase(home, request, locks, move |home, request| {
        let group = plan.authority.check(home)?;
        let current = super::authorize_scope_mutation(home, request, "session")?;
        if !plan.authority.configuration_matches(&group) || current.scope != plan.access.scope {
            return Err(conflict("member_configuration_changed"));
        }
        if !plan.revision.is_current() {
            return Err(conflict("revoked"));
        }
        let store = CapabilityStore::new(home.clone());
        if target_install::preflight_enable(
            &store,
            &current.actor.group_id,
            &current.actor.actor_id,
            &current.scope,
            &plan.id,
        )?
        .is_some()
        {
            return Err(conflict("policy_changed"));
        }
        let record = if CapabilityStore::builtin(&plan.id).is_some() {
            json!({})
        } else {
            store
                .resolved_record(&plan.id)
                .map_err(OpError::io)?
                .unwrap_or(Value::Null)
        };
        if recipe_id(&record)? != plan.fingerprint {
            return Err(conflict("recipe_changed"));
        }
        let installed = package_install::installed_artifact(home, &plan.id)?;
        if artifact_identity(installed.as_ref()) != artifact_identity(plan.installed.as_ref()) {
            // Only a first-install race may reuse an equivalent winner. Never
            // replace that winner's tools with this worker's older discovery.
            if plan.installed.is_some()
                || installed.is_none()
                || artifact_identity(installed.as_ref()) != artifact_identity(candidate.as_ref())
            {
                return Err(conflict("installation_changed"));
            }
        }
        if let Some(identity) = &plan.credential {
            let execution = execution_record(&record, installed.as_ref().or(candidate.as_ref()))?;
            let now = credentials::load_identity(
                home,
                &current.actor.group_id,
                &current.actor.by,
                &execution,
            )?;
            if !credentials::same_identity(identity, &now) {
                return Err(conflict("credential_changed"));
            }
        }
        // Failed discovery may update same-context diagnostic metadata, but it
        // must not enable the capability or publish a shared installation.
        let member_tools = if let Some(pending) = discovery {
            match pending.result {
                Ok(tools) => Some(tools),
                Err(error) => {
                    credentials::remember(
                        plan.credential.as_ref().expect("discovery identity"),
                        Vec::new(),
                        pending.failure,
                    );
                    return Err(error);
                }
            }
        } else {
            None
        };
        if installed.is_none()
            && let Some(candidate) = candidate
        {
            package_install::persist(home, &plan.id, &candidate)?;
        }
        let ttl = request
            .args
            .get("ttl_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(3600);
        let result = target_install::enable_prepared(
            home,
            &current.actor.group_id,
            &current.actor.actor_id,
            &current.scope,
            ttl,
            &plan.id,
        )?;
        if let Some(tools) = &member_tools {
            credentials::remember(
                plan.credential.as_ref().expect("discovery identity"),
                tools.clone(),
                None,
            );
        }
        super::finish_enable(result, &plan.id, &current, &action_id(), member_tools)
    })
    .await
}

fn action_id() -> String {
    format!("cact_{}", &uuid::Uuid::new_v4().simple().to_string()[..16])
}

struct Call {
    id: String,
    actor: String,
    authority: Authority,
    record: Value,
    fingerprint: Option<String>,
    installation: Value,
    revision: revocations::Snapshot,
    credential: Option<credentials::Identity>,
    invoker: Map<String, Value>,
    artifact_id: String,
    name: String,
    real: String,
    arguments: Map<String, Value>,
    notes: Map<String, Value>,
}

impl Call {
    fn capture(home: &HomeLayout, request: &DaemonRequest) -> Result<Self, OpError> {
        let access = super::actor_context(home, request)?;
        let actor = super::external_runtime::caller_actor_id(request);
        let name = required_arg(request, "tool_name")?;
        let arguments = request
            .args
            .get("arguments")
            .or_else(|| request.args.get("args"))
            .cloned()
            .unwrap_or_else(|| json!({}))
            .as_object()
            .cloned()
            .ok_or_else(|| OpError::new("invalid_args", "tool arguments must be an object"))?;
        let store = CapabilityStore::new(home.clone());
        let native = store.load().map_err(OpError::io)?;
        let effective = super::effective_state::load(home, &access.group_id, &actor, &native)
            .map_err(OpError::io)?;
        let enabled = effective
            .enabled
            .difference(&effective.blocked)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let explicit =
            crate::dispatch::string_arg(request, "capability_id").filter(|id| !id.is_empty());
        if explicit.as_ref().is_some_and(|id| !enabled.contains(id)) {
            return Err(tool_missing(&name));
        }
        let hint = explicit.clone().or_else(|| {
            credentials::known_capability(home, &access.group_id, &actor, &enabled, &name)
        });
        let hint = match hint {
            Some(id) => Some(id),
            None => super::external_runtime::cold_bearer_capability(home, &store, &enabled, &name)?,
        };
        let runtime = super::external_runtime::load(home)?;
        let hinted = hint
            .as_deref()
            .map(|id| store.resolved_record(id).map_err(OpError::io))
            .transpose()?
            .flatten();
        let execution = hinted
            .as_ref()
            .map(|record| credentials::execution_record(record, &runtime));
        let bearer = execution.as_ref().is_some_and(credentials::bearer);
        let (id, real, credential) = if bearer {
            if request.args.get("by").and_then(Value::as_str) != Some(actor.as_str()) {
                return Err(OpError::new(
                    "credential_route_not_supported",
                    "member credential calls require an explicit bound caller",
                ));
            }
            let id = hint.expect("bearer hint");
            let identity = credentials::load_identity(
                home,
                &access.group_id,
                &actor,
                execution.as_ref().expect("execution recipe"),
            )?;
            let real = find_real(&credentials::tools_for(&identity), &name).unwrap_or_default();
            (id, real, Some(identity))
        } else {
            let enabled = enabled.into_iter().collect::<Vec<_>>();
            let tools = super::external_runtime::all_dynamic_tools(
                home,
                &access.group_id,
                &actor,
                &enabled,
            )?;
            let tool = tools
                .iter()
                .find(|tool| {
                    tool["name"] == name
                        || explicit.as_deref().is_some_and(|id| {
                            tool["capability_id"] == id && tool["real_tool_name"] == name
                        })
                })
                .ok_or_else(|| tool_missing(&name))?;
            let id = tool["capability_id"].as_str().unwrap_or("").to_owned();
            if explicit.as_ref().is_some_and(|explicit| explicit != &id) {
                return Err(OpError::new(
                    "capability_tool_not_found",
                    "tool does not belong to the requested capability",
                ));
            }
            (
                id,
                tool["real_tool_name"].as_str().unwrap_or("").to_owned(),
                None,
            )
        };
        let record = store
            .resolved_record(&id)
            .map_err(OpError::io)?
            .unwrap_or(Value::Null);
        let installation = runtime["capability_artifacts"][&id].clone();
        let binding =
            runtime["actor_instances"][&access.group_id][&actor][&id]["artifact_id"].clone();
        let artifact_id = binding
            .as_str()
            .or_else(|| installation.as_str())
            .unwrap_or("")
            .to_owned();
        let artifact = &runtime["artifacts"][&artifact_id];
        let invoker = if credential.is_some() {
            Map::new()
        } else {
            artifact["invoker"].as_object().cloned().ok_or_else(|| {
                OpError::new(
                    "capability_runtime_unavailable",
                    "external capability invoker is missing",
                )
            })?
        };
        let revision = revocations::snapshot(home, &id, &access.group_id, &actor);
        let notes =
            cccc_core::capability_recommendations::notes(&store.require(&id).map_err(OpError::io)?);
        Ok(Self {
            authority: Authority::capture(&access.group, &[&actor]),
            fingerprint: recipe_id(&record)?,
            id,
            actor,
            record,
            installation,
            revision,
            credential,
            invoker,
            artifact_id,
            name,
            real,
            arguments,
            notes,
        })
    }

    /// Identity loss is an error, not a skipped-write success. A replacement
    /// member must never receive the original caller's private external result.
    fn check_authority(
        &self,
        home: &HomeLayout,
        request: &DaemonRequest,
    ) -> Result<GroupDoc, OpError> {
        validate_bindings(home, request)?;
        let group = self.authority.check(home)?;
        super::actor_context(home, request)?;
        Ok(group)
    }

    fn changed(
        &self,
        home: &HomeLayout,
        group: &GroupDoc,
    ) -> Result<Option<&'static str>, OpError> {
        if !self.authority.configuration_matches(group) {
            return Ok(Some("member_configuration_changed"));
        }
        if !self.revision.is_current() {
            return Ok(Some("revoked"));
        }
        let store = CapabilityStore::new(home.clone());
        let native = store.load().map_err(OpError::io)?;
        let effective =
            super::effective_state::load(home, &self.authority.group, &self.actor, &native)
                .map_err(OpError::io)?;
        if !effective.enabled.contains(&self.id) || effective.blocked.contains(&self.id) {
            return Ok(Some("capability_disabled"));
        }
        let record = store
            .resolved_record(&self.id)
            .map_err(OpError::io)?
            .unwrap_or(Value::Null);
        if recipe_id(&record)? != self.fingerprint {
            return Ok(Some("recipe_changed"));
        }
        let runtime = super::external_runtime::load(home)?;
        let current_binding = runtime["actor_instances"][&self.authority.group][&self.actor]
            [&self.id]["artifact_id"]
            .as_str()
            .or_else(|| runtime["capability_artifacts"][&self.id].as_str())
            .unwrap_or("");
        if runtime["capability_artifacts"][&self.id] != self.installation
            || current_binding != self.artifact_id
        {
            return Ok(Some("installation_changed"));
        }
        if let Some(identity) = &self.credential {
            let record = credentials::execution_record(&record, &runtime);
            let current =
                credentials::load_identity(home, &self.authority.group, &self.actor, &record);
            if !current.is_ok_and(|current| credentials::same_identity(identity, &current)) {
                return Ok(Some("credential_changed"));
            }
        } else if runtime["artifacts"][&self.artifact_id]["invoker"]
            != Value::Object(self.invoker.clone())
        {
            return Ok(Some("installation_changed"));
        }
        Ok(None)
    }
}

fn find_real(tools: &[Value], name: &str) -> Option<String> {
    tools
        .iter()
        .find(|tool| tool["name"] == name || tool["real_tool_name"] == name)
        .and_then(|tool| tool["real_tool_name"].as_str())
        .map(str::to_owned)
}

fn tool_missing(name: &str) -> OpError {
    OpError::new(
        "capability_tool_not_found",
        format!(
            "tool not found or not enabled: {name}; prepare with cccc_capability_use(capability_id=...) before retrying a direct tool name"
        ),
    )
}

fn uncertain(mut error: OpError) -> OpError {
    error
        .details
        .insert("external_action_may_have_occurred".into(), json!(true));
    error.details.insert("automatic_retry".into(), json!(false));
    error
}

async fn call(home: &HomeLayout, request: &DaemonRequest, locks: &DispatchLocks) -> OpResult {
    let mut plan = phase(home, request, locks, Call::capture).await?;
    if plan.credential.is_some() && plan.real.is_empty() {
        let (prepared, pending) = worker(move || {
            let pending = credentials::discover_unpublished(
                plan.credential.as_ref().expect("credential"),
                &plan.id,
            );
            Ok((plan, pending))
        })
        .await?;
        plan = phase(home, request, locks, move |home, request| {
            let group = prepared.check_authority(home, request)?;
            if let Some(reason) = prepared.changed(home, &group)? {
                return Err(conflict(reason));
            }
            let identity = prepared.credential.as_ref().expect("credential");
            credentials::remember(
                identity,
                pending.result.as_ref().ok().cloned().unwrap_or_default(),
                pending.failure,
            );
            let tools = pending.result?;
            let mut prepared = prepared;
            prepared.real =
                find_real(&tools, &prepared.name).ok_or_else(|| tool_missing(&prepared.name))?;
            Ok(prepared)
        })
        .await?;
    }
    // No credential discovery or shared-state write occurs in the worker. A
    // successful admission permits one call, never a replay after revocation.
    let work_home = home.clone();
    let (plan, outcome) = worker(move || {
        let outcome = if let Some(identity) = &plan.credential {
            credentials::call_unpublished(identity, &plan.real, plan.arguments.clone())
        } else {
            credentials::CallOutcome {
                result: super::external_runtime::invoke_guarded(
                    &work_home,
                    &plan.invoker,
                    &plan.real,
                    json!(plan.arguments),
                    &plan.record,
                ),
                failure: None,
            }
        };
        Ok((plan, outcome))
    })
    .await
    .map_err(uncertain)?;
    phase(home, request, locks, move |home, request| {
        let group = plan.check_authority(home, request).map_err(uncertain)?;
        let mut changed = match plan.changed(home, &group) {
            Ok(changed) => changed,
            Err(error) => {
                tracing::warn!(code=%error.code, "capability call state could not be revalidated");
                Some("state_unavailable")
            }
        };
        if changed.is_none() {
            if let (Some(identity), Some(code)) = (&plan.credential, outcome.failure) {
                credentials::remember(identity, Vec::new(), Some(code));
            }
            let (state, message) = match &outcome.result {
                Ok(result) if result["isError"] == true => ("tool_call_failed", "external tool returned isError"),
                Ok(_) => ("verified", ""),
                Err(error) => ("tool_call_failed", error.message.as_str()),
            };
            // Another group's call may update unrelated entries under the same
            // file lock; no global writer is needed for this short metadata step.
            if let Err(error) = super::external_runtime::record_call_state(home, &plan.authority.group, &plan.actor, &plan.id, if plan.credential.is_some() { "" } else { &plan.artifact_id }, state, message) {
                tracing::warn!(code=%error.code, "failed to persist capability call state");
                changed = Some("state_persistence_failed");
            }
        }
        let result = outcome.result.map_err(|mut error| {
            if let Some(reason) = changed {
                error.details.insert("state_update_skipped".into(), json!(true));
                error.details.insert("state_update_skipped_reason".into(), json!(reason));
            }
            uncertain(error)
        })?;
        let mut payload = json!({"capability_id":plan.id,"tool_name":plan.name,"real_tool_name":plan.real,"result":result});
        let map = payload.as_object_mut().expect("tool response");
        map.extend(plan.notes);
        if let Some(reason) = changed {
            map.insert("state_update_skipped".into(), json!(true));
            map.insert("state_update_skipped_reason".into(), json!(reason));
        }
        object(payload)
    }).await.map_err(uncertain)
}
