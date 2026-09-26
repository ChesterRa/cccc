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
    check_grok_binding_page(false).await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn grok_binding_replaces_chatgpt_surface_before_navigation() {
    check_grok_binding_page(true).await;
}

#[cfg(target_os = "linux")]
async fn check_grok_binding_page(switch_provider: bool) {
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
    let (_, _, _, mut state) = crate::app_with_shutdown(
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
    // This fixture deliberately retains an old provider page. Drive reaping
    // explicitly below, so the app's periodic reaper cannot remove it during
    // setup before the binding route is exercised.
    state.browser_surfaces =
        std::sync::Arc::new(crate::browser_surface::BrowserSurfaces::default());
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
    let grok_profile = super::super::web_model_shared_browser::profile(&state, "grok_web");
    // Keep this isolated Grok owner available and intercept new targets before
    // navigation. No authenticated session or external provider is contacted.
    state
        .browser_surfaces
        .ensure_open_shared_system(
            "web-model-login:grok_web",
            &grok_profile,
            &local_url,
            800,
            600,
        )
        .await
        .expect("Grok login fixture");
    let grok_requests = if switch_provider {
        Some(
            intercept_fixture_profile(
                state
                    .browser_surfaces
                    .info("web-model-login:grok_web")
                    .await["metadata"]["cdp_port"]
                    .as_u64()
                    .expect("fixture port"),
            )
            .await,
        )
    } else {
        None
    };
    let mut surface_actor = group.actors[0].clone();
    if switch_provider {
        surface_actor.runtime = ActorRuntime::WebModel;
    }
    let surface_profile = super::super::web_model_shared_browser::profile(
        &state,
        surface_actor
            .runtime
            .web_model_provider()
            .expect("provider"),
    );
    state
        .browser_surfaces
        .ensure_open_shared_actor(
            &key,
            &surface_profile,
            &local_url,
            (800, 600),
            &crate::browser_surface::actor_identity(&surface_actor),
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
                r#"<!doctype html><main><div data-testid="bot-working-slot"></div><div data-testid="chat-input"><div contenteditable="true" role="textbox" id="prompt-textarea" class="ProseMirror" style="width:400px;height:100px"></div></div><input type="file"><button data-testid="chat-submit" aria-label="Submit">Submit</button></main>"#
            ).into());
            if responder.execute(response).await.is_err() {
                break;
            }
        }
    });
    let initial_url = if switch_provider {
        "https://chatgpt.com/c/fixture"
    } else {
        a
    };
    page.goto(initial_url).await.expect("initial page fixture");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let ready = page.evaluate(format!("location.href === '{initial_url}' && document.readyState === 'complete' && !!document.querySelector('#prompt-textarea')")).await;
            if ready.is_ok_and(|value| value.into_value::<bool>().unwrap_or(false)) { break; }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await.expect("fixture composer ready");
    let body = json!({"group_id":gid,"actor_id":"grok","url":b});
    for draft in [
        "const d = new DataTransfer(); d.items.add(new File(['draft'], 'unsent.txt')); document.querySelector('input').files = d.files;",
        "document.querySelector('input').value='';document.querySelector('[contenteditable]').textContent='unsent';",
        "document.querySelector('[contenteditable]').textContent='';document.querySelector('[data-testid=bot-working-slot]').innerHTML='<button data-testid=\"stop-button\" aria-label=\"Stop\" style=\"width:100px;height:30px\">Working</button>';",
    ] {
        page.evaluate(format!("() => {{ {draft} }}"))
            .await
            .expect("draft");
        assert!(
            bind_grok(State(state.clone()), Json(body.clone()))
                .await
                .is_err()
        );
        assert_eq!(page.url().await.expect("url").as_deref(), Some(initial_url));
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
    let saved_surface = state.browser_surfaces.info(&key).await;
    let saved_page = state
        .browser_surfaces
        .sessions
        .lock()
        .await
        .get(&key)
        .map(|session| session.page.clone());
    let url_after_save = if let Some(saved_page) = saved_page {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let url = saved_page.url().await.expect("saved URL");
                if url.as_deref() == Some(b) {
                    break url;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
    } else {
        Ok(None)
    };
    let reaped = state
        .browser_surfaces
        .close_missing_actors(&groups)
        .await
        .expect("reap");
    let binding_after_save =
        super::super::web_model_connector_store::for_actor(&state, gid, "grok").expect("binding");
    state
        .browser_surfaces
        .shutdown_all()
        .await
        .expect("cleanup");
    if let Some(task) = grok_requests {
        task.abort();
    }
    intercept.abort();
    daemon.abort();
    page_server.abort();
    assert!(result.is_ok(), "save failed: {result:?}");
    assert_eq!(binding_after_save["url"], b);
    assert_eq!(
        saved_surface["metadata"]["actor_identity"],
        crate::browser_surface::actor_identity(&group.actors[0])
    );
    assert_eq!(
        saved_surface["metadata"]["profile_dir"],
        grok_profile.to_string_lossy().as_ref()
    );
    assert_eq!(reaped, 0, "saved window must belong to the current Actor");
    assert_eq!(
        url_after_save.expect("saved page navigation").as_deref(),
        Some(b),
        "save must align the page without opening the viewer"
    );
}

#[cfg(target_os = "linux")]
async fn intercept_fixture_profile(port: u64) -> tokio::task::JoinHandle<()> {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    let version = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client")
        .get(format!("http://127.0.0.1:{port}/json/version"))
        .send()
        .await
        .expect("fixture CDP")
        .json::<Value>()
        .await
        .expect("CDP metadata");
    let endpoint = version["webSocketDebuggerUrl"]
        .as_str()
        .expect("fixture endpoint");
    let (mut socket, _) = tokio_tungstenite::connect_async(endpoint)
        .await
        .expect("fixture connection");
    socket
        .send(Message::Text(
            json!({"id":1,"method":"Target.setAutoAttach","params":{
                "autoAttach":true,"waitForDebuggerOnStart":true,"flatten":true
            }})
            .to_string()
            .into(),
        ))
        .await
        .expect("intercept new pages");
    let (ready, waiting) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(async move {
        let mut ready = Some(ready);
        let mut id = 1;
        while let Some(Ok(message)) = socket.next().await {
            let Ok(message) = message.to_text() else {
                continue;
            };
            let Ok(event) = serde_json::from_str::<Value>(message) else {
                continue;
            };
            if event["id"] == 1 {
                assert!(
                    event.get("error").is_none(),
                    "fixture interception rejected"
                );
                let _ = ready.take().expect("one reply").send(());
            }
            let commands = match event["method"].as_str() {
                Some("Target.attachedToTarget") => vec![
                    (
                        event["params"]["sessionId"].clone(),
                        "Fetch.enable",
                        json!({}),
                    ),
                    (
                        event["params"]["sessionId"].clone(),
                        "Runtime.runIfWaitingForDebugger",
                        json!({}),
                    ),
                ],
                Some("Fetch.requestPaused") => vec![(
                    event["sessionId"].clone(),
                    "Fetch.fulfillRequest",
                    json!({
                        "requestId":event["params"]["requestId"], "responseCode":200,
                        "body":base64::engine::general_purpose::STANDARD.encode("<!doctype html><p>isolated Grok fixture</p>"),
                        "responseHeaders":[{"name":"Content-Type","value":"text/html"}]
                    }),
                )],
                _ => vec![],
            };
            for (session, method, params) in commands {
                id += 1;
                if socket
                    .send(Message::Text(
                        json!({"id":id,"sessionId":session,"method":method,"params":params})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    });
    waiting.await.expect("interception ready");
    handle
}
