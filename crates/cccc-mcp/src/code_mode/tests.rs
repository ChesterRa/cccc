use super::{
    Owner, cells, drain, expire_cell_after, normalize_identifier, parse_exec_pragma,
    reject_unsupported_source, shutdown, spawn_cell,
};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use std::time::Duration;

static CODE_CELL_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[test]
fn pragma_and_source_guards_match_public_contract() {
    let (source, pragma) = parse_exec_pragma(
        "// @exec: {\"yield-time_ms\": 25, \"max_output_tokens\": 99}\ntext('ok')",
    )
    .expect("pragma");
    assert_eq!(source, "text('ok')");
    assert_eq!(pragma["yield-time_ms"], 25);
    assert!(reject_unsupported_source("const important = 1").is_ok());
    assert!(reject_unsupported_source("require('node:fs')").is_err());
    assert!(reject_unsupported_source("import('node:fs')").is_err());
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
