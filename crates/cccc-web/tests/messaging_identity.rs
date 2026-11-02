#![cfg(unix)]

mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, access_tokens::AccessTokenStore};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn group_scoped_web_messages_derive_the_user_and_limit_chet_admission() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("identity", "")
        .expect("group");
    let tokens = AccessTokenStore::new(home.clone()).expect("tokens");
    tokens
        .create(
            "group-user",
            vec![group.group_id.clone()],
            false,
            Some("test-group-token"),
        )
        .expect("group token");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    for _ in 0..100 {
        if home.daemon_dir().join("ccccd.addr.json").exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        home.daemon_dir().join("ccccd.addr.json").exists(),
        "daemon address"
    );
    let app = cccc_web::app(home.clone());
    let send = |body: Value| {
        Request::post(format!("/api/v1/groups/{}/send", group.group_id))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer test-group-token")
            .body(Body::from(body.to_string()))
            .expect("request")
    };
    let response = app
        .clone()
        .oneshot(send(
            json!({"text":"hello", "to":["user"], "message_mode":"send"}),
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let result: Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(result["result"]["event"]["by"], "user");
    assert!(
        result["result"]["event"]["data"]
            .get(cccc_core::actors::CHET_WEB_ADMISSION_ARG)
            .is_none()
    );
    let response = app
        .clone()
        .oneshot(send(
            json!({"text":"hello", "to":["@chet"], "message_mode":"send"}),
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = app
        .clone()
        .oneshot(send(json!({
            "text":"x".repeat(70 * 1024), "to":["@chet"], "message_mode":"send"
        })))
        .await
        .expect("oversized response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/groups/{}/tracked_send", group.group_id))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer test-group-token")
                .body(Body::from(
                    json!({"text":"hello", "to":["@chet"]}).to_string(),
                ))
                .expect("tracked request"),
        )
        .await
        .expect("tracked response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let boundary = "identity-boundary";
    let multipart = format!(
        concat!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nhello\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"to_json\"\r\n\r\n[\"@chet\"]\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"message_mode\"\r\n\r\nsend\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"example.txt\"\r\n",
            "Content-Type: text/plain\r\n\r\nfixture\r\n--{boundary}--\r\n"
        ),
        boundary = boundary
    );
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/groups/{}/send_upload", group.group_id))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .header(header::AUTHORIZATION, "Bearer test-group-token")
                .body(Body::from(multipart))
                .expect("upload request"),
        )
        .await
        .expect("upload response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let admin = auth_support::authenticated_app(home.clone());
    let response = admin
        .oneshot(
            Request::post(format!("/api/v1/groups/{}/send", group.group_id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"text":"hello", "to":["@chet"], "message_mode":"send"}).to_string(),
                ))
                .expect("admin request"),
        )
        .await
        .expect("admin response");
    assert_eq!(response.status(), StatusCode::OK);
    let origin = "e".repeat(32);
    cccc_core::voice_notifications::register_origin(
        &home,
        &origin,
        "synthetic",
        "thread",
        cccc_contracts::ActorRuntime::Codex,
    )
    .expect("fixture origin");
    let source = cccc_daemon::handle_request(
        &home,
        &DaemonRequest {
            v: 1,
            op: "send".into(),
            args: json!({"group_id":group.group_id,"by":"user","to":["user"],
            "text":"Chet message","message_mode":"send","_voice_origin":origin})
            .as_object()
            .expect("source args")
            .clone(),
        },
    );
    assert!(source.ok, "{source:?}");
    let source_id = source.result["event"]["id"].as_str().expect("source ID");
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/groups/{}/reply", group.group_id))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer test-group-token")
                .body(Body::from(
                    json!({"text":"reply","reply_to":source_id}).to_string(),
                ))
                .expect("reply request"),
        )
        .await
        .expect("reply response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = app
        .oneshot(send(
            json!({"by":"worker", "text":"hello", "to":["user"], "message_mode":"send"}),
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
}
