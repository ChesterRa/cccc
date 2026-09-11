mod auth_support;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cccc_core::HomeLayout;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn runtime_endpoint_returns_frontend_availability_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let response = auth_support::authenticated_app(home)
        .oneshot(
            Request::get("/api/v1/runtimes")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let runtimes = payload["result"]["runtimes"].as_array().expect("runtimes");
    assert!(!runtimes.is_empty());
    assert!(runtimes.iter().any(|runtime| {
        runtime["name"] == "custom"
            && runtime["display_name"] == "Custom"
            && runtime["available"] == true
    }));
    assert!(runtimes.iter().any(|runtime| {
        runtime["name"] == "cline"
            && runtime["display_name"] == "Cline CLI"
            && runtime["recommended_command"] == "cline --tui --auto-approve true"
    }));
}

#[cfg(unix)]
#[tokio::test]
async fn corrupt_management_state_preserves_native_catalog_and_start_fail_closed() {
    use std::os::unix::fs::PermissionsExt;
    // 在子进程中提供确定可用的外部 CLI，不修改并行测试的全局 PATH。
    const CHILD: &str = "CCCC_RUNTIME_DISCOVERY_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let fixture = tempfile::tempdir().expect("corrupt management state");
        let binary = fixture.path().join("codex");
        std::fs::write(&binary, b"#!/bin/sh\nexit 0\n").expect("corrupt management state");
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))
            .expect("corrupt management state");
        let mut paths = vec![fixture.path().to_path_buf()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let output =
            std::process::Command::new(std::env::current_exe().expect("corrupt management state"))
                .args([
                    "--exact",
                    "corrupt_management_state_preserves_native_catalog_and_start_fail_closed",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env(
                    "PATH",
                    std::env::join_paths(paths).expect("corrupt management state"),
                )
                .output()
                .expect("corrupt management state");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let native = cccc_runtime::detect_runtimes();
    assert!(
        native
            .iter()
            .any(|runtime| runtime.name == "codex" && runtime.available)
    );
    let expected_available: Vec<_> = native
        .iter()
        .filter(|runtime| runtime.available)
        .map(|runtime| &runtime.name)
        .collect();
    let temp = tempfile::tempdir().expect("corrupt management state");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("corrupt management state");
    home.initialize().expect("corrupt management state");
    let path = cccc_core::cli_management::root(&home).join("state.json");
    cccc_core::fs::atomic_write(&path, b"broken").expect("corrupt management state");
    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::get("/api/v1/runtimes")
                .body(Body::empty())
                .expect("corrupt management state"),
        )
        .await
        .expect("corrupt management state");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("corrupt management state")
            .to_bytes(),
    )
    .expect("corrupt management state");
    assert_eq!(
        payload["result"]["cli_management_error"],
        "cli_management_state_error"
    );
    let runtimes = payload["result"]["runtimes"]
        .as_array()
        .expect("corrupt management state");
    assert_eq!(
        payload["result"]["available"],
        serde_json::json!(expected_available)
    );
    assert_eq!(runtimes.len(), native.len());
    for (runtime, expected) in runtimes.iter().zip(&native) {
        let mut original_fields = runtime.clone();
        original_fields
            .as_object_mut()
            .expect("corrupt management state")
            .remove("managed_error");
        assert_eq!(
            original_fields,
            serde_json::to_value(expected).expect("corrupt management state")
        );
        if runtime["name"] != "custom" && runtime["name"] != "web_model" {
            assert_eq!(runtime["managed_error"], "cli_management_state_error");
        }
    }
    assert!(
        cccc_core::cli_management::apply_environment(&home, "codex", &mut Default::default())
            .is_err()
    );
    assert_eq!(
        std::fs::read(&path).expect("corrupt management state"),
        b"broken"
    );
    std::fs::remove_file(path).expect("corrupt management state");
    let response = auth_support::authenticated_app(home)
        .oneshot(
            Request::get("/api/v1/runtimes")
                .body(Body::empty())
                .expect("corrupt management state"),
        )
        .await
        .expect("corrupt management state");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("corrupt management state")
            .to_bytes(),
    )
    .expect("corrupt management state");
    assert!(payload["result"]["cli_management_error"].is_null());
    assert_eq!(
        payload["result"]["runtimes"],
        serde_json::to_value(&native).expect("corrupt management state")
    );
    assert_eq!(
        payload["result"]["available"],
        serde_json::json!(expected_available)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn managed_runtime_with_missing_dependencies_is_not_advertised_as_available() {
    use cccc_core::cli_management as management;
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().expect("managed runtime with");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("managed runtime with");
    home.initialize().expect("managed runtime with");
    let now = chrono::Utc::now();
    for runtime in ["deepseek", "codex"] {
        management::submit(&home, runtime, management::Operation::Install, runtime, now)
            .expect("managed runtime with");
        management::claim_next(&home, now).expect("managed runtime with");
        let root = management::root(&home).join("versions").join(runtime);
        let binary = if runtime == "deepseek" {
            root.join("deepseek/node_modules/.bin/dsh-acp-demo")
        } else {
            root.join("bin/codex")
        };
        cccc_core::fs::atomic_write(&binary, b"#!/bin/sh\nexit 0\n").expect("managed runtime with");
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))
            .expect("managed runtime with");
        management::finish(
            &home,
            runtime,
            Ok(management::Installation {
                version: "test".into(),
                executable: binary,
                bin_paths: if runtime == "codex" {
                    vec![root.join("missing-node")]
                } else {
                    vec![]
                },
                installed_at: now.to_rfc3339(),
            }),
            now,
        )
        .expect("managed runtime with");
    }
    let response = auth_support::authenticated_app(home)
        .oneshot(
            Request::get("/api/v1/runtimes")
                .body(Body::empty())
                .expect("managed runtime with"),
        )
        .await
        .expect("managed runtime with");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("managed runtime with")
            .to_bytes(),
    )
    .expect("managed runtime with");
    for name in ["codex", "deepseek"] {
        let runtime = payload["result"]["runtimes"]
            .as_array()
            .expect("managed runtime with")
            .iter()
            .find(|runtime| runtime["name"] == name)
            .expect("managed runtime with");
        assert_eq!(runtime["available"], false, "{name}");
    }
}
