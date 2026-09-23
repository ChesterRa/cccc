use super::*;
use base64::Engine;
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest};
use cccc_core::{GroupStore, HomeLayout};
use chromiumoxide::{
    Page,
    cdp::browser_protocol::fetch::{
        EnableParams, EventRequestPaused, FulfillRequestParams, HeaderEntry,
    },
};
use futures_util::StreamExt;

// Fetch interception serves synthetic ChatGPT-origin HTML and tool replies.
// No request goes to ChatGPT, no login/profile/provider credits are used.
const HTML: &str = r#"<!doctype html><main><form onsubmit="send(event)"><textarea id="prompt-textarea" style="width:500px;height:120px"></textarea><button type="submit">Send</button></form></main><script>
window.submits=0; window.holdReceipt=true; window.holdCall=false;
async function send(e) {
 e.preventDefault(); const t=document.querySelector('textarea'); const text=t.value; window.submits++;
 const u=document.createElement('article'); u.dataset.messageAuthorRole='user';u.textContent=text;document.querySelector('main').append(u);t.value='';
 if(location.pathname==='/') history.replaceState(null,'','/c/WEB:00000000-0000-4000-8000-000000000001');
 if(window.holdCall) return;
 const match=text.match(/wm_pair_[a-f0-9]+/);
 if(!match) { (window.business ||= []).push(text); return; }
 const code=match[0]; window.reply=null;
 const r=await (await fetch('/pair-tool?code='+encodeURIComponent(code))).json();
 window.reply=r.result.structuredContent;
 if(!window.holdReceipt) showReceipt();
}
function showReceipt(){ const a=document.createElement('article');a.dataset.messageAuthorRole='assistant';a.textContent=window.reply.receipt;document.querySelector('main').append(a); }
</script>"#;

