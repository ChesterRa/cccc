//! Source-aware stdio mode. Native IDs never fall back to the active Group.
use super::*;
use std::collections::BTreeSet;

pub(super) struct Context {
    runtime: String,
    host_source_id: Option<String>,
    opencode_server: Option<cccc_core::local_sessions::NativeHost>,
}

impl Context {
    pub fn new(runtime: &str, host_pid: Option<u32>) -> Result<Self> {
        anyhow::ensure!(
            matches!(runtime, "codex" | "claude" | "pi" | "opencode"),
            "Unsupported native session runtime"
        );
        Ok(Self {
            runtime: runtime.into(),
            opencode_server: if runtime == "opencode" {
                Some(cccc_core::local_sessions::NativeHost::parent(
                    std::process::id(),
                )?)
            } else {
                None
            },
            host_source_id: if !matches!(runtime, "codex" | "opencode") {
                let host = match host_pid {
                    Some(pid) => cccc_core::local_sessions::NativeHost::capture(pid)?,
                    None => {
                        anyhow::ensure!(
                            runtime != "claude"
                                || std::env::var_os("CLAUDE_CODE_SHELL_PREFIX")
                                    .is_none_or(|prefix| prefix.is_empty()),
                            "Native Claude automatic host inference is unavailable with CLAUDE_CODE_SHELL_PREFIX; launch the MCP executable directly without a shell prefix or supply an explicit host PID"
                        );
                        cccc_core::local_sessions::NativeHost::parent(std::process::id())?
                    }
                };
                Some(format!("{runtime}:{}", host.key()))
            } else {
                None
            },
        })
    }

    pub async fn handle(
        &mut self,
        home: &HomeLayout,
        client: &DaemonClient,
        request: &Value,
    ) -> Value {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        match request["method"].as_str() {
            Some("tools/list") => {
                let names =
                    cccc_core::actor_base_tool_names("foreman", None).collect::<BTreeSet<_>>();
                let catalog = tools::catalog()
                    .into_iter()
                    .filter(|tool| {
                        tool["name"]
                            .as_str()
                            .is_some_and(|name| name == "cccc_session" || names.contains(name))
                    })
                    .map(|mut tool| {
                        if tool["name"] != "cccc_session" {
                            let schema = &mut tool["inputSchema"];
                            if schema["properties"].get("group_id").is_none() {
                                schema["properties"]["group_id"] = json!({"type":"string","description":"Explicit Group ID from the CCCC message or cccc_session list self_memberships; no active-Group fallback"});
                            }
                            let required = schema.as_object_mut().expect("tool schema")
                                .entry("required").or_insert(json!([])).as_array_mut().expect("required array");
                            if !required.contains(&json!("group_id")) { required.push(json!("group_id")); }
                        }
                        if tool["name"] == "cccc_message_send" {
                            tool["description"] = json!("Send a new message in this native collaboration Group. Defaults to mode=send for timely collaboration; choose mail explicitly for asynchronous Inbox work, or request_reply when a concrete reply is required. For an existing event, use cccc_message_reply with its event_id. Choose recipients deliberately; keep user and agent audiences separate, and avoid routine noise. Pass the explicit group_id from the received message or cccc_session list.");
                            tool["inputSchema"]["properties"]["mode"]["default"] = json!("send");
                        }
                        if tool["name"] == "cccc_bootstrap" {
                            let description = tool["description"].as_str().unwrap_or_default();
                            tool["description"] = json!(format!("Pass the explicit group_id from the received message or cccc_session list self_memberships. {description}"));
                        }
                        tool
                    }).collect::<Vec<_>>();
                json!({"jsonrpc":"2.0","id":id,"result":{"tools":catalog}})
            }
            Some("tools/call") => {
                let name = request["params"]["name"].as_str().unwrap_or_default();
                let args = match request["params"].get("arguments") {
                    Some(Value::Object(args)) => args.clone(),
                    None => Map::new(),
                    _ => {
                        return protocol_error(
                            id,
                            -32602,
                            "tools/call arguments must be an object",
                        );
                    }
                };
                match self.call(home, client, request, name, args).await {
                    Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                    Err(error) => {
                        json!({"jsonrpc":"2.0","id":id,"result":tool_error_result(&error)})
                    }
                }
            }
            _ => super::handle(home, client, request, None).await,
        }
    }

