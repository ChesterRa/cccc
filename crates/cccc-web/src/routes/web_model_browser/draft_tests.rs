use super::*;
use base64::Engine;
use chromiumoxide::cdp::browser_protocol::fetch::{
    EnableParams, EventRequestPaused, FulfillRequestParams,
};
use futures_util::StreamExt;

#[cfg(target_os = "linux")]
#[tokio::test]
async fn preview_and_saved_alignment_preserve_attachment_only_drafts() {
    if crate::system_browser_path().is_none() || !Path::new("/usr/bin/Xvfb").is_file() {
        return;
    }
    let _chrome = crate::browser_surface::chrome_test_guard().await;
    let temp = tempfile::tempdir().expect("temp");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("draft", "").expect("group");
    let mut actor = cccc_contracts::Actor::new("a");
    actor.runtime = ActorRuntime::WebModel;
    actor.enabled = false;
    cccc_core::actors::add(&mut group, actor).expect("actor");
    groups.save(&group).expect("save");
    let generation = &group.actors[0].generation;
    let gid = &group.group_id;
    use cccc_core::web_model_connectors as bindings;
    let connector = bindings::configure(&home).expect("configure");
    let id = connector["connector"]["connector_id"].as_str().expect("id");
    let pair = bindings::begin_pairing(&home, id, gid, "a", generation, false).expect("begin");
    bindings::accept_pairing(&home, id, pair["code"].as_str().expect("code"), "fixture")
        .expect("accept");
    bindings::confirm_pairing(
        &home,
        id,
        gid,
        "a",
        generation,
        pair["pairing_id"].as_str().expect("pair"),
        "https://chatgpt.com/c/saved",
    )
    .expect("confirm");
    let shutdown = tokio::sync::broadcast::channel(1).0;
    let (_, _, _, state) = crate::app_with_shutdown(
        home.clone(),
        shutdown,
        crate::WebMode::Normal,
        None,
        crate::LiveBinding {
            host: "127.0.0.1".into(),
            port: 0,
        },
        "draft-fixture".into(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let local = format!("http://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().fallback(|| async { "fixture" })).await
    });
    let key = key(gid, "a");
    state
        .browser_surfaces
        .ensure_open_shared_actor(
            &key,
            &browser_profile_path(home.root(), gid, "a").expect("profile"),
            &local,
            (800, 600),
            generation,
        )
        .await
        .expect("open");
    let page = state
        .browser_surfaces
        .sessions
        .lock()
        .await
        .get(&key)
        .expect("session")
        .page
        .clone();
    let mut events = page
        .event_listener::<EventRequestPaused>()
        .await
        .expect("events");
    page.execute(EnableParams::default())
        .await
        .expect("intercept");
    let responder = page.clone();
    let intercept = tokio::spawn(async move {
        while let Some(e) = events.next().await {
            let mut response = FulfillRequestParams::new(e.request_id.clone(), 200);
            response.body = Some(base64::engine::general_purpose::STANDARD.encode(
                "<!doctype html><form><textarea id=prompt-textarea></textarea><input type=file><button type=submit>Send</button></form>"
            ).into());
            if responder.execute(response).await.is_err() {
                break;
            }
        }
    });
    page.goto("https://chatgpt.com/c/draft")
        .await
        .expect("offline conversation");
    page.evaluate("() => { const d = new DataTransfer(); d.items.add(new File(['draft'], 'unsent.txt')); document.querySelector('input').files = d.files; }").await.expect("select file");
    let inspected = state
        .browser_surfaces
        .prompt_readiness(&key)
        .await
        .expect("readiness");
    assert_eq!(inspected["composer_chars"], 0);
    assert_eq!(inspected["running"], false);
    ensure_open_for_actor(&state, gid, "a", 800, 600)
        .await
        .expect("open viewer");
    let aligned_url = page.url().await.expect("url");
    let preview = bind_current(State(state.clone()), Json(json!({"group_id":gid,"actor_id":"a","conversation_url":"https://chatgpt.com/c/another"}))).await;
    let final_url = page.url().await.expect("url");
    let files = page
        .evaluate("document.querySelector('input').files.length")
        .await
        .expect("files")
        .into_value::<u64>()
        .expect("count");
    state
        .browser_surfaces
        .shutdown_all()
        .await
        .expect("cleanup");
    intercept.abort();
    server.abort();
    assert_eq!(
        aligned_url.as_deref(),
        Some("https://chatgpt.com/c/draft"),
        "viewer alignment must preserve an upload"
    );
    assert!(
        preview.is_err(),
        "preview must reject an attachment-only draft"
    );
    assert_eq!(final_url, aligned_url);
    assert_eq!(files, 1, "selected file must survive both actions");
}
