use super::{PNG, auth_support, wait_for_address};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest};
use cccc_core::{GroupStore, HomeLayout, access_tokens::AccessTokenStore};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

async fn post(
    home: &HomeLayout,
    token: Option<&str>,
    group_id: &str,
    operation: &str,
    body: Value,
) -> (StatusCode, Value) {
    request(
        home,
        token,
        group_id,
        operation,
        "application/json",
        serde_json::to_vec(&body).expect("JSON"),
    )
    .await
}

async fn request(
    home: &HomeLayout,
    token: Option<&str>,
    group_id: &str,
    operation: &str,
    content_type: &str,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let mut request = Request::post(format!("/api/v1/groups/{group_id}/{operation}"))
        .header(header::CONTENT_TYPE, content_type);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = auth_support::authenticated_app(home.clone())
        .oneshot(request.body(Body::from(body)).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).expect("JSON response"),
    )
}

fn multipart(reply_to: Option<&str>, im: bool) -> Vec<u8> {
    let mut fields = vec![
        ("text", "uploaded image"),
        ("to_json", "[\"receiver\"]"),
        ("message_mode", "mail"),
    ];
    if let Some(reply_to) = reply_to {
        fields.push(("reply_to", reply_to));
    }
    if im {
        fields.push(("transport", "im"));
    }
    let mut body = Vec::new();
    for (key, value) in fields {
        body.extend_from_slice(format!(
            "--origin-boundary\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n"
        ).as_bytes());
    }
    body.extend_from_slice(b"--origin-boundary\r\nContent-Disposition: form-data; name=\"files\"; filename=\"image.png\"\r\nContent-Type: image/png\r\n\r\n");
    body.extend_from_slice(PNG);
    body.extend_from_slice(b"\r\n--origin-boundary--\r\n");
    body
}