    async fn call(
        &mut self,
        home: &HomeLayout,
        client: &DaemonClient,
        request: &Value,
        name: &str,
        mut args: Map<String, Value>,
    ) -> std::result::Result<Value, ToolCallError> {
        args.remove("_cccc_native_binding");
        args.remove("_cccc_native_source_binding");
        args.remove("_cccc_web_binding");
        let (session_action, session_mode) = if name == "cccc_session" {
            // Identity comes from the native host, never the model's selector.
            args.remove("source_id");
            args.remove("by");
            let action = session_action(&args)?;
            let mode = if action == "send" {
                Some(crate::argument_normalization::message_mode(
                    &mut args, "send",
                )?)
            } else {
                None
            };
            validate_session_args(action, &mut args)?;
            (Some(action), mode)
        } else {
            (None, None)
        };
        let source_id = if self.runtime == "opencode" {
            opencode_session_id(&request["params"]["_meta"])?.map(|id| format!("opencode:{id}"))
        } else if self.runtime != "codex" {
            self.host_source_id.clone()
        } else {
            native_thread_id(&request["params"]["_meta"])?.map(|id| format!("codex:{id}"))
        };
        if session_action == Some("list") {
            let mut sources =
                daemon(client, "local_session_list", json!({"source_id":source_id})).await?;
            sources["self_source_id"] = json!(source_id);
            if sources.get("self_memberships").is_none() {
                sources["self_memberships"] = json!([]);
            }
            return Ok(result(sources));
        }
        let source_id =
            source_id.ok_or("The native runtime has not supplied the current session identity")?;
        // Receive readiness controls new handoffs, not an existing member's
        // ability to finish and reply after its Monitor/queue disconnects.
        let session = cccc_core::local_sessions::read(home, &source_id)
            .map_err(|_| "Native lifecycle identity is unavailable; reload the integration and submit a native prompt")?;
        if session.runtime.as_str() != self.runtime || !session.host.alive() {
            return Err("This native session no longer has a live lifecycle identity".into());
        }
        if self.runtime == "opencode"
            && (session.opencode_server.as_ref() != self.opencode_server.as_ref()
                || self
                    .opencode_server
                    .as_ref()
                    .is_none_or(|server| !server.alive()))
        {
            return Err(
                "The OpenCode MCP process does not belong to this source's captured backend".into(),
            );
        }
        let session_op = if let Some(action) = session_action {
            args.insert("source_id".into(), json!(source_id));
            args.remove("action");
            let op = match action {
                "send" => {
                    args.insert("message_mode".into(), json!(session_mode));
                    args.insert("require_peer_insight".into(), json!(true));
                    "local_session_send"
                }
                "conversation" => "local_session_conversation",
                "leave" => "local_session_leave",
                _ => return Err("Unknown native session action".into()),
            };
            let has_group = args
                .get("group_id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty());
            if op == "local_session_conversation" || (op == "local_session_send" && !has_group) {
                if has_group {
                    return Err("Create a conversation with source_ids, not group_id".into());
                }
                args.insert(
                    "_cccc_native_source_binding".into(),
                    json!(cccc_contracts::local_session::LocalSessionSourceBinding {
                        source_id: source_id.clone(),
                        session_revision: session.revision.clone(),
                    }),
                );
                return daemon(client, op, Value::Object(args)).await.map(result);
            }
            Some(op)
        } else {
            None
        };
        let group_id = args.get("group_id").and_then(Value::as_str).filter(|s|!s.is_empty())
            .ok_or("Pass the explicit group_id from the CCCC message; native sessions do not use the global active Group")?.to_owned();
        let actors = daemon(client, "actor_list", json!({"group_id":group_id})).await?;
        let actor = actors["actors"]
            .as_array()
            .and_then(|actors| {
                actors
                    .iter()
                    .find(|a| a["external_session_id"] == source_id)
            })
            .ok_or_else(|| {
                let previous = actors["actors"].as_array().into_iter().flatten()
                    .filter_map(|actor| actor["external_session_id"].as_str())
                    .filter_map(|id| cccc_core::local_sessions::read_record(home,id).ok())
                    .filter(|old| old.host != session.host && session.same_native_conversation(old)
                        && old.retired != Some(cccc_core::local_sessions::RetireReason::IntegrationDisabled))
                    .collect::<Vec<_>>();
                let reload = if session.runtime == cccc_contracts::local_session::LocalSessionRuntime::Pi {"/reload"} else {"/reload-plugins"};
                let next = if let Some(old)=previous.iter().find(|old|old.host_alive()) {
                    format!("Another host of this conversation is still running as PID {}. If you intend to continue here, close that instance, then use {reload} in the original application to reload CCCC and retry recovery. ",old.host.pid)
                } else if let [old]=previous.as_slice() && old.host.has_exited() {
                    format!("This Group still references your conversation's previous host PID {}. Use {reload} in the original application to reload CCCC and retry recovery after closing any other live instance. ",old.host.pid)
                } else {String::new()};
                ToolCallError::new(
                    "native_membership_missing",
                    format!("Native source {source_id} is not a member of Group {group_id}. {next}If membership is still missing, ask the user to open Connect existing sessions, select this existing Group, and Connect to Group."),
                    json!({"group_id":group_id,"source_id":source_id}).as_object().expect("membership details").clone(),
                )
            })?
            .clone();
        if actor["enabled"] != true {
            return Err(ToolCallError::new(
                "native_membership_paused",
                format!(
                    "CCCC access for native source {source_id} in Group {group_id} is paused. Ask the user to choose Resume CCCC access for this member; restarting the native application does not enable paused access."
                ),
                json!({"group_id":group_id,"source_id":source_id,"actor_id":actor["id"]})
                    .as_object()
                    .expect("membership details")
                    .clone(),
            ));
        }
        let actor: cccc_contracts::Actor = serde_json::from_value(actor)
            .map_err(|_| "Native membership identity is unavailable")?;
        let binding = cccc_contracts::local_session::LocalSessionBinding {
            group_id: group_id.clone(),
            actor_id: actor.id.clone(),
            actor_generation: cccc_core::actors::generation_identity(&actor),
            source_id,
            session_revision: session.revision,
        };
        if let Some(op) = session_op {
            args.insert("by".into(), json!(actor.id));
            args.insert("_cccc_native_binding".into(), json!(binding));
            return daemon(client, op, Value::Object(args)).await.map(result);
        }
        router::call_with_context(
            home,
            client,
            name,
            args,
            Some(RequestContext {
                group_id: &group_id,
                actor_id: &actor.id,
                binding: None,
                native_binding: Some(&binding),
            }),
            false,
        )
        .await
    }
}

fn session_action(args: &Map<String, Value>) -> std::result::Result<&'static str, ToolCallError> {
    match args.get("action") {
        None => Ok("list"),
        Some(Value::String(action)) => match action.as_str() {
            "list" => Ok("list"),
            "send" => Ok("send"),
            "conversation" => Ok("conversation"),
            "leave" => Ok("leave"),
            _ => Err("invalid_args: action must be list, send, conversation or leave".into()),
        },
        _ => Err("invalid_args: action must be list, send, conversation or leave".into()),
    }
}

