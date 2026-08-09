// Included by the crate-level integration test harness.
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn chatgpt_web_model_actor_is_singleton_across_groups() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let first = call(&home, "group_create", json!({"title":"first"}));
    let first_group = first.result["group"]["group_id"]
        .as_str()
        .expect("first group id");
    let second = call(&home, "group_create", json!({"title":"second"}));
    let second_group = second.result["group"]["group_id"]
        .as_str()
        .expect("second group id");

    call(
        &home,
        "actor_add",
        json!({"group_id":first_group,"actor_id":"web1","runtime":"web_model","by":"user"}),
    );
    let duplicate = raw_call(
        &home,
        "actor_add",
        json!({"group_id":second_group,"actor_id":"web2","runtime":"web_model","by":"user"}),
    );
    assert_eq!(
        duplicate.error.expect("singleton error").code,
        "chatgpt_web_model_singleton"
    );

    call(
        &home,
        "actor_add",
        json!({"group_id":second_group,"actor_id":"peer","runtime":"codex","by":"user"}),
    );
    let converted = raw_call(
        &home,
        "actor_update",
        json!({"group_id":second_group,"actor_id":"peer","patch":{"runtime":"web_model"},"by":"user"}),
    );
    assert_eq!(
        converted.error.expect("update singleton error").code,
        "chatgpt_web_model_singleton"
    );

    call(
        &home,
        "actor_remove",
        json!({"group_id":first_group,"actor_id":"web1","by":"user"}),
    );
    let replacement = call(
        &home,
        "actor_update",
        json!({"group_id":second_group,"actor_id":"peer","patch":{"runtime":"web_model"},"by":"user"}),
    );
    assert_eq!(replacement.result["actor"]["runtime"], "web_model");
}

#[test]
fn actor_scope_paths_are_persisted_as_attached_scope_keys() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let project = temp.path().join("project");
    std::fs::create_dir(&project).expect("project");
    let created = call(&home, "group_create", json!({"title":"scoped actors"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let attached = call(
        &home,
        "attach",
        json!({"group_id":group_id,"path":project,"by":"user"}),
    );
    let scope_key = attached.result["group"]["active_scope_key"]
        .as_str()
        .expect("scope key");

    let added = call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer",
            "default_scope_key":project,
            "by":"user"
        }),
    );
    assert_eq!(added.result["actor"]["default_scope_key"], scope_key);

    let updated = call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer",
            "patch":{"default_scope_key":project},
            "by":"user"
        }),
    );
    assert_eq!(updated.result["actor"]["default_scope_key"], scope_key);
    assert_eq!(
        cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .load(group_id)
            .expect("group")
            .actors[0]
            .default_scope_key,
        scope_key
    );

    let invalid = raw_call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer",
            "patch":{"default_scope_key":temp.path().join("other")},
            "by":"user"
        }),
    );
    assert_eq!(
        invalid.error.expect("scope error").code,
        "scope_not_attached"
    );
}

