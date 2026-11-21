//! The command and its grant/QUIC connection are real; the two-device approval UI is a local HTTP fixture.
#[path = "session_node_join/api.rs"]
mod api;
#[path = "session_node_join/failures.rs"]
mod failures;
#[path = "session_node_join/homes.rs"]
mod homes;
use api::Api;
use axum::http::StatusCode;
use cccc_contracts::session_node_join::{CreateRequest, PollRequest};
use cccc_core::session_nodes;
use homes::{Homes, connection, output};
use serde_json::{Value, json};

fn created(id: &str, expiry: &str) -> Value {
    json!({"ok":true,"result":{"request_id":id,"user_code":"ABCD2345","expires_at":expiry}})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn join_approval_is_single_use_and_resume_skips_http() {
    let mut f = Homes::new();
    f.start().await;
    let mut api = Api::start().await;
    let mut first = f.join(&api.origin, &["--name", "Test laptop"]);
    let create = api.next(&mut first, "requests").await;
    let requested: CreateRequest =
        serde_json::from_value(create.body.clone()).expect("creation contract");
    assert_eq!(requested.name, "Test laptop");
    assert!(
        requested.device_code.len() == 64
            && requested.device_code.bytes().all(|b| b.is_ascii_hexdigit())
    );
    create.answer(StatusCode::SERVICE_UNAVAILABLE, json!({}));
    let retry = api.next(&mut first, "requests").await;
    let repeated: CreateRequest =
        serde_json::from_value(retry.body.clone()).expect("creation retry");
    assert!(
        repeated.device_code == requested.device_code,
        "a retried request keeps its private claim"
    );
    assert_eq!(repeated.node_id, requested.node_id);
    let id = uuid::Uuid::new_v4().to_string();
    let expiry = (chrono::Utc::now() + chrono::Duration::minutes(2)).to_rfc3339();
    retry.answer(StatusCode::OK, created(&id, &expiry));
    let pending = api.next(&mut first, "poll").await;
    let claim: PollRequest =
        serde_json::from_value(pending.body.clone()).expect("private poll contract");
    assert_eq!(claim.request_id, id);
    assert!(claim.device_code == requested.device_code);
    pending.answer(
        StatusCode::OK,
        json!({"ok":true,"result":{"status":"pending"}}),
    );
    let approved = api.next(&mut first, "poll").await;
    assert!(
        session_nodes::saved_client(&f.node)
            .expect("preapproval state")
            .is_none()
    );
    assert!(
        session_nodes::peers(&f.hub)
            .expect("preapproval peers")
            .is_empty()
    );
    let generation = session_nodes::capture_join_generation(&f.hub, &requested.node_id, &expiry)
        .expect("revocation generation");
    let mut ticket = cccc_web::session_node::create_bound_invitation(
        &f.hub,
        &requested.node_id,
        &expiry,
        &generation,
    )
    .await
    .expect("isolated key-bound hub ticket");
    ticket.addresses.retain(|a| {
        a.parse::<std::net::SocketAddr>()
            .is_ok_and(|a| a.ip().is_loopback())
    });
    let wire = cccc_web::session_node::encode_ticket(&ticket).expect("encode synthetic ticket");
    approved.answer(
        StatusCode::OK,
        json!({"ok":true,"result":{"status":"approved","ticket":wire}}),
    );
    let private = [&requested.device_code[..], &ticket.secret[..], &wire[..]];
    let (paired, first_progress) = connection(&f.node, &mut first, &private).await;
    let binding = paired.join_binding.as_ref().expect("confirmed origin");
    assert_eq!(binding.origin, format!("{}/", api.origin));
    assert_eq!(binding.hub_id, ticket.endpoint_id);
    assert_eq!(paired.node_id, requested.node_id);
    let peers = session_nodes::peers(&f.hub).expect("registered device");
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].grant_id, paired.grant_id);
    first.start_kill().expect("stop fixture connector");
    let (_, remainder) = output(first, &private).await;
    let text = first_progress + &remainder;
    assert!(
        text.contains(&format!("{}/?session_node_join={id}", api.origin))
            && text.contains("ABCD2345")
    );
    assert!(text.contains("Device: Test laptop\nVerification code: ABCD2345"));
    assert!(
        text.find("Device approved.").expect("approval progress")
            < text
                .find("Session node connected.")
                .expect("commit progress")
    );

    #[cfg(unix)]
    let resume_args = {
        f.install_fake_codex();
        &["--runtime", "codex"][..]
    };
    #[cfg(not(unix))]
    let resume_args: &[&str] = &[];
    let mut resumed = f.join(&format!("{}/", api.origin), resume_args);
    let (restored, resume_progress) = tokio::select! {
        value=connection(&f.node,&mut resumed, &private)=>value,
        _=api.calls.recv()=>panic!("a matching saved grant must resume without requesting approval"),
    };
    assert_eq!(restored.grant_id, paired.grant_id);
    assert_eq!(restored.node_id, paired.node_id);
    resumed.start_kill().expect("stop fixture resume");
    let (_, remainder) = output(resumed, &private).await;
    let text = resume_progress + &remainder;
    assert!(
        text.contains("Resuming")
            && text.contains("--reauthorize")
            && text.contains("Session node connected.")
    );
    #[cfg(unix)]
    assert!(
        text.contains("Native codex integration enabled") && f.native_calls().contains("codex:mcp")
    );
    let saved = f.descriptor_bytes();
    let mut retry = f.join(&api.origin, &["--reauthorize", "--runtime", "codex"]);
    let fresh = api.next(&mut retry, "requests").await;
    let value: CreateRequest = serde_json::from_value(fresh.body.clone()).expect("new request");
    assert_eq!(value.node_id, requested.node_id);
    assert!(value.device_code != requested.device_code);
    assert!(!value.name.trim().is_empty());
    fresh.answer(StatusCode::OK, created(&id, &expiry));
    api.next(&mut retry, "poll").await.answer(
        StatusCode::OK,
        json!({"ok":true,"result":{"status":"denied"}}),
    );
    let (ok, text) = output(retry, &[&value.device_code, &wire]).await;
    assert!(!ok && text.contains("denied") && !text.contains("integration enabled"));
    assert!(
        f.descriptor_bytes() == saved,
        "denied reauthorization must preserve the prior grant and origin"
    );

    let mut other = Api::start().await;
    let mut changed = f.join(&other.origin, &[]);
    let create = other.next(&mut changed, "requests").await;
    create.answer(StatusCode::OK, created(&id, &expiry));
    other.next(&mut changed, "poll").await.answer(
        StatusCode::OK,
        json!({"ok":true,"result":{"status":"expired"}}),
    );
    let (ok, text) = output(changed, &[&wire]).await;
    assert!(!ok && text.contains("expired"));
    assert!(
        f.descriptor_bytes() == saved,
        "an unapproved second origin cannot take over reconnect"
    );
    cccc_web::session_node::revoke(&f.hub, &paired.node_id)
        .await
        .expect("revoke the real fixture grant");
    let revoked = f.join(&api.origin, &[]);
    let protected = [&wire[..], &requested.device_code[..], &ticket.secret[..]];
    let (ok, text) = tokio::select! {
        value=output(revoked, &protected)=>value,
        _=api.calls.recv()=>panic!("revocation must not silently create a new approval"),
    };
    assert!(!ok && text.contains("rejected") && text.contains("--reauthorize"));
    assert!(
        f.descriptor_bytes() == saved,
        "a rejected Hello cannot overwrite the saved grant"
    );
    f.close().await;
}
