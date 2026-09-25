use super::*;
use base64::Engine;
use cccc_contracts::ActorRuntime;
use chromiumoxide::cdp::browser_protocol::fetch::{
    EnableParams, EventRequestPaused, FulfillRequestParams,
};
use futures_util::StreamExt;
use std::path::Path;

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
            &super::super::web_model_shared_browser::profile(&state, "chatgpt_web"),
            &local,
            (800, 600),
            &crate::browser_surface::actor_identity(&group.actors[0]),
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
    // Shared Actor open exposes the window before navigation finishes. Wait for
    // this local fixture before installing interception for the next navigation.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if page
                .evaluate("location.protocol === 'http:' && document.readyState === 'complete'")
                .await
                .ok()
                .and_then(|r| r.into_value::<bool>().ok())
                == Some(true)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("fixture navigation complete");
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

#[cfg(target_os = "linux")]
#[tokio::test]
async fn grok_binding_aligns_existing_page_without_viewer_and_preserves_drafts() {
    use cccc_core::web_model_connectors as bindings;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    if crate::system_browser_path().is_none() || !Path::new("/usr/bin/Xvfb").is_file() {
        return;
    }
    let _chrome = crate::browser_surface::chrome_test_guard().await;
    let temp = tempfile::tempdir().expect("temp");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("grok route", "").expect("group");
    let mut actor = cccc_contracts::Actor::new("grok");
    actor.runtime = ActorRuntime::GrokWebModel;
    actor.runner = cccc_contracts::RunnerKind::Headless;
    actor.enabled = false;
    cccc_core::actors::add(&mut group, actor).expect("actor");
    groups.save(&group).expect("save");
    let gid = &group.group_id;
    let connector = bindings::configure_provider(&home, "grok_web").expect("connector");
    let id = connector["connector"]["connector_id"].as_str().expect("id");
    let a = "https://grok.com/bot/00000000-0000-4000-8000-000000000001";
    let b = "https://grok.com/bot/00000000-0000-4000-8000-000000000002";
    bindings::bind_grok(&home, id, gid, "grok", &group.actors[0].generation, a)
        .expect("initial binding");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let address = cccc_contracts::DaemonAddress {
        v: 1,
        transport: cccc_contracts::Transport::Tcp,
        path: String::new(),
        host: "127.0.0.1".into(),
        port: listener.local_addr().expect("address").port(),
        pid: std::process::id(),
        version: "test".into(),
        ts: "test".into(),
    };
    std::fs::write(
        home.daemon_dir().join("ccccd.addr.json"),
        serde_json::to_vec(&address).expect("address"),
    )
    .expect("write address");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await.expect("request");
            let request = serde_json::from_str(&line).expect("request");
            let response = cccc_daemon::handle_request(&daemon_home, &request);
            let mut bytes = serde_json::to_vec(&response).expect("response");
            bytes.push(b'\n');
            stream.get_mut().write_all(&bytes).await.expect("response");
        }
    });
    let (_, _, _, state) = crate::app_with_shutdown(
        home.clone(),
        tokio::sync::broadcast::channel(1).0,
        crate::WebMode::Normal,
        None,
        crate::LiveBinding {
            host: "127.0.0.1".into(),
            port: 0,
        },
        "binding-fixture".into(),
    );
    let key = key(gid, "grok");
    let page_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("page listener");
    let local_url = format!(
        "http://{}/",
        page_listener.local_addr().expect("page address")
    );
    let page_server = tokio::spawn(async move {
        axum::serve(
            page_listener,
            Router::new().fallback(|| async { "fixture" }),
        )
        .await
    });
    state
        .browser_surfaces
        .open(&key, &temp.path().join("profile"), &local_url, 800, 600)
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
                r#"<!doctype html><main><div data-testid="bot-working-slot"></div><div data-testid="chat-input"><div contenteditable="true" role="textbox" class="ProseMirror" style="width:400px;height:100px"></div></div><input type="file"><button data-testid="chat-submit" aria-label="Submit">Submit</button></main>"#
            ).into());
            if responder.execute(response).await.is_err() {
                break;
            }
        }
    });
    page.goto(a).await.expect("Bot A fixture");
    let body = json!({"group_id":gid,"actor_id":"grok","url":b});
    for draft in [
        "const d = new DataTransfer(); d.items.add(new File(['draft'], 'unsent.txt')); document.querySelector('input').files = d.files;",
        "document.querySelector('input').value='';document.querySelector('[contenteditable]').textContent='unsent';",
        "document.querySelector('[contenteditable]').textContent='';document.querySelector('[data-testid=bot-working-slot]').innerHTML='<div style=\"width:100px;height:30px\">Working</div>';",
    ] {
        page.evaluate(format!("() => {{ {draft} }}"))
            .await
            .expect("draft");
        assert!(
            bind_grok(State(state.clone()), Json(body.clone()))
                .await
                .is_err()
        );
        assert_eq!(page.url().await.expect("url").as_deref(), Some(a));
        assert_eq!(
            super::super::web_model_connector_store::for_actor(&state, gid, "grok")
                .expect("binding")["url"],
            a
        );
    }
    page.evaluate("document.querySelector('[data-testid=bot-working-slot]').textContent=''")
        .await
        .expect("idle");
    let result = bind_grok(State(state.clone()), Json(body.clone())).await;
    let url_after_save = page.url().await.expect("url");
    let binding_after_save =
        super::super::web_model_connector_store::for_actor(&state, gid, "grok").expect("binding");
    state.browser_surfaces.close(&key).await.expect("cleanup");
    intercept.abort();
    daemon.abort();
    page_server.abort();
    assert!(result.is_ok(), "save failed: {result:?}");
    assert_eq!(binding_after_save["url"], b);
    assert_eq!(
        url_after_save.as_deref(),
        Some(b),
        "save must align the page without opening the viewer"
    );
}