#[test]
fn prompt_im_space_and_voice_operations_share_rust_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"integrations"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"peer1","runtime":"codex","by":"user"}),
    );
    let prompt = call(
        &home,
        "actor_prompt",
        json!({"group_id":group_id,"actor_id":"peer1"}),
    );
    assert!(prompt.result["prompt"].as_str().is_some_and(|text| {
        text.contains("You are peer1")
            && text.contains("CCCC Protocol:")
            && text.contains("use MCP tool `cccc_bootstrap`")
    }));

    let invalid_im = raw_call(
        &home,
        "im_set",
        json!({"group_id":group_id,"platform":"telegram"}),
    );
    assert!(!invalid_im.ok);
    call(
        &home,
        "im_set",
        json!({"group_id":group_id,"platform":"telegram","token_env":"TELEGRAM_TOKEN"}),
    );
    let start = raw_call(&home, "im_start", json!({"group_id":group_id}));
    assert!(!start.ok);
    assert_eq!(
        start.error.as_ref().map(|error| error.code.as_str()),
        Some("adapter_unavailable")
    );
    let status = call(&home, "im_status", json!({"group_id":group_id}));
    assert_eq!(status.result["configured"], true);
    assert_eq!(status.result["running"], false);
    assert_eq!(status.result["adapter_available"], false);

    call(
        &home,
        "group_space_bind",
        json!({"group_id":group_id,"provider":"notebooklm","lane":"work","remote_space_id":"notebook-1"}),
    );
    let capabilities = call(
        &home,
        "group_space_capabilities",
        json!({"group_id":group_id,"provider":"notebooklm"}),
    );
    assert_eq!(
        capabilities.result["ingest"]["resource_ingest"]["source_types"],
        json!(["pasted_text"])
    );
    assert!(
        capabilities.result["unavailable_capabilities"]
            .as_array()
            .is_some_and(|items| items.contains(&json!("resource_ingest.web_page")))
    );
    let unsupported_resource = raw_call(
        &home,
        "group_space_ingest",
        json!({
            "group_id":group_id,
            "lane":"work",
            "kind":"resource_ingest",
            "payload":{"source_type":"web_page","url":"https://example.test"}
        }),
    );
    assert_eq!(
        unsupported_resource.error.expect("capability error").code,
        "capability_unavailable"
    );
    let unavailable = raw_call(
        &home,
        "group_space_ingest",
        json!({"group_id":group_id,"lane":"work","payload":{}}),
    );
    assert_eq!(
        unavailable.error.expect("provider error").code,
        "space_provider_not_configured"
    );
    let local = raw_call(
        &home,
        "group_space_status",
        json!({"group_id":group_id,"provider":"local"}),
    );
    assert_eq!(
        local.error.expect("unsupported provider").code,
        "provider_unavailable"
    );
    let status = call(
        &home,
        "group_space_status",
        json!({"group_id":group_id,"provider":"notebooklm"}),
    );
    assert_eq!(
        status.result["bindings"]["work"]["remote_space_id"],
        "notebook-1"
    );

    let invalid_document = raw_call(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"../escape.md","content":"bad"}),
    );
    assert!(!invalid_document.ok);
    let document = call(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"voice/notes.md","content":"safe"}),
    );
    assert_eq!(document.result["document"]["storage_kind"], "rust_home");

    let profile = call(
        &home,
        "actor_profile_upsert",
        json!({"profile_id":"profile1","name":"Default","runtime":"codex"}),
    );
    assert_eq!(profile.result["profile"]["revision"], 1);
    let legacy = call(
        &home,
        "actor_profile_upsert",
        json!({"profile_id":"legacy","name":"Legacy","env":{"LEGACY_TOKEN":"secret"}}),
    );
    assert_eq!(
        legacy.result["profile"]["env"],
        json!({"LEGACY_TOKEN":"secret"})
    );
    let legacy_keys = call(
        &home,
        "actor_profile_secret_keys",
        json!({"profile_id":"legacy"}),
    );
    assert_eq!(legacy_keys.result["keys"], json!([]));
    let linked = call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"linked","profile_id":"legacy","by":"user"}),
    );
    assert_eq!(linked.result["actor"]["profile_id"], "legacy");
    assert_eq!(linked.result["actor"]["profile_revision_applied"], 1);
    let linked_private = raw_call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"linked-private","profile_id":"legacy","env_private":{"TOKEN":"denied"},"by":"user"}),
    );
    assert_eq!(
        linked_private.error.expect("linked private error").code,
        "actor_profile_linked_readonly"
    );
    let custom = call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"custom-private","env_private":{"TOKEN":"value"},"by":"user"}),
    );
    assert_eq!(custom.result["actor"]["id"], "custom-private");
    let custom_keys = call(
        &home,
        "actor_env_private_keys",
        json!({"group_id":group_id,"actor_id":"custom-private"}),
    );
    assert_eq!(custom_keys.result["keys"], json!(["TOKEN"]));
    let conflict = raw_call(
        &home,
        "actor_profile_upsert",
        json!({"profile_id":"profile1","name":"Changed","expected_revision":0}),
    );
    assert!(!conflict.ok);
    call(
        &home,
        "actor_profile_env_private_update",
        json!({"profile_id":"profile1","set":{"API_TOKEN":"secret-value"}}),
    );
    let keys = call(
        &home,
        "actor_profile_env_private_keys",
        json!({"profile_id":"profile1"}),
    );
    assert_eq!(keys.result["keys"], json!(["API_TOKEN"]));
    assert_eq!(keys.result["masked_values"]["API_TOKEN"], "********");
    assert!(
        !serde_json::to_string(&keys.result)
            .expect("serialize keys")
            .contains("secret-value")
    );

    let denied = raw_call(
        &home,
        "group_space_provider_credential_update",
        json!({"provider":"notebooklm","by":"peer1","auth_json":"{}"}),
    );
    assert_eq!(
        denied.error.expect("permission error").code,
        "permission_denied"
    );
    let credential = call(
        &home,
        "group_space_provider_credential_update",
        json!({"provider":"notebooklm","by":"user","auth_json":"{\"cookie\":\"secret\"}"}),
    );
    assert_eq!(
        credential.result["credential"]["masked_value"],
        "{\"******\"}"
    );
    assert!(
        !serde_json::to_string(&credential.result)
            .expect("credential response")
            .contains("secret")
    );
    let health = call(
        &home,
        "group_space_provider_health_check",
        json!({"provider":"notebooklm","by":"user"}),
    );
    assert_eq!(health.result["healthy"], false);
    assert_eq!(
        health.result["error"]["code"],
        "space_provider_auth_invalid"
    );
    let remote_status = call(
        &home,
        "group_space_status",
        json!({"group_id":group_id,"provider":"notebooklm"}),
    );
    assert_eq!(remote_status.result["provider"]["auth_configured"], true);
    assert_eq!(remote_status.result["provider"]["write_ready"], false);
    let invalid_query_option = raw_call(
        &home,
        "group_space_query",
        json!({"group_id":group_id,"provider":"notebooklm","lane":"work","query":"x","options":{"language":"zh"}}),
    );
    assert_eq!(
        invalid_query_option.error.expect("invalid option").code,
        "invalid_args"
    );

    let memory_layout = call(
        &home,
        "memory_reme_layout_get",
        json!({"group_id":group_id}),
    );
    assert!(memory_layout.result["memory_root"].is_string());
    assert!(memory_layout.result["today_daily_file"].is_string());
    assert_eq!(memory_layout.result["backend"]["name"], "local");

    let missing_date = raw_call(
        &home,
        "memory_reme_write",
        json!({"group_id":group_id,"target":"daily","content":"entry"}),
    );
    assert!(!missing_date.ok);
    let memory_write = call(
        &home,
        "memory_reme_write",
        json!({"group_id":group_id,"target":"memory","content":"durable fact","mode":"append","idempotency_key":"memory-1"}),
    );
    assert_eq!(memory_write.result["status"], "written");
    let memory_dedup = call(
        &home,
        "memory_reme_write",
        json!({"group_id":group_id,"target":"memory","content":"changed payload","idempotency_key":"memory-1"}),
    );
    assert_eq!(memory_dedup.result["reason"], "persistence_idempotency_key");
    let memory_replace = call(
        &home,
        "memory_reme_write",
        json!({"group_id":group_id,"target":"memory","content":"replacement","mode":"replace"}),
    );
    assert_eq!(memory_replace.result["status"], "written");
    let index = call(
        &home,
        "memory_reme_index_sync",
        json!({"group_id":group_id,"mode":"scan"}),
    );
    assert!(index.result["indexed_files"].as_u64().unwrap_or(0) >= 2);
    assert!(index.result["indexed_chunks"].as_u64().unwrap_or(0) >= 1);
    assert!(
        index.result["watched_paths"]
            .as_array()
            .is_some_and(|paths| paths
                .iter()
                .all(|path| path.as_str().is_some_and(|path| path.ends_with(".md"))))
    );
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = raw_call(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

fn raw_call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
