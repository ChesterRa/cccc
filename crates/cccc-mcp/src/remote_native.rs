//! Native source routing is explicit and never falls back across machines.
use super::*;
use cccc_contracts::{
    local_session::LocalSessionRuntime,
    session_node::{LEGACY_DEVICE_SESSION_RUNTIMES, supports_device_runtime},
};
use cccc_core::local_sessions::NativeHost;

pub(super) async fn route(
    home: &HomeLayout,
    source: &cccc_core::local_sessions::NativeSession,
    hub: Option<&str>,
    name: &str,
    args: &Map<String, Value>,
) -> std::result::Result<Option<Value>, ToolCallError> {
    let group_id = args
        .get("group_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if hub.is_none()
        && (group_id.is_empty() || !LEGACY_DEVICE_SESSION_RUNTIMES.contains(&source.runtime))
    {
        return Ok(None);
    }
    let Some(saved) = cccc_core::session_nodes::saved_client(home).map_err(|error| {
        ToolCallError::new(
            "session_node_connector_state_unreadable",
            format!(
                "{error}. Check its permissions or restore the saved device connector state before retrying"
            ),
            Map::new(),
        )
    })? else {
        if hub.is_some() {
            return Err("This native host has no remote-session connector".into());
        }
        return Ok(None);
    };
    let remote = saved
        .memberships
        .iter()
        .filter(|membership| {
            membership.source_id == source.source_id
                && membership.source_revision == source.revision
                && membership.group_id == group_id
        })
        .collect::<Vec<_>>();
    if hub.is_none() && remote.is_empty() {
        return Ok(None);
    }
    if hub.is_some_and(|hub| hub != saved.endpoint.endpoint_id) {
        return Err("Unknown remote hub_id; select it from cccc_session list".into());
    }
    if remote.len() != 1 {
        return Err(
            "This native source has no unambiguous membership in the selected remote Group".into(),
        );
    }
    let local = cccc_core::GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .is_ok_and(|group| {
            group
                .actors
                .iter()
                .any(|actor| actor.external_session_id == source.source_id)
        });
    if hub.is_none() && local {
        return Err("Group ID exists locally and remotely; pass hub_id from cccc_session list to select the remote Group".into());
    }
    if !remote[0].enabled {
        return Err("Remote Group access is paused; ask the user to resume this member".into());
    }
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}});
    let response =
        cccc_client::session_node::call(&saved, &source.source_id, &source.revision, request)
            .await
            .map_err(|error| format!("Remote-session tool failed: {error}"))?;
    response
        .get("result")
        .cloned()
        .map(Some)
        .ok_or_else(|| "Remote-session tool returned no result".into())
}

pub(super) fn append_memberships(
    home: &HomeLayout,
    source_id: Option<&str>,
    opencode_server: Option<&NativeHost>,
    result: &mut Value,
) {
    let Some(source_id) = source_id else {
        return;
    };
    let Ok(source) = cccc_core::local_sessions::read(home, source_id) else {
        return;
    };
    if !supports_device_runtime(source.runtime)
        || !source.host_alive()
        || (source.runtime == LocalSessionRuntime::Opencode
            && source.opencode_server.as_ref() != opencode_server)
    {
        return;
    }
    let client = match cccc_core::session_nodes::saved_client(home) {
        Ok(Some(client)) => client,
        Ok(None) => return,
        Err(error) => {
            cccc_runtime::diagnostic(format_args!("Device memberships unavailable: {error}"));
            return;
        }
    };
    let online = client.owner.alive() && client.connected;
    if let Some(memberships) = result
        .get_mut("self_memberships")
        .and_then(Value::as_array_mut)
    {
        memberships.extend(
            client
                .memberships
                .iter()
                .filter(|membership| {
                    membership.source_id == source_id
                        && membership.source_revision == source.revision
                })
                .map(|membership| {
                    let mut value = serde_json::to_value(membership).unwrap_or_default();
                    value["remote"] = json!(true);
                    value["connected"] = json!(online);
                    value
                }),
        );
    }
}