fn effects(groups: &GroupStore, group_ids: &[&str]) -> Vec<(Vec<u8>, Vec<String>)> {
    group_ids
        .iter()
        .map(|id| {
            let ledger = std::fs::read(groups.ledger_path(id).expect("ledger")).expect("events");
            let mut blobs = std::fs::read_dir(groups.state_dir(id).expect("state").join("blobs"))
                .map(|entries| {
                    entries
                        .map(|entry| {
                            entry
                                .expect("entry")
                                .file_name()
                                .to_string_lossy()
                                .into_owned()
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            blobs.sort();
            (ledger, blobs)
        })
        .collect()
}

#[tokio::test]
async fn ordinary_web_messages_cannot_supply_im_origin_or_inaccessible_relay_sources() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut source = groups.create("source", "").expect("source");
    let mut destination = groups.create("destination", "").expect("destination");
    for (group, id) in [(&mut source, "origin"), (&mut destination, "receiver")] {
        let mut actor = Actor::new(id);
        actor.runtime = ActorRuntime::Custom;
        actor.command = vec!["/bin/true".into()];
        cccc_core::actors::add(group, actor).expect("actor");
        groups.save(group).expect("save");
    }
    let tokens = AccessTokenStore::new(home.clone()).expect("tokens");
    let guest = tokens
        .create("guest", vec![destination.group_id.clone()], false, None)
        .expect("guest");
    let member = tokens
        .create(
            "member",
            vec![source.group_id.clone(), destination.group_id.clone()],
            false,
            None,
        )
        .expect("member");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;
    let client = cccc_client::DaemonClient::new(home.clone());
    let args = json!({"text":"ordinary message","to":["receiver"],"message_mode":"mail"});
    let (status, ordinary) = post(
        &home,
        Some(&guest.token),
        &destination.group_id,
        "send",
        args.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ordinary}");
    let parent = ordinary["result"]["event"]["id"].as_str().expect("parent");
    let ids = [&*source.group_id, &*destination.group_id];
    let before = effects(&groups, &ids);
    // Literal keys guard the public behavior independently of the shared list.
    for key in [
        "transport",
        "im_platform",
        "im_chat_id",
        "im_thread_id",
        "source_platform",
        "source_user_id",
        "source_message_id",
    ] {
        for token in [Some(guest.token.as_str()), None] {
            let mut forged = args.clone();
            forged[key] = json!("do-not-echo-source-value");
            let (status, payload) = post(&home, token, &destination.group_id, "send", forged).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{key}: {payload}");
            assert!(
                payload["error"]["message"]
                    .as_str()
                    .expect("message")
                    .contains("IM provenance is host-owned"),
                "{payload}"
            );
            assert!(!payload.to_string().contains("do-not-echo-source-value"));
        }
    }
    for (operation, body) in [
        (
            "reply",
            json!({"reply_to":parent,"text":"reply","to":["receiver"],"message_mode":"mail","source_platform":"slack"}),
        ),
        (
            "tracked_send",
            json!({"title":"task","text":"work","to":["receiver"],"transport":"im"}),
        ),
        (
            "slash_skill_dispatch",
            json!({"task_text":"work","transport":"im"}),
        ),
        (
            "send_cross_group",
            json!({"dst_group_id":source.group_id,"text":"cross","to":["origin"],"message_mode":"mail","transport":"im"}),
        ),
    ] {
        let (status, payload) = post(
            &home,
            Some(&member.token),
            &destination.group_id,
            operation,
            body,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{operation}: {payload}");
        assert!(
            payload["error"]["message"]
                .as_str()
                .expect("message")
                .contains("IM provenance is host-owned"),
            "{payload}"
        );
    }
    for (operation, reply) in [("send_upload", None), ("reply_upload", Some(parent))] {
        let (status, payload) = request(
            &home,
            Some(&guest.token),
            &destination.group_id,
            operation,
            "multipart/form-data; boundary=origin-boundary",
            multipart(reply, true),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{operation}: {payload}");
        assert!(
            payload["error"]["message"]
                .as_str()
                .expect("message")
                .contains("IM provenance is host-owned"),
            "{payload}"
        );
    }
    let relay = json!({"text":"relay reference","to":["receiver"],"message_mode":"mail","src_group_id":source.group_id,"src_event_id":"unavailable-source"});
    let (status, payload) = post(
        &home,
        Some(&guest.token),
        &destination.group_id,
        "send",
        relay.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{payload}");
    assert_eq!(payload["error"]["message"], "source Group access denied");
    assert_eq!(
        effects(&groups, &ids),
        before,
        "rejected requests changed ledgers or blobs"
    );

    // Access to both Groups permits manual relay; no new existence requirement.
    for token in [Some(member.token.as_str()), None] {
        let (status, payload) =
            post(&home, token, &destination.group_id, "send", relay.clone()).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(
            payload["result"]["event"]["data"]["src_group_id"],
            source.group_id
        );
    }
    let incoming = client
        .call(&DaemonRequest {
            v: 1,
            op: "send_cross_group".into(),
            args: json!({
                "group_id":source.group_id,"by":"origin","dst_group_id":destination.group_id,
                "to":["receiver"],"text":"real incoming message","message_mode":"mail"
            })
            .as_object()
            .expect("args")
            .clone(),
        })
        .await
        .expect("cross-group call");
    assert!(incoming.ok, "{:?}", incoming.error);
    let (status, payload) = post(
        &home,
        Some(&guest.token),
        &destination.group_id,
        "reply",
        json!({
            "reply_to":incoming.result["event"]["id"],"text":"normal reply","message_mode":"mail"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload}");
    assert_eq!(payload["result"]["dst_event"]["group_id"], source.group_id);

    // Both ordinary multipart paths still store and reference the uploaded file.
    for (operation, reply) in [("send_upload", None), ("reply_upload", Some(parent))] {
        let (status, payload) = request(
            &home,
            Some(&guest.token),
            &destination.group_id,
            operation,
            "multipart/form-data; boundary=origin-boundary",
            multipart(reply, false),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{operation}: {payload}");
        assert_eq!(
            payload["result"]["event"]["data"]["attachments"][0]["bytes"],
            PNG.len()
        );
    }
    // The actual host-side IM shape still reaches the daemon unchanged.
    let im = client.call(&DaemonRequest { v:1, op:"send".into(), args:json!({
        "group_id":destination.group_id,"by":"user","to":["receiver"],"text":"IM fixture",
        "message_mode":"mail","transport":"im","source_platform":"slack","source_user_id":"im-user"
    }).as_object().expect("args").clone() }).await.expect("IM call");
    assert!(im.ok, "{:?}", im.error);
    assert_eq!(im.result["event"]["data"]["transport"], "im");
    assert_eq!(im.result["event"]["data"]["source_user_id"], "im-user");
    let _ = client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
}