fn validate_session_args(
    action: &str,
    args: &mut Map<String, Value>,
) -> std::result::Result<(), ToolCallError> {
    let allowed: &[&str] = match action {
        "list" => &["action"],
        // mode has already been validated and removed at the MCP boundary.
        "send" => &[
            "action",
            "source_ids",
            "group_id",
            "to",
            "text",
            "client_id",
            "insight",
        ],
        "conversation" => &["action", "source_ids", "title"],
        "leave" => &["action", "group_id"],
        _ => unreachable!("validated session action"),
    };
    let unused_default = |field: &str, value: &Value| {
        // Some clients fill the shared catalog's optional fields. Ignore only
        // unset values or its default mode, never actual message intent.
        let known = matches!(
            field,
            "source_ids" | "group_id" | "text" | "title" | "to" | "mode" | "client_id" | "insight"
        );
        known
            && (value.is_null()
                || value.as_str().is_some_and(|value| value.trim().is_empty())
                || value.as_array().is_some_and(Vec::is_empty)
                || (field == "mode" && value == "send"))
    };
    if let Some((field, _)) = args
        .iter()
        .find(|(field, value)| !allowed.contains(&field.as_str()) && !unused_default(field, value))
    {
        return Err(format!("invalid_args: cccc_session action={action} does not use {field}. To send a message, use action=send with source_ids and text, or group_id and to; conversation only creates a Group.").into());
    }
    args.retain(|field, _| allowed.contains(&field.as_str()));
    Ok(())
}

async fn daemon(
    client: &DaemonClient,
    op: &str,
    args: Value,
) -> std::result::Result<Value, ToolCallError> {
    let response = client
        .call(&cccc_contracts::DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_default(),
        })
        .await
        .map_err(|e| e.to_string())?;
    if let Some(error) = response.error {
        return Err(ToolCallError::from_daemon(error));
    }
    Ok(Value::Object(response.result))
}

fn result(payload: Value) -> Value {
    json!({"content":[{"type":"text","text":serde_json::to_string(&payload).unwrap_or_default()}],"structuredContent":payload,"isError":false})
}

fn native_thread_id(meta: &Value) -> std::result::Result<Option<String>, ToolCallError> {
    // Codex supplies params._meta.threadId. Nested provider metadata and
    // sessionId are unrelated to the native thread's authority.
    match meta.get("threadId") {
        None => Ok(None),
        Some(Value::String(id)) if !id.trim().is_empty() => Ok(Some(id.clone())),
        Some(_) => {
            Err("The native runtime must supply params._meta.threadId as a nonempty string".into())
        }
    }
}