pub(super) fn authorize(
    home: &HomeLayout,
    binding: &cccc_contracts::local_session::LocalSessionBinding,
    name: &str,
    args: &Map<String, Value>,
) -> std::result::Result<(), ToolCallError> {
    if !cccc_core::session_nodes::is_remote_source(&binding.source_id) {
        return Ok(());
    }
    cccc_core::local_sessions::validate_binding(home, binding)
        .map_err(|_| "Remote membership is no longer valid")?;
    // Deliberately narrower than CORE_TOOL_NAMES (which also exposes files,
    // Connect and capability dispatch). Admission here never replaces the
    // daemon's parameter-specific Actor/Group/message authorization.
    const COLLABORATION: &[&str] = &[
        "cccc_bootstrap",
        "cccc_help",
        "cccc_message_send",
        "cccc_message_reply",
        "cccc_message_deliver",
        "cccc_reply_request_cancel",
        "cccc_message_history",
        "cccc_inbox_read",
        "cccc_inbox_mark_read",
        "cccc_context_get",
        "cccc_context_sync",
        "cccc_coordination",
        "cccc_agent_state",
        "cccc_task",
        "cccc_backlog",
        "cccc_actor_notes",
        "cccc_todo",
        "cccc_session",
    ];
    if !COLLABORATION.contains(&name) {
        return Err("This LAN connector exposes CCCC collaboration tools only. Use the original native application's tools for files, commands and approvals on the remote machine".into());
    }
    if args
        .get("refs")
        .and_then(Value::as_array)
        .is_some_and(|refs| !refs.is_empty())
    {
        return Err("File references are not transferred by the LAN session connector; send text or use the existing file transfer connection".into());
    }
    Ok(())
}

