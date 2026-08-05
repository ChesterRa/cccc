use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::{GroupStore, HomeLayout};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn group_prompt_routes_follow_the_web_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("prompt routes", "").expect("group");
    let app = cccc_web::app(home);
    let base = format!("/api/v1/groups/{}/prompts", group.group_id);

    let (status, initial) = request_json(
        &app,
        Request::get(&base)
            .body(Body::empty())
            .expect("get prompts"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(initial["result"]["preamble"]["kind"], "preamble");
    assert_eq!(
        initial["result"]["preamble"]["filename"],
        "CCCC_PREAMBLE.md"
    );
    assert_eq!(initial["result"]["preamble"]["source"], "builtin");
    assert_eq!(initial["result"]["help"]["kind"], "help");
    assert_eq!(initial["result"]["help"]["filename"], "CCCC_HELP.md");
    assert_eq!(initial["result"]["help"]["source"], "builtin");
    assert!(
        initial["result"]["help"]["content"]
            .as_str()
            .expect("builtin help")
            .contains("Actor Notes")
    );

    let actor_note = "## @actor: reviewer\n\nUse agent-browser for browser tasks.\n";
    let (status, saved) = request_json(
        &app,
        Request::put(format!("{base}/help"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "content": actor_note,
                    "by": "user",
                    "editor_mode": "structured",
                    "changed_blocks": ["actor:reviewer"],
                })
                .to_string(),
            ))
            .expect("put help"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["result"]["kind"], "help");
    assert_eq!(saved["result"]["source"], "home");
    assert_eq!(saved["result"]["content"], actor_note);
    assert_eq!(saved["result"]["notified_actor_ids"], json!([]));
    assert_eq!(
        saved["result"]["notification_failures"][0]["stage"],
        "actor_list"
    );

    let (status, unchanged) = request_json(
        &app,
        Request::put(format!("{base}/help"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"content":actor_note,"by":"user"}).to_string(),
            ))
            .expect("put unchanged help"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged["result"]["notified_actor_ids"], json!([]));
    assert_eq!(unchanged["result"]["notification_failures"], json!([]));

    let override_path = store
        .group_dir(&group.group_id)
        .expect("group dir")
        .join("prompts/CCCC_HELP.md");
    assert_eq!(
        std::fs::read_to_string(&override_path).expect("saved override"),
        actor_note
    );

    let (status, loaded) = request_json(
        &app,
        Request::get(&base)
            .body(Body::empty())
            .expect("get prompts"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(loaded["result"]["help"]["source"], "home");
    assert_eq!(loaded["result"]["help"]["content"], actor_note);

    let (status, missing_confirmation) = request_json(
        &app,
        Request::delete(format!("{base}/help"))
            .body(Body::empty())
            .expect("delete without confirmation"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        missing_confirmation["error"]["code"],
        "confirmation_required"
    );
    assert!(override_path.exists());

    let (status, reset) = request_json(
        &app,
        Request::delete(format!("{base}/help?confirm=help"))
            .body(Body::empty())
            .expect("delete help"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reset["result"]["source"], "builtin");
    assert!(!override_path.exists());
}

#[tokio::test]
async fn group_prompt_routes_reject_unknown_kinds_and_reset_blank_content() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("prompt validation", "").expect("group");
    let app = cccc_web::app(home);
    let base = format!("/api/v1/groups/{}/prompts", group.group_id);

    let (status, unsupported) = request_json(
        &app,
        Request::put(format!("{base}/agents"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"content":"ignored"}"#))
            .expect("put unsupported kind"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(unsupported["error"]["code"], "invalid_kind");

    let (status, reset) = request_json(
        &app,
        Request::put(format!("{base}/preamble"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"content":"   "}"#))
            .expect("put blank preamble"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reset["result"]["kind"], "preamble");
    assert_eq!(reset["result"]["source"], "builtin");
}

async fn request_json(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let value = serde_json::from_slice(&body).expect("json");
    (status, value)
}
