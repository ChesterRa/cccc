#![cfg(unix)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::{HomeLayout, access_tokens::AccessTokenStore, cli_management};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn cli_management_is_admin_only_and_unmanaged_uninstall_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let home = HomeLayout::from_path(temp.path().join("home")).unwrap();
    home.initialize().unwrap();
    let tokens = AccessTokenStore::new(home.clone()).unwrap();
    let admin = tokens.create("admin", vec![], true, None).unwrap();
    let member = tokens
        .create("member", vec!["g_test".into()], false, None)
        .unwrap();
    let daemon_home = home.clone();
    let mut daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            assert!(
                !daemon.is_finished(),
                "daemon stopped before becoming ready: {:?}",
                (&mut daemon).await
            );
            if client
                .call(&DaemonRequest {
                    v: 1,
                    op: "ping".into(),
                    args: Default::default(),
                })
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let app = cccc_web::app(home.clone());

    for (method, suffix) in [
        ("GET", ""),
        ("GET", "/jobs/unknown/log"),
        ("POST", "/jobs"),
        ("PUT", "/schedules"),
        ("POST", "/uninstall"),
    ] {
        for (token, status) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some(member.token.as_str()), StatusCode::FORBIDDEN),
        ] {
            let mut request = Request::builder()
                .method(method)
                .uri(format!("/api/v1/cli-management{suffix}"));
            if let Some(token) = token {
                request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
            }
            let response = app
                .clone()
                .oneshot(
                    request
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{method} {suffix}");
        }
    }
    assert!(!cli_management::root(&home).exists());

    // 旧通知配置即使损坏也不再读取，不应阻塞 CLI 管理。
    let retired_config = cli_management::root(&home).join("notification.secret.json");
    cccc_core::fs::atomic_write(&retired_config, b"invalid legacy notification config").unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::get("/api/v1/cli-management")
                .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let catalog = payload(response).await;
    assert_eq!(catalog["result"]["runtimes"].as_array().unwrap().len(), 19);
    assert!(catalog["result"].get("notification").is_none());
    assert!(
        catalog["result"]["runtimes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|runtime| runtime["uninstall_available"] == false)
    );

    for (path, body) in [
        ("/uninstall", json!({})),
        (
            "/jobs",
            json!({"runtime":"codex","operation":"uninstall","request_id":"uninstall-check"}),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post(format!("/api/v1/cli-management{path}"))
                    .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error = payload(response).await;
        assert_eq!(error["error"]["code"], "invalid_args");
    }
    assert!(cli_management::load(&home).unwrap().jobs.is_empty());

    let rules = json!({"revision":0,"rules":[{"id":"monday","enabled":true,"trigger":{"kind":"cron","cron":"0 3 * * 1","timezone":"Asia/Shanghai"}}]});
    for expected in [StatusCode::OK, StatusCode::CONFLICT] {
        let response = app
            .clone()
            .oneshot(
                Request::put("/api/v1/cli-management/schedules")
                    .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(rules.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert_eq!(cli_management::load(&home).unwrap().rules.len(), 1);
    let retired = app
        .clone()
        .oneshot(
            Request::put("/api/v1/cli-management/notification")
                .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    // 未注册地址沿用原生 SPA 兜底；HTML 不是成功的配置 API 响应。
    assert_eq!(retired.status(), StatusCode::OK);
    assert!(
        retired.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert!(
        std::fs::read_to_string(&retired_config)
            .unwrap()
            .contains("invalid legacy")
    );
    // 模拟旧版本留下的未知 Runtime 队列项：后台必须失败并留日志，不能执行任意软件。
    cli_management::submit(
        &home,
        "unknown",
        cli_management::Operation::Install,
        "invalid-queued",
        chrono::Utc::now(),
    )
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if cli_management::load(&home).unwrap().jobs["invalid-queued"].status
                == cli_management::JobStatus::Failed
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::get("/api/v1/cli-management/jobs/invalid-queued/log")
                .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let logs = payload(response).await;
    assert!(
        logs["result"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["stream"] == "error")
    );
    assert!(
        !logs["result"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["stream"] == "notification")
    );
    // 原始输出只在受保护文件中；真实 HTTP 日志入口返回原生尾部视图并脱敏。
    {
        use std::io::Write;
        let directory = cli_management::root(&home).join("logs/invalid-queued");
        std::fs::create_dir_all(&directory).unwrap();
        let filename = "00000000-0000-4000-8000-000000000001.stdout.log";
        cccc_core::fs::atomic_write(
            &directory.join(filename),
            b"ordinary output\ntoken=private-output-value\n",
        )
        .unwrap();
        let mut index = std::fs::OpenOptions::new()
            .append(true)
            .open(cli_management::root(&home).join("logs/invalid-queued.jsonl"))
            .unwrap();
        let offset = index.metadata().unwrap().len();
        writeln!(
            index,
            "{}",
            json!({"stream":"stdout", "output_file":filename, "text":""})
        )
        .unwrap();
        let response = app
            .clone()
            .oneshot(
                Request::get(format!(
                    "/api/v1/cli-management/jobs/invalid-queued/log?offset={offset}"
                ))
                .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let view = payload(response).await;
        assert!(view.to_string().contains("ordinary output"));
        assert!(view.to_string().contains("[REDACTED]"));
        assert!(!view.to_string().contains("private-output-value"));
        assert!(!view.to_string().contains(filename));
        assert!(view["result"]["entries"][0].get("output_file").is_none());
    }
    assert!(
        cli_management::load(&home)
            .unwrap()
            .installations
            .is_empty()
    );
    // 模拟状态可读但写锁路径不可用；HTTP 必须明确报后台故障而非返回旧快照。
    let lock = cli_management::root(&home).join("state.lock");
    let preserved = lock.with_extension("preserved");
    std::fs::rename(&lock, &preserved).unwrap();
    std::fs::create_dir(&lock).unwrap();
    let mut interrupted = cli_management::load(&home).unwrap();
    let mut queued = interrupted.jobs["invalid-queued"].clone();
    queued.id = "storage-recovery".into();
    queued.status = cli_management::JobStatus::Queued;
    queued.started_at = None;
    queued.finished_at = None;
    queued.error = None;
    interrupted.jobs.insert(queued.id.clone(), queued);
    cccc_core::fs::write_json(
        &cli_management::root(&home).join("state.json"),
        &interrupted,
    )
    .unwrap();
    let fault = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let response = app
                .clone()
                .oneshot(
                    Request::get("/api/v1/cli-management")
                        .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            if response.status() == StatusCode::SERVICE_UNAVAILABLE {
                break payload(response).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await;
    std::fs::remove_dir(&lock).unwrap();
    std::fs::rename(&preserved, &lock).unwrap();
    let restored = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let response = app
                .clone()
                .oneshot(
                    Request::get("/api/v1/cli-management")
                        .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            if response.status() == StatusCode::OK {
                let body = payload(response).await;
                if body["result"]["state"]["jobs"]["storage-recovery"]["status"] == "failed" {
                    break body;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await;
    client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        cli_management::load(&home).unwrap().rules[0].rule.id,
        "monday"
    );
    assert_eq!(fault.unwrap()["error"]["code"], "cli_worker_unavailable");
    assert!(restored.is_ok());
}

#[tokio::test]
async fn managed_uninstall_http_uses_worker_and_retries_are_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let home = HomeLayout::from_path(temp.path().join("home")).unwrap();
    home.initialize().unwrap();
    let now = chrono::Utc::now();
    cli_management::submit(
        &home,
        "codex",
        cli_management::Operation::Install,
        "fixture",
        now,
    )
    .unwrap();
    cli_management::claim_next(&home, now).unwrap();
    let executable = cli_management::root(&home).join("versions/fixture/bin/codex");
    cccc_core::fs::atomic_write(&executable, b"controlled test fixture").unwrap();
    cli_management::finish(
        &home,
        "fixture",
        Ok(cli_management::Installation {
            version: "1.0.0".into(),
            executable: executable.clone(),
            bin_paths: vec![executable.parent().unwrap().into()],
            installed_at: now.to_rfc3339(),
        }),
        now,
    )
    .unwrap();
    let external = temp.path().join("external-codex");
    cccc_core::fs::atomic_write(&external, b"preserve external installation").unwrap();
    let admin = AccessTokenStore::new(home.clone())
        .unwrap()
        .create("admin", vec![], true, None)
        .unwrap();
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while client
            .call(&DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Default::default(),
            })
            .await
            .is_err()
        {
            assert!(!daemon.is_finished());
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let app = cccc_web::app(home.clone());
    // 专用入口提交，通用入口重试相同编号；完成后重试也不能产生第二次删除。
    for path in ["/uninstall", "/jobs"] {
        let mut body = json!({"runtime":"codex","request_id":"remove-fixture"});
        if path == "/jobs" {
            body["operation"] = json!("uninstall");
        }
        let response = app
            .clone()
            .oneshot(
                Request::post(format!("/api/v1/cli-management{path}"))
                    .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let state = cli_management::load(&home).unwrap();
                let job = &state.jobs["remove-fixture"];
                if job.status == cli_management::JobStatus::Succeeded {
                    break;
                }
                assert!(
                    matches!(
                        job.status,
                        cli_management::JobStatus::Queued | cli_management::JobStatus::Running
                    ),
                    "{job:?}"
                );
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
    let state = cli_management::load(&home).unwrap();
    assert!(state.installations.is_empty());
    assert_eq!(state.jobs.len(), 2);
    assert!(!executable.exists());
    assert!(external.exists());
    let response = app
        .oneshot(
            Request::get("/api/v1/cli-management/jobs/remove-fixture/log")
                .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        payload(response)
            .await
            .to_string()
            .contains("受管软件删除完成")
    );
    client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

async fn payload(response: axum::response::Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
