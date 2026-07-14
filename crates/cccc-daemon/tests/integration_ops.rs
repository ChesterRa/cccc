use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

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
    assert!(
        prompt.result["prompt"].as_str().is_some_and(|text| {
            text.contains("You are peer1") && text.contains("No fabrication")
        })
    );

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
        json!({"group_id":group_id,"lane":"work","remote_space_id":"notebook-1"}),
    );
    let first = call(
        &home,
        "group_space_ingest",
        json!({"group_id":group_id,"lane":"work","idempotency_key":"same","payload":{"title":"Migration evidence","content":"Rust only"}}),
    );
    let second = call(
        &home,
        "group_space_ingest",
        json!({"group_id":group_id,"lane":"work","idempotency_key":"same","payload":{"title":"ignored duplicate"}}),
    );
    assert_eq!(first.result["job_id"], second.result["job_id"]);
    assert_eq!(second.result["deduped"], true);
    let query = call(
        &home,
        "group_space_query",
        json!({"group_id":group_id,"lane":"work","query":"Migration"}),
    );
    assert_eq!(query.result["degraded"], true);
    assert!(
        !query.result["references"]
            .as_array()
            .expect("refs")
            .is_empty()
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
