use super::*;
use cccc_contracts::{Event, local_session::LocalSessionSource};
use cccc_core::{actors, ledger};
use serde_json::Map;
use std::collections::BTreeSet;

const CONVERSATION_KEY: &str = "local_session_conversation";

fn selected_source_ids(request: &DaemonRequest) -> Result<Vec<String>, OpError> {
    let mut source_ids = request
        .args
        .get("source_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| OpError::new("invalid_args", "source_ids must be an array"))?
        .iter()
        .map(|v| {
            v.as_str().map(str::to_owned).ok_or_else(|| {
                OpError::new("invalid_args", "source_ids must contain source identifiers")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(caller) = string_arg(request, "source_id") {
        source_ids.insert(0, caller);
    }
    let mut ids = BTreeSet::new();
    source_ids.retain(|id| ids.insert(id.clone()));
    if ids.is_empty() || ids.len() > 16 {
        return Err(OpError::new(
            "invalid_args",
            "Select between one and sixteen local sessions",
        ));
    }
    Ok(source_ids)
}

fn resolve_sources(
    home: &HomeLayout,
    source_ids: &[String],
) -> Result<Vec<LocalSessionSource>, OpError> {
    let directory = super::store::sources(home)?;
    source_ids
        .iter()
        .map(|id| {
            directory
                .iter()
                .find(|source| &source.source_id == id && source.connected)
                .cloned()
                .ok_or_else(|| {
                    OpError::new(
                        "local_session_offline",
                        format!("The selected source is not connected: {id}"),
                    )
                })
        })
        .collect()
}

fn resolve_scopes(
    sources: &[LocalSessionSource],
    attached: &[cccc_core::Scope],
) -> Result<Vec<cccc_core::Scope>, OpError> {
    let mut scopes = Vec::<cccc_core::Scope>::new();
    for source in sources {
        let scope =
            cccc_core::scope::detect(std::path::Path::new(&source.cwd)).map_err(OpError::io)?;
        if let Some(existing) = attached
            .iter()
            .chain(&scopes)
            .find(|existing| existing.scope_key == scope.scope_key && existing.url != scope.url)
        {
            return Err(OpError::new(
                "local_session_scope_conflict",
                format!(
                    "These native sessions use different checkouts of the same project ({} and {}). A Group cannot represent both working directories; connect them in separate Groups.",
                    existing.url, scope.url
                ),
            ));
        }
        scopes.push(scope);
    }
    Ok(scopes)
}

pub(super) fn create(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    local(request)?;
    let source_ids = selected_source_ids(request)?;
    let direct = request
        .args
        .get("direct")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let ids = source_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if direct {
        for meta in store.list().map_err(OpError::io)? {
            let group = match store.load(&meta.group_id) {
                Ok(group) => group,
                Err(error) => {
                    tracing::warn!(group_id = %meta.group_id, %error, "Skipping unavailable Group during native-session reuse");
                    continue;
                }
            };
            if group
                .extra
                .get(CONVERSATION_KEY)
                .is_some_and(|v| v["direct"] == true && v["source_ids"] == json!(ids))
                && ids.iter().all(|id| {
                    group
                        .actors
                        .iter()
                        .any(|a| &a.external_session_id == id && a.enabled)
                })
            {
                return object(
                    json!({"group_id":group.group_id,"group":super::super::group_runtime::group(home, group),"reused":true}),
                );
            }
        }
    }
    // Actor order determines coordination roles. Preserve the initiating source
    // or the human's selection order; only the reuse key is a sorted set.
    let sources = resolve_sources(home, &source_ids)?;
    let scopes = resolve_scopes(&sources, &[])?;
    let title = string_arg(request, "title")
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| {
            sources
                .iter()
                .map(|s| s.title.clone())
                .collect::<Vec<_>>()
                .join(" · ")
        });
    let created = store
        .create(&title, "Local native-session collaboration")
        .map_err(OpError::io)?;
    let result = populate(
        &store,
        &created,
        &sources,
        &scopes,
        &ids,
        direct,
        string_arg(request, "source_id").as_deref(),
    );
    match result {
        Ok(group) => object(
            json!({"group_id":group.group_id,"group":super::super::group_runtime::group(home, group),"reused":false}),
        ),
        Err(mut error) => {
            if let Err(cleanup) = store.delete(&created.group_id) {
                error
                    .details
                    .insert("cleanup_error".into(), json!(cleanup.to_string()));
            }
            Err(error)
        }
    }
}

pub(super) fn join(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    local(request)?;
    if request.args.contains_key("source_id")
        || string_arg(request, "by").is_some_and(|by| by != "user")
    {
        return Err(OpError::new(
            "permission_denied",
            "Joining an existing Group requires an explicit user selection",
        ));
    }
    let group_id = required_arg(request, "group_id")?;
    let source_ids = selected_source_ids(request)?;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = store.load(&group_id).map_err(OpError::not_found)?;
    let sources = resolve_sources(home, &source_ids)?
        .into_iter()
        .filter(|source| {
            !group
                .actors
                .iter()
                .any(|actor| actor.external_session_id == source.source_id)
        })
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return object(
            json!({"group_id":group_id,"group":super::super::group_runtime::group(home, group),"reused":true}),
        );
    }
    // Validate the entire selection before changing membership or attaching any
    // scopes. Neither a new directory nor a new source changes Group defaults.
    let scopes = resolve_scopes(&sources, &group.scopes)?;
    let path = store.ledger_path(&group_id).map_err(OpError::io)?;
    store
        .mutate_with_rollback(
            &group_id,
            |group| {
                for scope in &scopes {
                    if !group.scopes.iter().any(|s| s.scope_key == scope.scope_key) {
                        group.scopes.push(scope.clone());
                    }
                }
                add_sources(group, &sources, &scopes)
                    .map_err(|error| std::io::Error::other(error.message))
            },
            |added| {
                for actor in added {
                    let mut event = Event::new("actor.add", &group_id);
                    event.by = "user".into();
                    event.data.insert("actor".into(), json!(actor));
                    ledger::append(&path, &event)?;
                }
                Ok(())
            },
        )
        .map_err(OpError::io)?;
    let group = store.load(&group_id).map_err(OpError::io)?;
    object(
        json!({"group_id":group_id,"group":super::super::group_runtime::group(home, group),"reused":false}),
    )
}

fn add_sources(
    group: &mut GroupDoc,
    sources: &[LocalSessionSource],
    scopes: &[cccc_core::Scope],
) -> Result<Vec<Actor>, OpError> {
    let mut added = Vec::new();
    for (source, scope) in sources.iter().zip(scopes) {
        let mut index = 1;
        let id = loop {
            let id = format!("{}-{index}", source.runtime.as_str());
            if actors::find(group, &id).is_none() {
                break id;
            }
            index += 1;
        };
        let mut actor = Actor::new(id);
        actor.runtime = actor_runtime(source.runtime);
        actor.external_session_id = source.source_id.clone();
        actor.title.clone_from(&source.title);
        actor.default_scope_key.clone_from(&scope.scope_key);
        added.push(actors::add(group, actor).map_err(OpError::invalid)?);
    }
    Ok(added)
}

fn populate(
    store: &GroupStore,
    created: &GroupDoc,
    sources: &[LocalSessionSource],
    scopes: &[cccc_core::Scope],
    ids: &[String],
    direct: bool,
    origin: Option<&str>,
) -> Result<GroupDoc, OpError> {
    let mut group = created.clone();
    group.running = true;
    group.extra.insert(
        CONVERSATION_KEY.into(),
        json!({"direct":direct,"source_ids":ids}),
    );
    // Native working directories become attached scopes without changing the
    // registry's project defaults or the user's globally selected Group.
    for scope in scopes {
        if !group.scopes.iter().any(|s| s.scope_key == scope.scope_key) {
            group.scopes.push(scope.clone());
        }
    }
    group.active_scope_key = group
        .scopes
        .first()
        .map(|s| s.scope_key.clone())
        .unwrap_or_default();
    let added = add_sources(&mut group, sources, scopes)?;
    store.save(&group).map_err(OpError::io)?;
    let path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    let mut event = Event::new("group.create", &group.group_id);
    event.by = if origin.is_some() {
        "svc:cccc.local-session"
    } else {
        "user"
    }
    .into();
    event.data = json!({"title":group.title,"topic":group.topic})
        .as_object()
        .cloned()
        .expect("object");
    if let Some(source_id) = origin {
        event.data.insert("source_id".into(), json!(source_id));
    }
    let by = event.by.clone();
    ledger::append(&path, &event).map_err(OpError::io)?;
    for actor in added {
        let mut event = Event::new("actor.add", &group.group_id);
        event.by.clone_from(&by);
        event.data.insert("actor".into(), json!(actor));
        if let Some(source_id) = origin {
            event.data.insert("source_id".into(), json!(source_id));
        }
        ledger::append(&path, &event).map_err(OpError::io)?;
    }
    Ok(group)
}

pub(super) fn send(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    local(request)?;
    if required_arg(request, "text")?.trim().is_empty() {
        return Err(OpError::new("invalid_args", "text must not be empty"));
    }
    if let Some(mode) = string_arg(request, "message_mode")
        && !matches!(mode.as_str(), "send" | "request_reply" | "mail")
    {
        return Err(OpError::new("invalid_args", "Invalid message_mode"));
    }
    let explicit_group = string_arg(request, "group_id").filter(|id| !id.is_empty());
    let source_directed = explicit_group.is_none();
    if (source_directed && request.args.contains_key("to"))
        || (!source_directed && request.args.contains_key("source_ids"))
    {
        return Err(OpError::new(
            "invalid_args",
            "Use source_ids for a direct send, or group_id with optional to for a Group send; do not combine addressing modes",
        ));
    }
    let group_id = if let Some(id) = explicit_group {
        id
    } else {
        let mut create_request = request.clone();
        create_request
            .args
            .insert("direct".into(), Value::Bool(true));
        create(home, &create_request)?
            .get("group_id")
            .and_then(Value::as_str)
            .expect("created Group ID")
            .to_owned()
    };
    let group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&group_id)
        .map_err(OpError::not_found)?;
    let source_id = string_arg(request, "source_id").unwrap_or_default();
    let by = if source_id.is_empty() {
        "user".to_owned()
    } else {
        group
            .actors
            .iter()
            .find(|a| a.external_session_id == source_id && a.enabled)
            .ok_or_else(|| {
                OpError::new(
                    "permission_denied",
                    "This source is not a member of the conversation",
                )
            })?
            .id
            .clone()
    };
    let mut args = request
        .args
        .iter()
        .filter(|(k, _)| {
            !matches!(
                k.as_str(),
                "source_id" | "source_ids" | "direct" | "title" | "by" | "group_id"
            )
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<Map<_, _>>();
    args.insert("group_id".into(), json!(group_id));
    args.insert("by".into(), json!(by));
    args.entry("to").or_insert_with(|| {
        if !source_directed {
            return json!(["@all"]);
        }
        let selected = request
            .args
            .get("source_ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<BTreeSet<_>>();
        json!(
            group
                .actors
                .iter()
                .filter(
                    |actor| actor.enabled && selected.contains(actor.external_session_id.as_str())
                )
                .map(|actor| &actor.id)
                .collect::<Vec<_>>()
        )
    });
    args.entry("message_mode").or_insert(json!("send"));
    let response = super::super::messaging::send_local_session(
        home,
        &DaemonRequest {
            op: "send".into(),
            args,
            ..request.clone()
        },
    )?;
    let mut result = response;
    result.insert("group_id".into(), json!(group_id));
    Ok(result)
}

pub(super) fn leave(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    local(request)?;
    let group_id = required_arg(request, "group_id")?;
    let source_id = required_arg(request, "source_id")?;
    let group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&group_id)
        .map_err(OpError::not_found)?;
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.external_session_id == source_id)
        .ok_or_else(|| {
            OpError::new(
                "permission_denied",
                "This source is not a member of the conversation",
            )
        })?;
    let mut forwarded = request.clone();
    forwarded.args.insert("actor_id".into(), json!(actor.id));
    forwarded.args.insert("by".into(), json!(actor.id));
    super::super::actors::lifecycle(home, &forwarded, "actor.stop")
}