async fn offline_page(
    state: &AppState,
    group: &str,
    actor: &str,
    profile: &std::path::Path,
    root_url: &str,
) -> (Page, tokio::task::JoinHandle<()>) {
    let key = super::super::web_model_browser::key(group, actor);
    state
        .browser_surfaces
        .open(&key, profile, root_url, 800, 600)
        .await
        .expect("fixture browser");
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
        .expect("intercept");
    page.execute(EnableParams::default())
        .await
        .expect("intercept all requests");
    let control = page.clone();
    let app = state.clone();
    let session = actor.to_owned();
    let handler = tokio::spawn(async move {
        while let Some(e) = events.next().await {
            let body = if e.request.url.contains("/pair-tool?") {
                let url = reqwest::Url::parse(&e.request.url).expect("fixture URL");
                let code = url
                    .query_pairs()
                    .find(|(key, _)| key == "code")
                    .expect("fixture code")
                    .1
                    .to_string();
                let posted = json!({"code":code});
                let connector = store::load(&app).expect("connector")[0].clone();
                super::super::web_model_connector_session::handle(&app, &connector, &json!({
                    "jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"cccc_pair","arguments":posted,"_meta":{"openai/session":session}}
                })).await.expect("MCP pairing").to_string()
            } else {
                HTML.to_owned()
            };
            let mut response = FulfillRequestParams::new(e.request_id.clone(), 200);
            response.response_headers = Some(vec![HeaderEntry::new(
                "Content-Type",
                if e.request.url.contains("/pair-tool?") {
                    "application/json"
                } else {
                    "text/html"
                },
            )]);
            response.body = Some(
                base64::engine::general_purpose::STANDARD
                    .encode(body)
                    .into(),
            );
            if control.execute(response).await.is_err() {
                break;
            }
        }
    });
    page.goto(format!("https://chatgpt.com/c/{actor}"))
        .await
        .expect("offline ChatGPT origin");
    (page, handler)
}
async fn page_value(page: &Page, script: &str) -> Value {
    page.evaluate(script)
        .await
        .expect("evaluate")
        .into_value()
        .unwrap_or(Value::Null)
}
async fn wait_page(page: &Page, script: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while page_value(page, script).await != true {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("page condition");
}
async fn show_receipt(page: &Page) {
    // Daemon acceptance precedes delivery of the HTTP response to the page.
    // Wait for the browser to receive it before simulating the assistant reply.
    wait_page(page, "!!window.reply?.receipt").await;
    page_value(page, "showReceipt()").await;
}
fn stored_pair(state: &AppState, group: &str, actor: &str) -> Option<Value> {
    web_model_connectors::pairing_for_actor(&store::load(state).expect("load")[0], group, actor)
}
async fn wait_pair(state: &AppState, group: &str, actor: &str, expected: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Some(p) = stored_pair(state, group, actor)
                && p["state"] == expected
            {
                return p;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("pair state")
}
async fn connect_actor(state: &AppState, group: &str, actor: &str) -> Value {
    change(
        State(state.clone()),
        Json(json!({"group_id":group,"actor_id":actor,"action":"connect"})),
    )
    .await
    .expect("start connection")
    .0["result"]
        .clone()
}

#[tokio::test]
async fn automatic_pairing_verifies_owned_receipts_and_preserves_drafts_routes_and_cancellation() {
    if crate::system_browser_path().is_none() {
        return;
    }
    let _chrome = crate::browser_surface::chrome_test_guard().await;
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("init");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("pairing", "").expect("group");
    for id in ["alpha", "beta"] {
        let mut actor = Actor::new(id);
        actor.runtime = ActorRuntime::WebModel;
        actor.enabled = false;
        cccc_core::actors::add(&mut group, actor).expect("actor");
    }
    groups.save(&group).expect("save");
    let configured = web_model_connectors::configure(&home).expect("connector");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    tokio::time::timeout(Duration::from_secs(5), async {
        while client
            .call(&DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Map::new(),
            })
            .await
            .is_err()
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("daemon ready");
    let shutdown = tokio::sync::broadcast::channel(1).0;
    let (_, _, _, state) = crate::app_with_shutdown(
        home.clone(),
        shutdown.clone(),
        crate::WebMode::Normal,
        None,
        crate::LiveBinding {
            host: "127.0.0.1".into(),
            port: 0,
        },
        "pairing-fixture".into(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let root_url = format!("http://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/", axum::routing::get(|| async { "offline entry" })),
        )
        .await
    });
    let gid = &group.group_id;
    let (a, ah) = offline_page(&state, gid, "alpha", &temp.path().join("alpha"), &root_url).await;
    let (b, bh) = offline_page(&state, gid, "beta", &temp.path().join("beta"), &root_url).await;
    page_value(
        &a,
        "document.querySelector('textarea').value='Keep my private draft'",
    )
    .await;
    let result = change(
        State(state.clone()),
        Json(json!({"group_id":gid,"actor_id":"alpha","action":"connect"})),
    )
    .await;
    assert!(
        result
            .expect_err("must protect draft")
            .to_string()
            .contains("pairing_composer_occupied")
    );
    assert!(stored_pair(&state, gid, "alpha").is_none());
    assert_eq!(
        page_value(&a, "document.querySelector('textarea').value").await,
        "Keep my private draft"
    );
    assert_eq!(page_value(&a, "window.submits").await, 0);
    page_value(&a, "document.querySelector('textarea').value=''").await;
    page_value(&a, "document.querySelector('form').insertAdjacentHTML('beforeend','<div data-testid=attachment>Unsent file</div>')").await;
    assert!(
        change(
            State(state.clone()),
            Json(json!({"group_id":gid,"actor_id":"alpha","action":"connect"}))
        )
        .await
        .expect_err("pending attachment")
        .to_string()
        .contains("pairing_composer_occupied")
    );
    page_value(
        &a,
        "document.querySelector('[data-testid=attachment]').remove()",
    )
    .await;
    // A connector icon and the upload button are not unsent user attachments.
    page_value(&a, "document.querySelector('form').insertAdjacentHTML('beforeend','<img alt=CCCC width=24 height=24 src=/connector-icon.png><button type=button data-testid=attachment-button>Add files</button>')").await;
    let started = connect_actor(&state, gid, "alpha").await;
    assert!(
        started["code"].is_null(),
        "no copyable code in the UI response"
    );
    assert!(
        change(
            State(state.clone()),
            Json(json!({"group_id":gid,"actor_id":"alpha","action":"connect"}))
        )
        .await
        .is_err(),
        "duplicate click cannot send twice"
    );
    wait_page(&a, "!!window.reply?.receipt").await;
    let accepted = wait_pair(&state, gid, "alpha", "awaiting_confirmation").await;
    assert!(
        web_model_connectors::binding_for_session(
            &store::load(&state).expect("store")[0],
            accepted["session_key"].as_str().expect("session")
        )
        .is_none(),
        "MCP accept alone grants nothing"
    );
    page_value(&b,&format!("document.body.insertAdjacentHTML('beforeend', '<article data-message-author-role=\"assistant\">'+{}+'</article>')",accepted["receipt"])).await;
    // User-role echo in A must also not count as an assistant reply.
    page_value(
        &a,
        &format!(
            "document.querySelector('textarea').value={}",
            accepted["receipt"]
        ),
    )
    .await;
    let target = state
        .browser_surfaces
        .pairing_page(&super::super::web_model_browser::key(gid, "beta"))
        .await
        .expect("B remains independently ready");
    assert!(
        target
            .receipt_url(
                &state.browser_surfaces,
                &super::super::web_model_browser::key(gid, "beta"),
                "not sent here",
                accepted["receipt"].as_str()
            )
            .await
            .expect("inspect B")
            .is_none()
    );
    page_value(
        &a,
        "document.querySelector('textarea').value='';showReceipt()",
    )
    .await;
    wait_pair(&state, gid, "alpha", "bound").await;
    assert_eq!(page_value(&a, "window.submits").await, 1);

    // B can pair independently through the same connector, including a new chat.
    page_value(
        &b,
        "history.replaceState(null,'','/');window.holdReceipt=false",
    )
    .await;
    let beta_key = super::super::web_model_browser::key(gid, "beta");
    let new_chat = state
        .browser_surfaces
        .pairing_page(&beta_key)
        .await
        .expect("new chat");
    // Real ChatGPT first exposes /c/WEB:<uuid>, then replaces it with the
    // server conversation ID. Before MCP acceptance this must remain pending.
    page_value(
        &b,
        "history.replaceState(null,'','/c/WEB:00000000-0000-4000-8000-000000000001')",
    )
    .await;
    assert!(
        new_chat
            .receipt_url(&state.browser_surfaces, &beta_key, "not sent yet", None)
            .await
            .expect("provisional new chat is not a target change")
            .is_none()
    );
    assert!(
        state
            .browser_surfaces
            .pairing_page(&beta_key)
            .await
            .is_err(),
        "a provisional URL cannot start a separate pairing"
    );
    page_value(&b, "history.replaceState(null,'','/')").await;
    connect_actor(&state, gid, "beta").await;
    wait_page(&b, "!!window.reply?.receipt").await;
    let pending = wait_pair(&state, gid, "beta", "awaiting_confirmation").await;
    let prompt = page_value(
        &b,
        "document.querySelector('[data-message-author-role=user]').innerText",
    )
    .await;
    assert!(
        new_chat
            .receipt_url(
                &state.browser_surfaces,
                &beta_key,
                prompt.as_str().expect("setup text"),
                pending["receipt"].as_str()
            )
            .await
            .expect("receipt on a provisional URL waits")
            .is_none()
    );
    assert!(
        web_model_connectors::binding_for_session(
            &store::load(&state).expect("store")[0],
            pending["session_key"].as_str().expect("session")
        )
        .is_none(),
        "a visible receipt cannot authorize a provisional conversation"
    );
    page_value(&b, "history.replaceState(null,'','/c/new-fixture')").await;
    wait_pair(&state, gid, "beta", "bound").await;
    assert_eq!(page_value(&b, "window.submits").await, 1);
    let stable_beta = state
        .browser_surfaces
        .pairing_page(&beta_key)
        .await
        .expect("stable B");
    page_value(
        &b,
        "history.replaceState(null,'','/c/WEB:00000000-0000-4000-8000-000000000001')",
    )
    .await;
    assert_eq!(
        stable_beta
            .receipt_url(&state.browser_surfaces, &beta_key, "", None)
            .await
            .expect_err("existing conversation cannot switch to provisional chat")
            .to_string(),
        "pairing_target_changed"
    );
    page_value(&b, "history.replaceState(null,'','/c/new-fixture')").await;
    let connector = store::load(&state).expect("store")[0].clone();
    assert_eq!(
        connector["bindings"].as_object().expect("bindings").len(),
        2
    );
    let before = connector["bindings"].clone();

    // Cancel while receipt is held. A late receipt cannot replace the old binding.
    page_value(&a, "window.reply=null;window.holdReceipt=true").await;
    let cancelled = connect_actor(&state, gid, "alpha").await;
    wait_page(&a, "!!window.reply?.receipt").await;
    assert_eq!(
        page_value(&a, "window.submits").await,
        2,
        "a new pairing code must be sent even when setup messages share a prefix"
    );
    let _ = change(State(state.clone()),Json(json!({"group_id":gid,"actor_id":"alpha","action":"cancel","pairing_id":cancelled["pairing_id"]}))).await.expect("cancel");
    show_receipt(&a).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while is_connecting(&state, cancelled["pairing_id"].as_str().expect("id")) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("cancel task ends");
    assert_eq!(
        stored_pair(&state, gid, "alpha").expect("cancel retained")["state"],
        "cancelled"
    );
    assert_eq!(store::load(&state).expect("store")[0]["bindings"], before);

    // A changed conversation rejects receipt completion and retains both routes.
    page_value(&a, "window.reply=null").await;
    connect_actor(&state, gid, "alpha").await;
    wait_page(&a, "!!window.reply?.receipt").await;
    page_value(
        &a,
        "history.replaceState(null,'','/c/wrong-conversation');showReceipt()",
    )
    .await;
    let failed = wait_pair(&state, gid, "alpha", "failed").await;
    assert_eq!(failed["error_code"], "pairing_target_changed");
    assert_eq!(store::load(&state).expect("store")[0]["bindings"], before);

    // A cleared composer with no user-message echo is ambiguous, never retried.
    page_value(&b, "window.uncertainSends=0;document.querySelector('form').onsubmit=e=>{e.preventDefault();window.uncertainSends++;document.querySelector('textarea').value='';}").await;
    connect_actor(&state, gid, "beta").await;
    let uncertain = wait_pair(&state, gid, "beta", "failed").await;
    assert_eq!(uncertain["error_code"], "pairing_submission_uncertain");
    assert_eq!(page_value(&b, "window.uncertainSends").await, 1);
    assert_eq!(store::load(&state).expect("store")[0]["bindings"], before);

    // An interrupted old attempt is observational only after a Web restart.
    let old = web_model_connectors::begin_pairing(
        &home,
        configured["connector"]["connector_id"]
            .as_str()
            .expect("id"),
        gid,
        "alpha",
        &group.actors[0].generation,
        false,
    )
    .expect("orphan");
    assert!(!is_connecting(
        &state,
        old["pairing_id"].as_str().expect("id")
    ));
    assert_eq!(page_value(&a, "window.submits").await, 3);

    // Normal startup connects both Actors with no pairing POST, and leaves the
    // real message queued until both the MCP call and owned receipt are proven.
    for actor in ["alpha", "beta"] {
        web_model_connectors::retire_actor(&home, gid, actor).expect("reset fixture routes");
    }
    a.goto("https://chatgpt.com/c/alpha")
        .await
        .expect("fresh page");
    page_value(&a, "history.replaceState(null,'','/')").await;
    b.goto("https://chatgpt.com/c/beta")
        .await
        .expect("fresh page");
    let invoke = |op: &str, args: Value| {
        let app = state.clone();
        let op = op.to_owned();
        async move {
            call(&app, &op, args.as_object().expect("args").clone())
                .await
                .expect("daemon operation")
        }
    };
    for actor in ["alpha", "beta"] {
        invoke(
            "actor_start",
            json!({"group_id":gid,"actor_id":actor,"by":"user"}),
        )
        .await;
    }
    let message = invoke(
        "message_send",
        json!({"group_id":gid,"by":"user","to":["alpha"],"text":"Unique queued task 84973","message_mode":"send"}),
    )
    .await;
    assert!(!message.is_null());
    page_value(
        &a,
        "document.querySelector('textarea').value='preserve before automatic connection'",
    )
    .await;
    super::super::web_model_supervisor::ensure_running_actor(&state, Some(gid), true).await;
    assert!(
        stored_pair(&state, gid, "alpha").is_none(),
        "draft defers initial send"
    );
    assert_eq!(page_value(&a, "window.submits").await, 0);
    wait_pair(&state, gid, "beta", "awaiting_confirmation").await;
    page_value(&a, "document.querySelector('textarea').value=''").await;
    super::super::web_model_supervisor::ensure_running_actor(&state, Some(gid), false).await;
    wait_pair(&state, gid, "alpha", "awaiting_confirmation").await;
    assert_eq!(page_value(&a, "window.submits").await, 1);
    assert_eq!(page_value(&a, "(window.business||[]).length").await, 0);
    let pending = cccc_core::integration_state::group_get(&groups, gid, "runtime_states")
        .expect("runtime state");
    assert_ne!(pending["alpha"]["status"], "working");
    show_receipt(&a).await;
    assert!(store::for_actor(&state, gid, "alpha").is_none());
    assert_eq!(page_value(&a, "(window.business||[]).length").await, 0);
    page_value(&a, "history.replaceState(null,'','/c/alpha')").await;
    show_receipt(&b).await;
    wait_pair(&state, gid, "alpha", "bound").await;
    wait_pair(&state, gid, "beta", "bound").await;
    wait_page(
        &a,
        "(window.business||[]).some(t=>t.includes('Unique queued task 84973'))",
    )
    .await;
    assert_eq!(
        page_value(
            &a,
            "window.business.filter(t=>t.includes('Unique queued task 84973')).length"
        )
        .await,
        1
    );
    assert_eq!(
        page_value(
            &b,
            "(window.business||[]).some(t=>t.includes('Unique queued task 84973'))"
        )
        .await,
        false
    );
    let bound = store::for_actor(&state, gid, "beta").expect("B bound");
    // A normal restart uses the established binding, without another handshake.
    invoke(
        "actor_stop",
        json!({"group_id":gid,"actor_id":"beta","by":"user"}),
    )
    .await;
    invoke(
        "actor_start",
        json!({"group_id":gid,"actor_id":"beta","by":"user"}),
    )
    .await;
    super::super::web_model_supervisor::ensure_running_actor(&state, Some(gid), true).await;
    assert_eq!(
        store::for_actor(&state, gid, "beta").expect("B still bound")["revision"],
        bound["revision"]
    );
    assert_eq!(page_value(&b, "window.submits").await, 1);

    // Rapid stop/start cannot revive an accepted but unverified handshake.
    invoke(
        "actor_stop",
        json!({"group_id":gid,"actor_id":"beta","by":"user"}),
    )
    .await;
    web_model_connectors::retire_actor(&home, gid, "beta").expect("reset B route");
    b.goto("https://chatgpt.com/c/beta").await.expect("fresh B");
    invoke(
        "actor_start",
        json!({"group_id":gid,"actor_id":"beta","by":"user"}),
    )
    .await;
    super::super::web_model_supervisor::ensure_running_actor(&state, Some(gid), true).await;
    wait_pair(&state, gid, "beta", "awaiting_confirmation").await;
    invoke(
        "actor_stop",
        json!({"group_id":gid,"actor_id":"beta","by":"user"}),
    )
    .await;
    invoke(
        "actor_start",
        json!({"group_id":gid,"actor_id":"beta","by":"user"}),
    )
    .await;
    show_receipt(&b).await;
    let interrupted = wait_pair(&state, gid, "beta", "failed").await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while is_connecting(&state, interrupted["pairing_id"].as_str().expect("id")) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("old task exits");
    super::super::web_model_supervisor::ensure_running_actor(&state, Some(gid), false).await;
    assert_eq!(
        page_value(&b, "window.submits").await,
        1,
        "interrupted send not retried"
    );
    assert!(store::for_actor(&state, gid, "beta").is_none());
    // Explicit retry is available while running. Cancelling it also fences timers.
    page_value(&b, "window.reply=null").await;
    let retry = connect_actor(&state, gid, "beta").await;
    wait_pair(&state, gid, "beta", "awaiting_confirmation").await;
    let _ = change(State(state.clone()),Json(json!({"group_id":gid,"actor_id":"beta","action":"cancel","pairing_id":retry["pairing_id"]}))).await.expect("cancel running attempt");
    show_receipt(&b).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while is_connecting(&state, retry["pairing_id"].as_str().expect("id")) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("cancel task exits");
    super::super::web_model_supervisor::ensure_running_actor(&state, Some(gid), false).await;
    assert_eq!(page_value(&b, "window.submits").await, 2);
    assert_eq!(
        stored_pair(&state, gid, "beta").expect("cancel fence")["state"],
        "cancelled"
    );
    assert!(store::for_actor(&state, gid, "beta").is_none());
    page_value(&b, "window.holdReceipt=false").await;
    connect_actor(&state, gid, "beta").await;
    wait_pair(&state, gid, "beta", "bound").await;

    // Group pause/resume also invalidates an in-progress handshake, even when
    // the Actor remains enabled. Neither resume nor an expired orphan resends.
    invoke(
        "actor_stop",
        json!({"group_id":gid,"actor_id":"beta","by":"user"}),
    )
    .await;
    web_model_connectors::retire_actor(&home, gid, "beta").expect("reset B route");
    b.goto("https://chatgpt.com/c/beta").await.expect("fresh B");
    invoke(
        "actor_start",
        json!({"group_id":gid,"actor_id":"beta","by":"user"}),
    )
    .await;
    super::super::web_model_supervisor::ensure_running_actor(&state, Some(gid), true).await;
    wait_pair(&state, gid, "beta", "awaiting_confirmation").await;
    invoke(
        "group_set_state",
        json!({"group_id":gid,"state":"paused","by":"user"}),
    )
    .await;
    invoke(
        "group_set_state",
        json!({"group_id":gid,"state":"active","by":"user"}),
    )
    .await;
    show_receipt(&b).await;
    let paused = wait_pair(&state, gid, "beta", "failed").await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while is_connecting(&state, paused["pairing_id"].as_str().expect("id")) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("paused task exits");
    super::super::web_model_supervisor::ensure_running_actor(&state, Some(gid), true).await;
    assert!(store::for_actor(&state, gid, "beta").is_none());
    let cid = configured["connector"]["connector_id"]
        .as_str()
        .expect("id");
    let orphan = web_model_connectors::begin_pairing(
        &home,
        cid,
        gid,
        "beta",
        &group.actors[1].generation,
        true,
    )
    .expect("orphan after restart");
    assert!(!is_connecting(
        &state,
        orphan["pairing_id"].as_str().expect("id")
    ));
    super::super::web_model_supervisor::ensure_running_actor(&state, Some(gid), true).await;
    store::update_connector(&state, cid, |c| {
        for p in c["pairings"].as_object_mut().expect("pairs").values_mut() {
            if p["actor_id"] == "beta" {
                p["expires_at_ms"] = json!(0);
            }
        }
    })
    .expect("expire orphan");
    super::super::web_model_supervisor::ensure_running_actor(&state, Some(gid), true).await;
    assert_eq!(
        page_value(&b, "window.submits").await,
        1,
        "no resumed or expired replay"
    );
    // A browser read failure does not prove that the user changed conversations.
    // It still fails closed and must not change another Actor's binding.
    let alpha_binding = store::for_actor(&state, gid, "alpha");
    page_value(&b, "window.reply=null").await;
    connect_actor(&state, gid, "beta").await;
    wait_pair(&state, gid, "beta", "awaiting_confirmation").await;
    page_value(&b, r#"() => {
        const query = document.querySelectorAll.bind(document);
        document.querySelectorAll = s => {
            if (s.includes('data-message-author-role="user"') && s.includes('data-message-author-role="assistant"')) throw new Error('fixture inspection failure');
            return query(s);
        };
    }"#).await;
    let failed = wait_pair(&state, gid, "beta", "failed").await;
    assert_eq!(failed["error_code"], "pairing_interrupted");
    assert!(store::for_actor(&state, gid, "beta").is_none());
    assert_eq!(store::for_actor(&state, gid, "alpha"), alpha_binding);
    state
        .browser_surfaces
        .shutdown_all()
        .await
        .expect("close fixtures");
    ah.abort();
    bh.abort();
    server.abort();
    let _ = shutdown.send(());
    client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await
        .expect("shutdown");
    daemon.await.expect("task").expect("daemon");
}