pub(super) async fn session(
    home: &HomeLayout,
    client: &DaemonClient,
    binding: &cccc_contracts::local_session::LocalSessionBinding,
    mut args: Map<String, Value>,
) -> std::result::Result<Value, ToolCallError> {
    authorize(home, binding, "cccc_session", &args)?;
    let action = args
        .remove("action")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "list".into());
    let op = match action.as_str() { "send" => "local_session_send", "leave" => "local_session_leave", _ => return Err("Remote cccc_session supports send and leave in an explicitly selected Group; discovery and admission are administrator actions".into()) };
    args.remove("source_ids");
    args.insert("group_id".into(), json!(binding.group_id));
    args.insert("by".into(), json!(binding.actor_id));
    args.insert("source_id".into(), json!(binding.source_id));
    args.insert("_cccc_native_binding".into(), json!(binding));
    if op == "local_session_send" {
        let default = crate::argument_normalization::default_new_message_mode(&args, true);
        let mode = crate::argument_normalization::message_mode(&mut args, default)?;
        args.insert("message_mode".into(), json!(mode));
        args.insert("require_peer_insight".into(), json!(true));
    }
    Ok(router::tool_result(Value::Object(
        router::daemon(client, op, args).await?,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::local_sessions::NativeSession;

    fn fixture_source(home: &HomeLayout) -> NativeSession {
        NativeSession {
            source_id: "codex:thread".into(),
            runtime: LocalSessionRuntime::Codex,
            native_session_id: "thread".into(),
            host: NativeHost::capture(std::process::id()).expect("fixture host"),
            cwd: home.root().to_string_lossy().into_owned(),
            title: "Local fixture".into(),
            origin: None,
            revision: "fixture".into(),
            context_epoch: 0,
            codex_home: None,
            codex_can_receive: false,
            codex_program: None,
            claude_config_dir: None,
            pi_agent_dir: None,
            opencode_config_dir: None,
            opencode_server: None,
            retired: None,
        }
    }

    #[tokio::test]
    async fn unreadable_connector_preserves_only_routes_known_to_be_local() {
        let temp = tempfile::tempdir().expect("isolated home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let path = home.root().join("state/session_nodes/client.json");
        std::fs::create_dir_all(path.parent().expect("connector directory")).expect("directory");
        std::fs::write(&path, "{").expect("unreadable connector");

        let mut source = fixture_source(&home);
        let group = json!({"group_id":"g_fixture"});
        let arguments = group.as_object().expect("Group arguments");
        for runtime in [LocalSessionRuntime::Pi, LocalSessionRuntime::Opencode] {
            source.runtime = runtime;
            assert!(
                route(&home, &source, None, "cccc_message_history", arguments)
                    .await
                    .expect("implicit Group routing remains local")
                    .is_none()
            );
            let error = route(
                &home,
                &source,
                Some("remote-hub"),
                "cccc_message_history",
                arguments,
            )
            .await
            .expect_err("explicit remote routing requires a readable connector");
            assert_eq!(
                error.error_value()["code"],
                "session_node_connector_state_unreadable"
            );
        }
        for runtime in [LocalSessionRuntime::Codex, LocalSessionRuntime::Claude] {
            source.runtime = runtime;
            assert!(
                route(&home, &source, None, "cccc_session", &Map::new())
                    .await
                    .expect("local action without a Group")
                    .is_none()
            );
            for hub in [None, Some("remote-hub")] {
                let error = route(&home, &source, hub, "cccc_message_history", arguments)
                    .await
                    .expect_err("cannot determine the target machine");
                let error = error.error_value();
                assert_eq!(error["code"], "session_node_connector_state_unreadable");
                assert!(
                    error["message"]
                        .as_str()
                        .expect("message")
                        .contains("state/session_nodes/client.json")
                );
            }
        }
        source.runtime = LocalSessionRuntime::Codex;
        let source = cccc_core::local_sessions::write(&home, source).expect("local registration");
        let mut listing = json!({"self_memberships":[{"group_id":"g_local"}]});
        let before = listing.clone();
        append_memberships(&home, Some(&source.source_id), None, &mut listing);
        assert_eq!(
            listing, before,
            "unreadable connector retains local memberships"
        );
    }

    #[test]
    fn opencode_remote_memberships_require_its_registered_backend() {
        use cccc_contracts::session_node::{SessionNodeEndpoint, SessionNodeMembership};
        use cccc_core::session_nodes::{self, ClientDescriptor};

        let temp = tempfile::tempdir().expect("isolated home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let mut source = fixture_source(&home);
        source.runtime = LocalSessionRuntime::Opencode;
        source.native_session_id = "ses_fixture".into();
        source.source_id = source.runtime.source_id(&source.native_session_id);
        source.opencode_server = Some(source.host.clone());
        let source = cccc_core::local_sessions::write(&home, source).expect("native registration");
        let client = ClientDescriptor {
            endpoint: SessionNodeEndpoint {
                endpoint_id: "hub".into(),
                addresses: vec![],
                relay_url: None,
            },
            node_id: "node".into(),
            grant_id: "grant".into(),
            secret: "fixture-secret".into(),
            owner: source.host.clone(),
            address: "127.0.0.1:1".parse().expect("loopback"),
            connected: true,
            memberships: vec![SessionNodeMembership {
                hub_id: "hub".into(),
                source_id: source.source_id.clone(),
                source_revision: source.revision,
                group_id: "g_device".into(),
                group_title: "Device Group".into(),
                actor_id: "member".into(),
                actor_generation: "generation".into(),
                role: "peer".into(),
                scope_key: String::new(),
                enabled: true,
            }],
        };
        session_nodes::set_client(&home, &client).expect("connector state");
        let mut listing = json!({"self_memberships":[{"group_id":"g_local"}]});
        let local = listing.clone();
        let other_backend = NativeHost {
            started: "another-start".into(),
            ..source.host.clone()
        };
        for backend in [None, Some(&other_backend)] {
            append_memberships(&home, Some(&source.source_id), backend, &mut listing);
            assert_eq!(listing, local);
        }
        append_memberships(
            &home,
            Some(&source.source_id),
            source.opencode_server.as_ref(),
            &mut listing,
        );
        assert_eq!(listing["self_memberships"][0]["group_id"], "g_local");
        assert_eq!(listing["self_memberships"][1]["group_id"], "g_device");
        assert_eq!(listing["self_memberships"][1]["hub_id"], "hub");
    }
}
