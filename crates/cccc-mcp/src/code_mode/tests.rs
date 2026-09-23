use super::{
    Owner, cells, drain, expire_cell_after, normalize_identifier, parse_exec_pragma, shutdown,
    spawn_cell, validate_source,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use std::time::Duration;

static CODE_CELL_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[test]
fn original_document_resources_bypass_js_and_the_text_budget() {
    use serde_json::json;
    for (mime, suffix) in [
        ("application/pdf", "pdf"),
        (crate::file_read::PPTX_MIME, "pptx"),
    ] {
        let document = json!({"type":"resource","resource":{
            "uri":format!("cccc-file:///fixture.{suffix}"),"mimeType":mime,"blob":"ZG9jdW1lbnQtYnl0ZXM="
        }});
        let mut output = super::OutputBuffer::new(1);
        output.push(json!({"type":"text","text":"1234"}));
        let metadata = super::capture_nested_result(
            &mut output,
            json!({
                "content":[document.clone()],"structuredContent":{"mime_type":mime}
            }),
        )
        .expect("document handoff despite exhausted text budget");
        assert_eq!(metadata["mime_type"], mime);
        assert!(!metadata.to_string().contains("ZG9jdW1lbnQtYnl0ZXM="));
        let envelope = super::tool_result(super::format_response(
            "completed",
            "fixture",
            output,
            std::time::Instant::now(),
            "",
        ));
        assert_eq!(envelope["content"][1], document);
        assert!(
            !envelope["structuredContent"]
                .to_string()
                .contains("ZG9jdW1lbnQtYnl0ZXM=")
        );
        assert!(
            !envelope["content"][0]["text"]
                .as_str()
                .expect("text")
                .contains("ZG9jdW1lbnQtYnl0ZXM=")
        );
    }
}

#[test]
fn nested_images_reach_the_native_envelope_without_entering_js_or_text() {
    use serde_json::json;
    let mut output = super::OutputBuffer::new(1);
    output.push(json!({"type":"text","text":"1234"}));
    let metadata = super::capture_nested_result(
        &mut output,
        json!({
            "content":[{"type":"image","mimeType":"image/png","data":"aW1hZ2UtYnl0ZXM="}],
            "structuredContent":{"path":"fixture.png","bytes":11,"mime_type":"image/png"}
        }),
    )
    .expect("capture image despite exhausted text budget");
    assert_eq!(metadata["path"], "fixture.png");
    assert!(!metadata.to_string().contains("aW1hZ2UtYnl0ZXM="));
    let payload = super::format_response(
        "completed",
        "fixture",
        output,
        std::time::Instant::now(),
        "",
    );
    assert!(
        !payload["output"]
            .as_str()
            .expect("summary")
            .contains("aW1hZ2UtYnl0ZXM=")
    );
    let envelope = super::tool_result(payload);
    assert_eq!(envelope["content"][1]["data"], "aW1hZ2UtYnl0ZXM=");
    assert!(
        !envelope["structuredContent"]
            .to_string()
            .contains("aW1hZ2UtYnl0ZXM=")
    );
    assert!(
        !envelope["content"][0]["text"]
            .as_str()
            .expect("text")
            .contains("aW1hZ2UtYnl0ZXM=")
    );
}

#[test]
fn image_output_budget_is_separate_and_failure_is_explicit() {
    use serde_json::json;
    let mut output = super::OutputBuffer::new(1);
    for _ in 0..4 {
        super::capture_nested_result(
            &mut output,
            json!({
                "content":[{"type":"image","mimeType":"image/png","data":"YQ=="}],
                "structuredContent":{}
            }),
        )
        .expect("image within budget");
    }
    assert!(
        super::capture_nested_result(
            &mut output,
            json!({
                "content":[{"type":"image","mimeType":"image/png","data":"YQ=="}],
                "structuredContent":{}
            })
        )
        .expect_err("image budget")
        .to_string()
        .contains("4 per result")
    );
    output.push(json!({"type":"text","text":"text"}));
    let envelope = super::tool_result(super::format_response(
        "completed",
        "fixture",
        output,
        std::time::Instant::now(),
        "",
    ));
    assert_eq!(envelope["content"].as_array().expect("content").len(), 5);
    assert!(
        envelope["structuredContent"]["output"]
            .as_str()
            .expect("output")
            .ends_with("text")
    );
}

#[tokio::test]
async fn source_literals_and_comments_are_not_module_loads() {
    let _guard = CODE_CELL_TEST_LOCK.lock().await;
    if !node_available().await {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let client = DaemonClient::new(home.clone());
    let owner = Owner {
        binding: None,
        generation: String::new(),
        home: home.root().into(),
        group_id: "g_source".into(),
        actor_id: "web1".into(),
    };
    for source in [
        r#"const python = "from pathlib import Path"; text(python);"#,
        "// import fs from 'node:fs'\ntext('ok');",
        "/* require('node:fs') */ text('ok');",
        "text(`raw import fs ${`nested require('fs') ${1 + 1}`}`);",
        r#"text(/import\s+path/.test('import path'));"#,
        r#"const label = "escaped \" import fs"; text(label);"#,
    ] {
        validate_source(source).expect("ordinary source text");
        let (id, cell) = spawn_cell(temp.path(), owner.clone(), source, Vec::new(), 5_000)
            .await
            .expect("spawn");
        cells().lock().await.insert(id.clone(), cell.clone());
        let result = drain(&home, &client, &id, cell, 5_000, 1_000)
            .await
            .expect("result");
        assert_eq!(result["status"], "completed", "{source}: {result}");
        assert!(!result["output"].as_str().expect("string result").is_empty());
    }
}

#[tokio::test]
async fn runtime_rejects_module_access_including_template_expressions() {
    let _guard = CODE_CELL_TEST_LOCK.lock().await;
    if !node_available().await {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let client = DaemonClient::new(home.clone());
    let owner = Owner {
        binding: None,
        generation: String::new(),
        home: home.root().into(),
        group_id: "g_source".into(),
        actor_id: "web1".into(),
    };
    for source in [
        "import fs from 'node:fs'; text('unexpected');",
        "await import('node:fs'); text('unexpected');",
        "await import /* comment */ ('node:fs'); text('unexpected');",
        "text(`${await import('node:fs')}`);",
        "require('node:fs'); text('unexpected');",
    ] {
        let (id, cell) = spawn_cell(temp.path(), owner.clone(), source, Vec::new(), 5_000)
            .await
            .expect("spawn");
        cells().lock().await.insert(id.clone(), cell.clone());
        let result = drain(&home, &client, &id, cell, 5_000, 1_000)
            .await
            .expect("result");
        assert_eq!(result["status"], "failed", "{source}: {result}");
        assert!(
            result["error_text"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );
    }
}

#[test]
fn pragma_and_source_guards_match_public_contract() {
    let (source, pragma) = parse_exec_pragma(
        "// @exec: {\"yield-time_ms\": 25, \"max_output_tokens\": 99}\ntext('ok')",
    )
    .expect("pragma");
    assert_eq!(source, "text('ok')");
    assert_eq!(pragma["yield-time_ms"], 25);
    assert!(validate_source("const important = 1").is_ok());
    assert!(validate_source(&"x".repeat(super::MAX_SOURCE_CHARS + 1)).is_err());
}

#[test]
fn nested_tool_names_are_safe_javascript_identifiers() {
    assert_eq!(normalize_identifier("cccc_repo"), "cccc_repo");
    assert_eq!(normalize_identifier("1 odd-tool"), "odd_tool");
}

#[tokio::test]
async fn shared_runtime_is_sandboxed_and_persists_actor_store() {
    let _guard = CODE_CELL_TEST_LOCK.lock().await;
    if !node_available().await {
        return;
    }
    let temp = tempfile::tempdir().expect("temp dir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let client = DaemonClient::new(home.clone());
    let owner = Owner {
        binding: None,
        generation: String::new(),
        home: home.root().to_path_buf(),
        group_id: "g_test".into(),
        actor_id: "peer1".into(),
    };
    let (first_id, first) = spawn_cell(
        temp.path(),
        owner.clone(),
        r#"text([typeof process, typeof require, typeof fetch].join(",")); store("answer", {value: 42});"#,
        Vec::new(),
        5_000,
    )
    .await
    .expect("first cell");
    cells().lock().await.insert(first_id.clone(), first.clone());
    let first_result = drain(&home, &client, &first_id, first, 5_000, 10_000)
        .await
        .expect("first result");
    assert_eq!(first_result["status"], "completed");
    assert_eq!(first_result["output"], "undefined,undefined,undefined");

    let (second_id, second) = spawn_cell(
        temp.path(),
        owner,
        r#"text(JSON.stringify(load("answer")));"#,
        Vec::new(),
        5_000,
    )
    .await
    .expect("second cell");
    cells()
        .lock()
        .await
        .insert(second_id.clone(), second.clone());
    let second_result = drain(&home, &client, &second_id, second, 5_000, 10_000)
        .await
        .expect("second result");
    assert_eq!(second_result["status"], "completed");
    assert_eq!(second_result["output"], r#"{"value":42}"#);
}

#[tokio::test]
async fn shutdown_terminates_running_cells_for_the_home() {
    let _guard = CODE_CELL_TEST_LOCK.lock().await;
    if !node_available().await {
        return;
    }
    let temp = tempfile::tempdir().expect("temp dir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let owner = Owner {
        binding: None,
        generation: String::new(),
        home: home.root().to_path_buf(),
        group_id: "g_shutdown".into(),
        actor_id: "peer1".into(),
    };
    let (cell_id, cell) = spawn_cell(
        temp.path(),
        owner,
        "await new Promise((resolve) => setTimeout(resolve, 60_000));",
        Vec::new(),
        5_000,
    )
    .await
    .expect("running cell");
    cells().lock().await.insert(cell_id.clone(), cell.clone());

    shutdown(&home).await;

    assert!(!cells().lock().await.contains_key(&cell_id));
    assert!(
        cell.process
            .lock()
            .await
            .try_wait()
            .expect("child status")
            .is_some()
    );
}

#[tokio::test]
async fn idle_cell_expires_without_another_start_request() {
    let _guard = CODE_CELL_TEST_LOCK.lock().await;
    if !node_available().await {
        return;
    }
    let temp = tempfile::tempdir().expect("temp dir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let owner = Owner {
        binding: None,
        generation: String::new(),
        home: home.root().to_path_buf(),
        group_id: "g_expiry".into(),
        actor_id: "peer1".into(),
    };
    let (cell_id, cell) = spawn_cell(
        temp.path(),
        owner,
        "await new Promise((resolve) => setTimeout(resolve, 60_000));",
        Vec::new(),
        5_000,
    )
    .await
    .expect("running cell");
    cells().lock().await.insert(cell_id.clone(), cell);

    expire_cell_after(cell_id.clone(), Duration::from_millis(20)).await;

    assert!(!cells().lock().await.contains_key(&cell_id));
    shutdown(&home).await;
}

async fn node_available() -> bool {
    tokio::process::Command::new("node")
        .arg("--version")
        .output()
        .await
        .is_ok()
}

#[tokio::test]
async fn nested_management_preserves_target_and_authenticated_caller() {
    use cccc_contracts::{Actor, ActorRuntime, DaemonRequest};
    use cccc_core::GroupStore;
    use serde_json::json;

    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("nested management", "").expect("group");
    for id in ["alpha", "beta"] {
        let mut actor = Actor::new(id);
        actor.runtime = ActorRuntime::WebModel;
        actor.enabled = true;
        cccc_core::actors::add(&mut group, actor).expect("actor");
    }
    store.save(&group).expect("save");
    let owner = Owner {
        home: home.root().into(),
        group_id: group.group_id.clone(),
        actor_id: "alpha".into(),
        generation: group.actors[0].generation.clone(),
        binding: None,
    };
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone());
    tokio::time::timeout(Duration::from_secs(5), async {
        while client
            .call(&DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Default::default(),
            })
            .await
            .is_err()
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("daemon ready");

    let result = super::call_nested(&home, &client, &owner, "cccc_capability_use", Some(&json!({
        "tool_name":"cccc_actor", "tool_arguments":{"action":"stop", "actor_id":"beta", "by":"user"}
    }))).await;
    let saved = store.load(&group.group_id).expect("saved actors");
    // Also ensure nested arguments cannot elevate a peer to user authority.
    let peer = Owner {
        actor_id: "beta".into(),
        generation: group.actors[1].generation.clone(),
        ..owner
    };
    let denied = super::call_nested(&home, &client, &peer, "cccc_capability_use", Some(&json!({
        "tool_name":"cccc_actor", "tool_arguments":{"action":"stop", "actor_id":"alpha", "by":"user"}
    }))).await;
    daemon.abort();
    let _ = daemon.await;
    result.expect("foreman may stop peer");
    assert!(saved.actors[0].enabled, "caller alpha must remain enabled");
    assert!(
        !saved.actors[1].enabled,
        "explicit target beta must be stopped"
    );
    assert!(
        denied.is_err(),
        "peer cannot impersonate user in nested calls"
    );
}

#[tokio::test]
async fn deferred_nested_calls_reject_replaced_binding_and_actor_generation() {
    use cccc_contracts::{Actor, ActorRuntime};
    use cccc_core::{GroupStore, web_model_connectors as bindings};
    use serde_json::json;
    let temp = tempfile::tempdir().expect("valid test fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("valid test fixture");
    home.initialize().expect("valid test fixture");
    let store = GroupStore::new(home.clone()).expect("valid test fixture");
    let mut group = store.create("deferred", " ").expect("valid test fixture");
    let mut actor = Actor::new("a");
    actor.runtime = ActorRuntime::WebModel;
    actor.enabled = true;
    cccc_core::actors::add(&mut group, actor).expect("valid test fixture");
    store.save(&group).expect("valid test fixture");
    let generation = group.actors[0].generation.clone();
    let c = bindings::configure(&home).expect("valid test fixture");
    let id = c["connector"]["connector_id"]
        .as_str()
        .expect("valid test fixture");
    let pair = bindings::begin_pairing(&home, id, &group.group_id, "a", &generation, false)
        .expect("valid test fixture");
    bindings::accept_pairing(
        &home,
        id,
        pair["code"].as_str().expect("valid test fixture"),
        "host-a",
    )
    .expect("valid test fixture");
    let mut binding = bindings::confirm_pairing(
        &home,
        id,
        &group.group_id,
        "a",
        &generation,
        pair["pairing_id"].as_str().expect("valid test fixture"),
        "https://chatgpt.com/c/a",
    )
    .expect("valid test fixture");
    binding["connector_id"] = json!(id);
    let owner = Owner {
        home: home.root().into(),
        group_id: group.group_id.clone(),
        actor_id: "a".into(),
        generation: generation.clone(),
        binding: Some(binding),
    };
    bindings::validate_binding(&home, owner.binding.as_ref().expect("valid test fixture"))
        .expect("valid test fixture");
    let mut args = json!({"group_id":group.group_id,"by":"a","_cccc_web_binding":owner.binding});
    let before = super::resolve_owner(&home, args.as_object().expect("valid test fixture"))
        .expect("valid test fixture");
    args["_cccc_web_binding"]["last_activity_at"] = json!("later");
    args["_cccc_web_binding"]["last_tool_name"] = json!("cccc_file");
    assert_eq!(
        before,
        super::resolve_owner(&home, args.as_object().expect("valid test fixture"))
            .expect("valid test fixture"),
        "activity must not invalidate code_wait ownership"
    );

    let pair = bindings::begin_pairing(&home, id, &group.group_id, "a", &generation, false)
        .expect("valid test fixture");
    bindings::accept_pairing(
        &home,
        id,
        pair["code"].as_str().expect("valid test fixture"),
        "host-new",
    )
    .expect("valid test fixture");
    bindings::confirm_pairing(
        &home,
        id,
        &group.group_id,
        "a",
        &generation,
        pair["pairing_id"].as_str().expect("valid test fixture"),
        "https://chatgpt.com/c/new",
    )
    .expect("valid test fixture");
    let client = DaemonClient::new(home.clone());
    let error = super::call_nested(
        &home,
        &client,
        &owner,
        "cccc_file",
        Some(&json!({"action":"info","rel_path":"private.txt"})),
    )
    .await
    .expect_err("old code must not borrow new binding");
    assert!(
        error.to_string().contains("conversation_pairing_changed"),
        "{error}"
    );
    store
        .mutate(&group.group_id, |g| {
            g.actors[0].generation = "recreated".into();
            Ok(())
        })
        .expect("valid test fixture");
    let error = super::call_nested(&home, &client, &owner, "cccc_file", None)
        .await
        .expect_err("old Actor");
    assert!(error.to_string().contains("Actor changed"));
}