fn opencode_session_id(meta: &Value) -> std::result::Result<Option<String>, ToolCallError> {
    match meta.get("ai.opencode/sessionID") {
        None => Ok(None),
        Some(Value::String(id))
            if id.starts_with("ses_")
                && id.len() <= 256
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') =>
        {
            Ok(Some(id.clone()))
        }
        Some(_) => Err(
            "OpenCode must supply params._meta[\"ai.opencode/sessionID\"] as a native session ID"
                .into(),
        ),
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "native_session/authority_tests.rs"]
mod authority_tests;

#[cfg(test)]
#[path = "native_session/action_tests.rs"]
mod action_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_identity_comes_only_from_current_request_metadata() {
        assert_eq!(
            native_thread_id(&json!({"threadId":"current","provider":{"threadId":"noise"}}))
                .expect("fixture should succeed"),
            Some("current".into())
        );
        assert_eq!(
            native_thread_id(&json!({"provider":{"threadId":"noise"},"sessionId":"current"}))
                .expect("fixture should succeed"),
            None
        );
        assert!(native_thread_id(&json!({"threadId":7,"sessionId":"current"})).is_err());
    }

    #[test]
    fn opencode_identity_uses_only_its_official_per_call_metadata() {
        assert_eq!(
            opencode_session_id(
                &json!({"ai.opencode/sessionID":"ses_current","threadId":"ses_other"})
            )
            .expect("native metadata"),
            Some("ses_current".into())
        );
        for meta in [
            json!({"sessionID":"ses_current"}),
            json!({"threadId":"ses_current"}),
            json!({"provider":{"ai.opencode/sessionID":"ses_current"}}),
        ] {
            assert_eq!(opencode_session_id(&meta).expect("no native ID"), None);
        }
        for value in [
            json!(null),
            json!(17),
            json!(""),
            json!("ses_../other"),
            json!("unrelated"),
        ] {
            assert!(opencode_session_id(&json!({"ai.opencode/sessionID":value})).is_err());
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn codex_metadata_protocol_uses_only_the_top_level_thread_id() {
        use cccc_contracts::{Actor, DaemonRequest, local_session::LocalSessionRuntime};
        use cccc_core::local_sessions::{self as native, NativeHost, NativeSession};

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let groups = cccc_core::GroupStore::new(home.clone()).expect("groups");
        let mut group = groups.create("Codex metadata boundary", "").expect("group");
        for (id, thread_id) in [("current", "current-thread"), ("noise", "noise-thread")] {
            let source_id = format!("codex:{thread_id}");
            native::write(
                &home,
                NativeSession {
                    source_id: source_id.clone(),
                    runtime: LocalSessionRuntime::Codex,
                    native_session_id: thread_id.into(),
                    host: NativeHost::capture(std::process::id()).expect("host"),
                    cwd: temp.path().to_string_lossy().into_owned(),
                    title: id.into(),
                    origin: None,
                    revision: thread_id.into(),
                    codex_home: Some(temp.path().join("codex")),
                    codex_can_receive: false,
                    codex_program: None,
                    claude_config_dir: None,
                    pi_agent_dir: None,
                    opencode_config_dir: None,
                    opencode_server: None,
                    retired: None,
                },
            )
            .expect("isolated native lifecycle");
            let mut actor = Actor::new(id);
            actor.external_session_id = source_id;
            cccc_core::actors::add(&mut group, actor).expect("native member");
        }
        groups.save(&group).expect("save group");
        let daemon_home = home.clone();
        let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        let client = DaemonClient::new(home.clone());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if client
                    .call(&DaemonRequest {
                        v: 1,
                        op: "ping".into(),
                        args: Map::new(),
                    })
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("daemon ready");
        let cases = [
            ("top_level", json!({"threadId":"current-thread"}), true),
            (
                "top_level_with_nested_noise",
                json!({"threadId":"current-thread","sessionId":"noise-thread",
                    "provider":{"threadId":"noise-thread"},
                    "nested":[{"threadId":"unrelated-thread"}]}),
                true,
            ),
            (
                "nested_only",
                json!({"provider":{"threadId":"noise-thread"}}),
                false,
            ),
            ("array_only", json!([{"threadId":"noise-thread"}]), false),
            (
                "number",
                json!({"threadId":7,"provider":{"threadId":"noise-thread"}}),
                false,
            ),
            (
                "object",
                json!({"threadId":{"threadId":"noise-thread"}}),
                false,
            ),
            (
                "null",
                json!({"threadId":null,"sessionId":"current-thread"}),
                false,
            ),
            (
                "empty",
                json!({"threadId":"","sessionId":"current-thread"}),
                false,
            ),
            (
                "blank",
                json!({"threadId":"  ","sessionId":"current-thread"}),
                false,
            ),
            (
                "session_id_only",
                json!({"sessionId":"current-thread"}),
                false,
            ),
            ("alias_only", json!({"thread_id":"current-thread"}), false),
            ("missing", Value::Null, false),
        ];
        let mut requests = Vec::new();
        for (label, meta, _) in &cases {
            requests.push(
                json!({"jsonrpc":"2.0","id":label,"method":"tools/call","params":{
                    "name":"cccc_bootstrap","_meta":meta,
                    "arguments":{"group_id":group.group_id,"actor_id":"current",
                        "source_id":"codex:current-thread","sessionId":"current-thread"}
                }}),
            );
        }
        // Discovery must use the same field without conflating nested values,
        // and requests after an invalid one must not reuse a cached identity.
        requests.push(json!({"jsonrpc":"2.0","id":"discovery",
            "method":"tools/call","params":{"name":"cccc_session",
                "_meta":{"threadId":"current-thread","provider":{"threadId":"noise-thread"}},
                "arguments":{"action":"list"}}}));
        requests.push(json!({"jsonrpc":"2.0","id":"anonymous",
            "method":"tools/call","params":{"name":"cccc_session",
                "_meta":{"provider":{"threadId":"noise-thread"},"sessionId":"current-thread"},
                "arguments":{"action":"list"}}}));

        // Re-enter the public stdio runner in a fixture process. The real
        // protocol parser, source routing, daemon and persistence stay intact.
        let mut child = tokio::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "native_session::tests::codex_metadata_stdio_fixture",
                "--nocapture",
            ])
            .env("CCCC_METADATA_FIXTURE_HOME", home.root())
            .env("CCCC_HOME", home.root())
            .env("CODEX_HOME", temp.path().join("codex"))
            .env("CLAUDE_CONFIG_DIR", temp.path().join("claude"))
            .env_remove("CCCC_ACTOR_ID")
            .env_remove("CCCC_GROUP_ID")
            .env_remove("CCCC_MCP_TOOL_PROFILE")
            .env_remove("CCCC_SECRETARY_TASK_TOKEN")
            .env_remove("CCCC_SECRETARY_TASK_TOKEN_FILE")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("public native stdio fixture");
        let mut stdin = child.stdin.take().expect("MCP input");
        for request in &requests {
            stdin
                .write_all(format!("{request}\n").as_bytes())
                .await
                .expect("request line");
        }
        drop(stdin);
        let output =
            tokio::time::timeout(std::time::Duration::from_secs(10), child.wait_with_output())
                .await
                .expect("stdio exit deadline")
                .expect("stdio output");
        daemon.abort();
        let _ = daemon.await;

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let responses = String::from_utf8(output.stdout)
            .expect("stdio UTF-8")
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(&line[line.find('{')?..]).ok())
            .filter(|value| value.get("jsonrpc").is_some())
            .collect::<Vec<_>>();
        assert_eq!(
            responses.len(),
            requests.len(),
            "complete protocol responses: {responses:?}"
        );
        let observed = cases
            .iter()
            .zip(&responses)
            .map(|((label, _, authorized), response)| {
                assert_eq!(response["id"], *label);
                assert!(response.get("error").is_none(), "{response}");
                json!({"case":label,"authorized":response["result"]["isError"] != true,
                "actor":response["result"]["structuredContent"]["session"]["actor_id"],
                "expected_authorized":authorized})
            })
            .collect::<Vec<_>>();
        eprintln!("metadata protocol observations: {}", json!(observed));
        for observation in observed {
            assert_eq!(
                observation["authorized"], observation["expected_authorized"],
                "{observation}"
            );
            if observation["authorized"] == true {
                assert_eq!(observation["actor"], "current", "{observation}");
            }
        }
        let discovery = &responses[cases.len()];
        let anonymous = &responses[cases.len() + 1];
        assert_ne!(discovery["result"]["isError"], true, "{discovery}");
        assert_eq!(
            discovery["result"]["structuredContent"]["self_source_id"],
            "codex:current-thread"
        );
        assert_eq!(
            anonymous["result"]["structuredContent"]["self_source_id"],
            Value::Null,
            "{anonymous}"
        );
        assert_eq!(
            anonymous["result"]["structuredContent"]["self_memberships"],
            json!([]),
            "{anonymous}"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn codex_metadata_stdio_fixture() {
        let Some(root) = std::env::var_os("CCCC_METADATA_FIXTURE_HOME") else {
            return;
        };
        crate::run_stdio_with_local_session(
            HomeLayout::from_path(root).expect("fixture Home"),
            Some("codex"),
            None,
        )
        .await
        .expect("native stdio");
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    #[tokio::test]
    async fn claude_stdio_uses_the_direct_parent_unless_an_explicit_host_is_supplied() {
        use cccc_core::local_sessions::NativeHost;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let host = NativeHost::capture(std::process::id()).expect("fixture parent");
        let source_id = format!("claude:{}", host.key());
        let daemon_home = home.clone();
        let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        let client = DaemonClient::new(home.clone());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if client
                    .call(&cccc_contracts::DaemonRequest {
                        v: 1,
                        op: "ping".into(),
                        args: Map::new(),
                    })
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("daemon ready");
        let cases = [
            ("direct_parent", None, None, true),
            ("empty_prefix", Some(""), None, true),
            ("shell_prefix", Some("fixture-wrapper"), None, false),
            ("blank_prefix", Some(" "), None, false),
            (
                "explicit_host_with_prefix",
                Some("fixture-wrapper"),
                Some(host.pid),
                true,
            ),
            ("invalid_explicit_host", None, Some(u32::MAX), false),
        ];
        let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"cccc_session","arguments":{"action":"list"}}});
        let mut observations = Vec::new();
        for (label, prefix, explicit_host, expected_started) in cases {
            // Spawn the executable directly, just like the native MCP entry.
            // No Claude process or provider configuration is needed.
            let mut command =
                tokio::process::Command::new(std::env::current_exe().expect("test binary"));
            command
                .args([
                    "--exact",
                    "native_session::tests::claude_stdio_host_fixture",
                    "--nocapture",
                ])
                .env("CCCC_CLAUDE_FIXTURE_HOME", home.root())
                .env("CCCC_HOME", home.root())
                .env("CODEX_HOME", temp.path().join("codex"))
                .env("CLAUDE_CONFIG_DIR", temp.path().join("claude"))
                .env_remove("CLAUDE_PID")
                .env_remove("CLAUDE_CODE_SHELL_PREFIX")
                .env_remove("CCCC_CLAUDE_FIXTURE_HOST_PID")
                .env_remove("CCCC_ACTOR_ID")
                .env_remove("CCCC_GROUP_ID")
                .env_remove("CCCC_MCP_TOOL_PROFILE")
                .env_remove("CCCC_SECRETARY_TASK_TOKEN")
                .env_remove("CCCC_SECRETARY_TASK_TOKEN_FILE")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true);
            if let Some(prefix) = prefix {
                command.env("CLAUDE_CODE_SHELL_PREFIX", prefix);
            }
            if let Some(pid) = explicit_host {
                command.env("CCCC_CLAUDE_FIXTURE_HOST_PID", pid.to_string());
            }
            let mut child = command.spawn().expect("direct native stdio fixture");
            let mut stdin = child.stdin.take().expect("MCP input");
            if expected_started {
                stdin
                    .write_all(format!("{request}\n").as_bytes())
                    .await
                    .expect("request line");
            }
            drop(stdin);
            let output =
                tokio::time::timeout(std::time::Duration::from_secs(10), child.wait_with_output())
                    .await
                    .expect("stdio exit deadline")
                    .expect("stdio output");
            let responses = String::from_utf8(output.stdout)
                .expect("stdio UTF-8")
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(&line[line.find('{')?..]).ok())
                .filter(|value| value.get("jsonrpc").is_some())
                .collect::<Vec<_>>();
            let stderr = String::from_utf8_lossy(&output.stderr);
            observations.push(json!({
                "case":label,
                "started":output.status.success(),
                "expected_started":expected_started,
                "source":responses.first().map(|response| response["result"]["structuredContent"]["self_source_id"].clone()),
                "response_count":responses.len(),
                "prefix_error":stderr.contains("CLAUDE_CODE_SHELL_PREFIX") && stderr.contains("automatic"),
            }));
        }
        daemon.abort();
        let _ = daemon.await;
        eprintln!(
            "Claude direct-host protocol observations: {}",
            json!(observations)
        );
        for observation in observations {
            assert_eq!(
                observation["started"], observation["expected_started"],
                "{observation}"
            );
            if observation["started"] == true {
                assert_eq!(observation["response_count"], 1, "{observation}");
                assert_eq!(observation["source"], source_id, "{observation}");
            } else {
                assert_eq!(observation["response_count"], 0, "{observation}");
                if matches!(
                    observation["case"].as_str(),
                    Some("shell_prefix" | "blank_prefix")
                ) {
                    assert_eq!(observation["prefix_error"], true, "{observation}");
                }
            }
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    #[tokio::test]
    async fn claude_stdio_host_fixture() {
        let Some(root) = std::env::var_os("CCCC_CLAUDE_FIXTURE_HOME") else {
            return;
        };
        let host_pid = std::env::var("CCCC_CLAUDE_FIXTURE_HOST_PID")
            .ok()
            .map(|pid| pid.parse().expect("fixture host PID"));
        crate::run_stdio_with_local_session(
            HomeLayout::from_path(root).expect("fixture Home"),
            Some("claude"),
            host_pid,
        )
        .await
        .expect("native stdio");
    }

    #[tokio::test]
    async fn native_tools_require_explicit_groups_but_discovery_does_not() {
        let temp = tempfile::tempdir().expect("fixture should succeed");
        let home = HomeLayout::from_path(temp.path().join("cccc")).expect("fixture should succeed");
        let client = DaemonClient::new(home.clone());
        let mut context = Context::new("codex", None).expect("fixture should succeed");
        let response = context
            .handle(&home, &client, &json!({"id":1,"method":"tools/list"}))
            .await;
        let tools = response["result"]["tools"]
            .as_array()
            .expect("fixture should succeed");
        assert!(tools.iter().any(|tool| tool["name"] == "cccc_session"));
        let session = tools
            .iter()
            .find(|tool| tool["name"] == "cccc_session")
            .expect("session tool");
        assert_eq!(
            session["inputSchema"]["properties"]["mode"]["default"],
            "send"
        );
        assert!(
            session["inputSchema"]["properties"]
                .get("message_mode")
                .is_none()
        );
        let send = tools
            .iter()
            .find(|tool| tool["name"] == "cccc_message_send")
            .expect("native send");
        assert_eq!(send["inputSchema"]["properties"]["mode"]["default"], "send");
        let history = tools
            .iter()
            .find(|tool| tool["name"] == "cccc_message_history")
            .expect("native notification read entry point");
        assert_eq!(
            history["inputSchema"]["properties"]["event_id"]["type"], "string",
            "fresh MCP discovery must declare the exact read used by Monitor"
        );
        let ordinary = super::tools::catalog()
            .into_iter()
            .find(|tool| tool["name"] == "cccc_message_send")
            .expect("ordinary send");
        assert_eq!(
            ordinary["inputSchema"]["properties"]["mode"]["default"],
            "mail"
        );
        for tool in tools.iter().filter(|tool| tool["name"] != "cccc_session") {
            assert!(
                tool["inputSchema"]["properties"].get("group_id").is_some(),
                "{}",
                tool["name"]
            );
            assert!(
                tool["inputSchema"]["required"]
                    .as_array()
                    .expect("fixture should succeed")
                    .contains(&json!("group_id")),
                "{}",
                tool["name"]
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn native_members_can_discover_and_reply_without_a_receive_channel() {
        use cccc_contracts::{
            Actor, ActorRuntime, DaemonRequest, Event, GroupState,
            local_session::LocalSessionRuntime,
        };
        use cccc_core::local_sessions::{self as native, NativeHost, NativeSession};
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
        let host = NativeHost::capture(std::process::id()).expect("host");
        let source_id = format!("claude:{}", host.key());
        native::write(
            &home,
            NativeSession {
                source_id: source_id.clone(),
                runtime: LocalSessionRuntime::Claude,
                native_session_id: "after-clear".into(),
                host: host.clone(),
                cwd: temp.path().to_string_lossy().into_owned(),
                title: "Native fixture".into(),
                origin: None,
                revision: "after-clear-revision".into(),
                codex_home: None,
                codex_can_receive: false,
                codex_program: None,
                claude_config_dir: None,
                pi_agent_dir: None,
                opencode_config_dir: None,
                opencode_server: None,
                retired: None,
            },
        )
        .expect("lifecycle metadata without a Monitor");
        let groups = cccc_core::GroupStore::new(home.clone()).expect("store");
        let mut member_group = groups
            .create("Existing native collaboration", "")
            .expect("Group");
        let mut actor = Actor::new("native-peer");
        actor.runtime = ActorRuntime::Claude;
        actor.external_session_id = source_id.clone();
        cccc_core::actors::add(&mut member_group, actor).expect("member");
        let mut recipient = Actor::new("offline-peer");
        recipient.runtime = ActorRuntime::Claude;
        recipient.external_session_id = "claude:offline-fixture".into();
        cccc_core::actors::add(&mut member_group, recipient).expect("offline recipient");
        groups.save(&member_group).expect("save Group");
        let mut paused = member_group.clone();
        paused.group_id = groups
            .create("Other paused membership", "")
            .expect("other Group")
            .group_id;
        paused.actors[0].enabled = false;
        paused.state = GroupState::Paused;
        groups.save(&paused).expect("paused Group");
        cccc_core::active::set(&home, &paused.group_id).expect("unrelated global selection");
        let mut incoming = Event::new("chat.message", &member_group.group_id);
        incoming.by = "user".into();
        incoming.data =
            json!({"to":["native-peer"],"text":"Please reply when done","message_mode":"send"})
                .as_object()
                .expect("message data")
                .clone();
        cccc_core::ledger::append(
            &groups.ledger_path(&member_group.group_id).expect("ledger"),
            &incoming,
        )
        .expect("incoming task");

        let daemon_home = home.clone();
        let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        let client = DaemonClient::new(home.clone());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if client
                    .call(&DaemonRequest {
                        v: 1,
                        op: "ping".into(),
                        args: Map::new(),
                    })
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("daemon ready");
        let mut context = Context::new("claude", Some(std::process::id())).expect("native context");
        let listed = context
            .handle(
                &home,
                &client,
                &json!({"id":1,"method":"tools/call","params":{
                    "name":"cccc_session","arguments":{"action":"list","source_id":"forged"}
                }}),
            )
            .await;
        let discovery = &listed["result"]["structuredContent"];
        assert_eq!(
            discovery["sources"],
            json!([]),
            "receive readiness stays separate"
        );
        assert_eq!(discovery["self_source_id"], source_id);
        let memberships = discovery["self_memberships"]
            .as_array()
            .expect("read-only memberships after clear");
        assert_eq!(memberships.len(), 2);
        assert!(memberships.iter().any(|membership| membership["group_id"]
            == member_group.group_id
            && membership["actor_id"] == "native-peer"
            && membership["role"] == "foreman"
            && membership["enabled"] == true));
        assert!(
            memberships
                .iter()
                .any(|membership| membership["group_id"] == paused.group_id
                    && membership["enabled"] == false
                    && membership["group_state"] == "paused")
        );
        let bootstrapped = context
            .handle(
                &home,
                &client,
                &json!({"id":10,"method":"tools/call","params":{
                    "name":"cccc_bootstrap","arguments":{"group_id":member_group.group_id}
                }}),
            )
            .await;
        assert_ne!(bootstrapped["result"]["isError"], true, "{bootstrapped}");
        let members = bootstrapped["result"]["structuredContent"]["session"]["members"]
            .as_array()
            .expect("basic collaboration includes its recipients");
        assert_eq!(members.len(), 2, "members of other Groups are excluded");
        let recipient_id = members
            .iter()
            .find(|member| member["source_id"] == "claude:offline-fixture")
            .expect("map a discovered native source to its Actor")["actor_id"]
            .as_str()
            .expect("recipient ID")
            .to_owned();
        let reply = context.handle(&home, &client, &json!({"id":2,"method":"tools/call","params":{
            "name":"cccc_message_reply","arguments":{"group_id":member_group.group_id,"event_id":incoming.id,"text":"Done from the original member","_cccc_native_binding":{"actor_id":"forged"}}
        }})).await;
        assert_ne!(
            reply["result"]["isError"], true,
            "offline receive channel cannot revoke reply access: {reply}"
        );
        assert_eq!(
            reply["result"]["structuredContent"]["event"]["by"],
            "native-peer"
        );
        assert_eq!(
            reply["result"]["structuredContent"]["event"]["group_id"],
            member_group.group_id
        );
        for (tool, explicit_mode, nested) in [
            ("cccc_message_send", None, false),
            ("cccc_message_send", Some("mail"), false),
            ("cccc_message_send", Some("request_reply"), false),
            ("cccc_message_send", None, true),
            ("cccc_session", None, false),
            ("cccc_session", Some("mail"), false),
            ("cccc_session", Some("request_reply"), false),
        ] {
            let mut args = json!({"group_id":member_group.group_id,"to":[recipient_id],"text":"Synthetic native message","insight":"Synthetic offline transport verification"});
            if tool == "cccc_session" {
                args["action"] = json!("send");
            }
            if let Some(mode) = explicit_mode {
                args["mode"] = json!(mode);
            }
            let (tool, args) = if nested {
                (
                    "cccc_capability_use",
                    json!({"group_id":member_group.group_id,"capability_id":"core","tool_name":"cccc_message_send","tool_arguments":args}),
                )
            } else {
                (tool, args)
            };
            let sent = context
                .handle(
                    &home,
                    &client,
                    &json!({"id":6,"method":"tools/call","params":{
                        "name":tool,"arguments":args
                    }}),
                )
                .await;
            assert_ne!(sent["result"]["isError"], true, "{sent}");
            let payload = &sent["result"]["structuredContent"];
            let payload = if nested {
                &payload["tool_result"]
            } else {
                payload
            };
            assert_eq!(
                payload["event"]["data"]["message_mode"],
                explicit_mode.unwrap_or("send")
            );
        }
        let ledger_path = groups.ledger_path(&member_group.group_id).expect("ledger");
        let message_count = || {
            cccc_core::ledger::read_all(&ledger_path)
                .expect("messages")
                .iter()
                .filter(|event| event.kind == "chat.message")
                .count()
        };
        let before = message_count();
        for tool in ["cccc_session", "cccc_message_send", "cccc_message_reply"] {
            for invalid in [
                json!({"message_mode":"mail"}),
                json!({"mode":"send","message_mode":"mail"}),
                json!({"mode":"mail","message_mode":"mail"}),
                json!({"mode":null}),
                json!({"mode":false}),
                json!({"mode":"invalid"}),
            ] {
                let mut args = json!({"action":"send","group_id":member_group.group_id,"to":["offline-peer"],"event_id":incoming.id,"text":"This message must not be appended","insight":"Synthetic invalid mode verification"});
                args.as_object_mut()
                    .expect("args")
                    .extend(invalid.as_object().expect("invalid mode").clone());
                let rejected = context.handle(&home, &client, &json!({"id":7,"method":"tools/call","params":{"name":tool,"arguments":args}})).await;
                assert_eq!(rejected["result"]["isError"], true, "{tool}: {rejected}");
                assert_eq!(
                    rejected["result"]["structuredContent"]["error"]["code"], "invalid_args",
                    "{rejected}"
                );
                assert_eq!(
                    message_count(),
                    before,
                    "{tool} must not send on invalid mode"
                );
            }
        }
        let binding = cccc_contracts::local_session::LocalSessionBinding {
            group_id: member_group.group_id.clone(),
            actor_id: member_group.actors[0].id.clone(),
            actor_generation: cccc_core::actors::generation_identity(&member_group.actors[0]),
            source_id: source_id.clone(),
            session_revision: "after-clear-revision".into(),
        };
        let mut replacement = member_group.clone();
        replacement.actors[0].generation = "replacement-generation".into();
        groups
            .save(&replacement)
            .expect("replace membership after its lookup");
        for (name, args) in [
            (
                "cccc_message_send",
                json!({"to":["user"],"text":"Stale request"}),
            ),
            (
                "cccc_capability_use",
                json!({"capability_id":"pack:group-runtime"}),
            ),
            ("cccc_bootstrap", json!({})),
            ("cccc_file", json!({"path":"fixture.txt"})),
        ] {
            let result = router::call_with_context(
                &home,
                &client,
                name,
                args.as_object().expect("arguments").clone(),
                Some(RequestContext {
                    group_id: &member_group.group_id,
                    actor_id: "native-peer",
                    binding: None,
                    native_binding: Some(&binding),
                }),
                false,
            )
            .await;
            assert!(
                result.is_err(),
                "{name} must retain its captured membership"
            );
            assert!(
                result
                    .expect_err("stale native authority")
                    .to_string()
                    .contains("permission_denied"),
                "{name}"
            );
        }
        groups
            .save(&member_group)
            .expect("restore native membership");
        let missing_group = context
            .handle(
                &home,
                &client,
                &json!({"id":3,"method":"tools/call","params":{
                    "name":"cccc_bootstrap","arguments":{}
                }}),
            )
            .await;
        assert_eq!(
            missing_group["result"]["isError"], true,
            "no active-Group fallback"
        );
        let disabled = context
            .handle(
                &home,
                &client,
                &json!({"id":4,"method":"tools/call","params":{
                    "name":"cccc_bootstrap","arguments":{"group_id":paused.group_id}
                }}),
            )
            .await;
        assert_eq!(
            disabled["result"]["isError"], true,
            "disabled memberships remain unauthorized"
        );
        native::retire(
            &home,
            &source_id,
            "after-clear",
            &host,
            native::RetireReason::SessionEnd,
        )
        .expect("native session ended");
        let retired = context.handle(&home, &client, &json!({"id":5,"method":"tools/call","params":{
            "name":"cccc_bootstrap","arguments":{"group_id":member_group.group_id,"source_id":source_id}
        }})).await;
        assert_eq!(
            retired["result"]["isError"], true,
            "ended native identity cannot regain access through arguments"
        );
        assert_eq!(
            cccc_core::active::get(&home).expect("active Group"),
            Some(paused.group_id)
        );
        assert_eq!(
            groups
                .list()
                .expect("no Group creation from discovery")
                .len(),
            2
        );
        let shutdown = client
            .call(&DaemonRequest {
                v: 1,
                op: "shutdown".into(),
                args: Map::new(),
            })
            .await
            .expect("stop fixture daemon");
        assert!(shutdown.ok);
        tokio::time::timeout(std::time::Duration::from_secs(5), daemon_task)
            .await
            .expect("fixture exit")
            .expect("daemon task")
            .expect("daemon stopped");
    }
}
