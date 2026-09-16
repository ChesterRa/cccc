use super::*;
use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod chrome_test_guard;
use chrome_test_guard::chrome_test_guard;

macro_rules! require_chrome {
    () => {
        if !chrome_available() {
            return;
        }
        let _chrome_guard = chrome_test_guard().await;
    };
}

#[test]
fn extracts_google_account_route_from_completion_url() {
    assert_eq!(
        authuser_from_url("https://notebooklm.google.com/?authuser=2"),
        2
    );
    assert_eq!(
        authuser_from_url("https://notebooklm.google.com/u/3/notebook/x"),
        3
    );
    assert_eq!(authuser_from_url("https://notebooklm.google.com/"), 0);
}

#[tokio::test]
async fn launches_chromium_and_captures_nonempty_frame() {
    require_chrome!();
    let (url, server) = local_page(
        "<!doctype html><html><body style='background:#fff'><h1>CCCC browser frame</h1><input autofocus></body></html>",
    )
    .await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let state = manager
        .open(
            "g_test::slot-1",
            &temp.path().join("profile"),
            &url,
            1120,
            760,
        )
        .await
        .expect("open");
    assert_eq!(state["state"], "ready");
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    let frame = manager.frame("g_test::slot-1").await.expect("frame");
    let image = base64::engine::general_purpose::STANDARD
        .decode(frame["data_base64"].as_str().expect("base64"))
        .expect("jpeg");
    assert!(image.len() > 1_000);
    assert_eq!(&image[..2], &[0xff, 0xd8]);
    assert_eq!(frame["width"], 1120);
    assert_eq!(frame["height"], 760);
    assert!(manager.close("g_test::slot-1").await.expect("close"));
    server.abort();
}

#[tokio::test]
async fn open_completes_after_dom_content_loaded_without_waiting_for_subresources() {
    require_chrome!();
    let (url, server) = page_with_stalled_subresource().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();

    let state = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        manager.open(
            "dom-content-loaded",
            &temp.path().join("profile"),
            &url,
            800,
            600,
        ),
    )
    .await
    .expect("open must not wait for the stalled image")
    .expect("open browser");

    assert_eq!(state["state"], "ready");
    let page = manager
        .sessions
        .lock()
        .await
        .get("dom-content-loaded")
        .expect("browser session")
        .page
        .clone();
    let heading: String = page
        .evaluate("document.querySelector('h1')?.textContent || ''")
        .await
        .expect("evaluate destination document")
        .into_value()
        .expect("heading text");
    assert_eq!(heading, "DOMContentLoaded");
    assert!(manager.close("dom-content-loaded").await.expect("close"));
    server.abort();
}

#[tokio::test]
async fn concurrent_ensure_open_reuses_one_profile_owner() {
    require_chrome!();
    let (url, server) = local_page("CCCC concurrent browser").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let profile = temp.path().join("profile");

    let (first, second) = tokio::join!(
        manager.ensure_open("web-model::g_test::actor", &profile, &url, 800, 600),
        manager.ensure_open("web-model::g_test::actor", &profile, &url, 800, 600),
    );
    let first = first.expect("first open");
    let second = second.expect("second open");

    assert_eq!(first["started_at"], second["started_at"]);
    assert_eq!(manager.sessions.lock().await.len(), 1);
    assert!(
        manager
            .close("web-model::g_test::actor")
            .await
            .expect("close")
    );
    server.abort();
}

#[tokio::test]
async fn reopens_same_profile_after_process_exit() {
    require_chrome!();
    let (url, server) = local_page("CCCC reopen browser").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let profile = temp.path().join("profile");

    manager
        .open("restartable", &profile, &url, 800, 600)
        .await
        .expect("first open");
    manager
        .open("restartable", &profile, &url, 800, 600)
        .await
        .expect("second open");

    assert!(manager.close("restartable").await.expect("close"));
    server.abort();
}

#[tokio::test]
async fn info_reaps_a_finished_browser_handler_instead_of_reporting_active() {
    require_chrome!();
    let (url, server) = local_page("CCCC browser exit status").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let key = "browser-exit-status";

    manager
        .open(key, &temp.path().join("profile"), &url, 800, 600)
        .await
        .expect("open");
    manager
        .sessions
        .lock()
        .await
        .get_mut(key)
        .expect("session")
        .handler
        .abort();
    tokio::task::yield_now().await;

    let status = manager.info(key).await;

    assert_eq!(status["active"], false);
    assert_eq!(status["state"], "failed");
    assert_eq!(status["error"]["code"], "browser_surface_process_exited");
    assert!(manager.sessions.lock().await.get(key).is_none());
    server.abort();
}

#[tokio::test]
async fn close_releases_key_for_a_different_profile() {
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let first_profile = temp.path().join("profile-1");
    let second_profile = temp.path().join("profile-2");

    manager
        .register_profile("space-provider::notebooklm", &first_profile)
        .await
        .expect("register first profile");
    assert!(
        !manager
            .close("space-provider::notebooklm")
            .await
            .expect("close inactive registration")
    );
    manager
        .register_profile("space-provider::notebooklm", &second_profile)
        .await
        .expect("register replacement profile");

    assert_eq!(
        manager
            .key_profiles
            .lock()
            .await
            .get("space-provider::notebooklm"),
        Some(&second_profile)
    );
}

#[tokio::test]
async fn inactive_stale_profile_is_replaced_for_the_same_key() {
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let first_profile = temp.path().join("profile-1");
    let second_profile = temp.path().join("profile-2");

    manager
        .register_profile("space-provider::notebooklm", &first_profile)
        .await
        .expect("register stale profile");
    manager
        .register_profile("space-provider::notebooklm", &second_profile)
        .await
        .expect("replace inactive stale profile");

    assert_eq!(
        manager
            .key_profiles
            .lock()
            .await
            .get("space-provider::notebooklm"),
        Some(&second_profile)
    );
}

#[tokio::test]
async fn failed_open_releases_profile_registration() {
    require_chrome!();
    let (url, server) = local_page("CCCC failed open cleanup").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let profile = temp.path().join("profile");
    let invalid_storage = json!({"cookies":"not-an-array"});

    manager
        .open_seeded(
            "failed-open",
            &profile,
            &url,
            800,
            600,
            Some(&invalid_storage),
        )
        .await
        .expect_err("invalid cookies should fail initialization");

    assert!(
        !manager
            .key_profiles
            .lock()
            .await
            .contains_key("failed-open")
    );
    server.abort();
}

#[tokio::test]
async fn open_and_close_share_one_profile_lifecycle_boundary() {
    require_chrome!();
    let (url, server) = local_page("CCCC open close race").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let profile = temp.path().join("profile");

    manager
        .open("race", &profile, &url, 800, 600)
        .await
        .expect("initial open");
    let (opened, closed) = tokio::join!(
        manager.open("race", &profile, &url, 800, 600),
        manager.close("race"),
    );

    opened.expect("racing open");
    closed.expect("racing close");
    let _ = manager.close("race").await;
    server.abort();
}

#[tokio::test]
async fn shutdown_closes_all_browser_processes() {
    require_chrome!();
    let (url, server) = local_page("CCCC shutdown browser").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let profile = temp.path().join("profile");

    manager
        .open("shutdown-test", &profile, &url, 800, 600)
        .await
        .expect("open");

    assert_eq!(manager.shutdown_all().await.expect("shutdown"), 1);
    assert!(manager.sessions.lock().await.is_empty());
    assert!(
        manager
            .open("shutdown-test", &profile, &url, 800, 600)
            .await
            .expect_err("open after shutdown must fail")
            .to_string()
            .contains("shutting down")
    );
    server.abort();
}

async fn local_page(body: &'static str) -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .expect("response");
        }
    });
    (format!("http://{address}"), server)
}

async fn page_with_stalled_subresource() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut request = [0_u8; 2048];
                let Ok(read) = stream.read(&mut request).await else {
                    return;
                };
                if String::from_utf8_lossy(&request[..read]).starts_with("GET /never ") {
                    futures_util::future::pending::<()>().await;
                    return;
                }
                let body = "<!doctype html><html><body><h1>DOMContentLoaded</h1><img src='/never'></body></html>";
                let _ = stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    (format!("http://{address}"), server)
}

#[tokio::test]
async fn restores_seeded_cookie_and_detects_real_auth_tokens() {
    require_chrome!();
    let (url, server) = local_page(
        "<!doctype html><script>globalThis.WIZ_global_data={SNlM0e:'csrf',FdrFJe:'session'}</script>",
    )
    .await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let seed = json!({"cookies":[{
        "name":"SID", "value":"present", "url":url, "path":"/",
        "secure":false, "httpOnly":false
    }]});
    manager
        .open_seeded(
            "notebooklm-test",
            &temp.path().join("profile"),
            &url,
            800,
            600,
            Some(&seed),
        )
        .await
        .expect("open seeded browser");
    let page = manager
        .sessions
        .lock()
        .await
        .get("notebooklm-test")
        .expect("browser session")
        .page
        .clone();
    let cookie: String = page
        .evaluate("document.cookie")
        .await
        .expect("evaluate cookie")
        .into_value()
        .expect("cookie string");
    assert!(cookie.contains("SID=present"));
    assert!(
        manager
            .notebooklm_auth_ready("notebooklm-test")
            .await
            .expect("auth probe")
    );
    assert!(manager.close("notebooklm-test").await.expect("close"));
    server.abort();
}

#[tokio::test]
async fn special_key_command_applies_native_input_behavior() {
    require_chrome!();
    let (url, server) = local_page(
        "<!doctype html><input id='email' autofocus value='waterbang@'><script>email.setSelectionRange(email.value.length,email.value.length)</script>",
    )
    .await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    manager
        .open(
            "keyboard-test",
            &temp.path().join("profile"),
            &url,
            800,
            600,
        )
        .await
        .expect("open browser");
    manager
        .sessions
        .lock()
        .await
        .get("keyboard-test")
        .expect("browser session")
        .page
        .evaluate("document.querySelector('#email').focus()")
        .await
        .expect("focus input");
    manager
        .command("keyboard-test", &json!({"t":"key","key":"Backspace"}))
        .await
        .expect("press backspace");
    let page = manager
        .sessions
        .lock()
        .await
        .get("keyboard-test")
        .expect("browser session")
        .page
        .clone();
    let value: String = page
        .evaluate("document.querySelector('#email').value")
        .await
        .expect("read input")
        .into_value()
        .expect("input value");
    assert_eq!(value, "waterbang");
    assert!(manager.close("keyboard-test").await.expect("close"));
    server.abort();
}

#[tokio::test]
async fn click_command_preserves_the_requested_mouse_button() {
    require_chrome!();
    let (url, server) = local_page(
        "<!doctype html><style>html,body{margin:0}#target{width:300px;height:300px}</style><div id='target'>target</div><script>target.addEventListener('contextmenu',event=>{event.preventDefault();document.body.dataset.button=String(event.button)})</script>",
    )
    .await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    manager
        .open(
            "mouse-button-test",
            &temp.path().join("profile"),
            &url,
            800,
            600,
        )
        .await
        .expect("open browser");

    manager
        .command(
            "mouse-button-test",
            &json!({"t":"click","x":100,"y":100,"button":"right"}),
        )
        .await
        .expect("right click");
    let page = manager
        .sessions
        .lock()
        .await
        .get("mouse-button-test")
        .expect("browser session")
        .page
        .clone();
    let button: String = page
        .evaluate("document.body.dataset.button || ''")
        .await
        .expect("read context-menu button")
        .into_value()
        .expect("button value");

    assert_eq!(button, "2");
    assert!(manager.close("mouse-button-test").await.expect("close"));
    server.abort();
}

#[tokio::test]
async fn core_interaction_commands_complete_a_real_page_journey() {
    require_chrome!();
    let (url, server) = local_page(
        "<!doctype html><style>html,body{margin:0}#input{position:absolute;left:20px;top:20px;width:240px;height:50px}</style><input id='input'><script>sessionStorage.loads=String(Number(sessionStorage.loads||0)+1)</script>",
    )
    .await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    manager
        .open(
            "navigate-test",
            &temp.path().join("profile"),
            &url,
            800,
            600,
        )
        .await
        .expect("open browser");
    let page = manager
        .sessions
        .lock()
        .await
        .get("navigate-test")
        .expect("browser session")
        .page
        .clone();
    let start_url = page
        .url()
        .await
        .expect("read start URL")
        .expect("start URL");

    manager
        .command(
            "navigate-test",
            &json!({"t":"click","x":100,"y":45,"button":"left"}),
        )
        .await
        .expect("focus input");
    manager
        .command("navigate-test", &json!({"t":"text","text":"hello"}))
        .await
        .expect("insert text");
    manager
        .command("navigate-test", &json!({"t":"key","key":"Backspace"}))
        .await
        .expect("press key");
    let value: String = page
        .evaluate("document.querySelector('#input').value")
        .await
        .expect("read input")
        .into_value()
        .expect("input value");
    assert_eq!(value, "hell");

    manager
        .command(
            "navigate-test",
            &json!({"t":"resize","width":960,"height":720}),
        )
        .await
        .expect("resize");
    let viewport: Value = page
        .evaluate("({width:window.innerWidth,height:window.innerHeight})")
        .await
        .expect("read viewport")
        .into_value()
        .expect("viewport");
    assert_eq!(viewport, json!({"width":960,"height":720}));

    let target = format!("{url}/next");

    manager
        .command("navigate-test", &json!({"t":"navigate","url":target}))
        .await
        .expect("navigate");
    let observed = page
        .url()
        .await
        .expect("read browser URL")
        .expect("browser URL");

    assert_eq!(observed, target);
    assert_eq!(manager.info("navigate-test").await["url"], target);
    manager
        .command("navigate-test", &json!({"t":"refresh"}))
        .await
        .expect("refresh");
    let loads: String = page
        .evaluate("sessionStorage.loads")
        .await
        .expect("read load count")
        .into_value()
        .expect("load count");
    assert_eq!(loads, "3");

    manager
        .command("navigate-test", &json!({"t":"back"}))
        .await
        .expect("back");
    assert_eq!(
        page.url().await.expect("read back URL"),
        Some(start_url.clone())
    );
    assert_eq!(manager.info("navigate-test").await["url"], start_url);
    assert!(manager.close("navigate-test").await.expect("close"));
    server.abort();
}

#[tokio::test]
async fn scroll_command_targets_the_nested_container_under_the_pointer() {
    require_chrome!();
    let (url, server) = local_page(
        "<!doctype html><style>html,body{margin:0;height:100%;overflow:hidden}#scroller{width:400px;height:300px;overflow:auto}#content{height:2000px}</style><div id='scroller'><div id='content'>scroll target</div></div>",
    )
    .await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    manager
        .open("scroll-test", &temp.path().join("profile"), &url, 800, 600)
        .await
        .expect("open browser");

    manager
        .command(
            "scroll-test",
            &json!({"t":"scroll","x":200,"y":150,"dx":0,"dy":240}),
        )
        .await
        .expect("dispatch wheel");
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let page = manager
        .sessions
        .lock()
        .await
        .get("scroll-test")
        .expect("browser session")
        .page
        .clone();
    let scroll_top: f64 = page
        .evaluate("document.querySelector('#scroller').scrollTop")
        .await
        .expect("read nested scroll position")
        .into_value()
        .expect("numeric scroll position");

    assert!(
        scroll_top >= 200.0,
        "nested container did not scroll: {scroll_top}"
    );
    assert!(manager.close("scroll-test").await.expect("close"));
    server.abort();
}

fn chrome_available() -> bool {
    [
        "/opt/homebrew/bin/chromium",
        "/usr/bin/chromium",
        "/usr/bin/google-chrome",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    ]
    .iter()
    .any(|path| std::path::Path::new(path).is_file())
}

#[test]
fn classifies_only_group_owned_browser_sessions() {
    assert_eq!(session_group_id("g_one::presentation"), Some("g_one"));
    assert_eq!(session_group_id("web-model::g_two::actor"), Some("g_two"));
    assert_eq!(session_group_id("space-provider::notebooklm"), None);
    assert_eq!(
        session_actor("web-model::g_two::actor"),
        Some(("g_two", "actor"))
    );
    assert_eq!(session_actor("g_one::presentation"), None);
}

// Our route-level browser regressions share the upstream Chrome admission lock.
pub(super) async fn t05_chrome_test_guard() -> tokio::sync::OwnedMutexGuard<()> {
    chrome_test_guard().await
}

#[tokio::test]
async fn shared_web_model_operations_preserve_busy_drafts_and_serialize_manual_navigation() {
    require_chrome!();
    let (url,server)=local_page(r#"<!doctype html><html><body><form><textarea id="prompt-textarea" placeholder="Message">my unsent draft</textarea><button type="button" aria-label="Stop streaming">Stop</button></form></body></html>"#).await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = std::sync::Arc::new(BrowserSurfaces::default());
    let profile = temp.path().join("profile");
    let (first, second) = tokio::join!(
        manager.ensure_open(SHARED_WEB_MODEL_KEY, &profile, &url, 800, 600),
        manager.ensure_open(SHARED_WEB_MODEL_KEY, &profile, &url, 800, 600),
    );
    assert_eq!(
        first.expect("first open")["started_at"],
        second.expect("reuse")["started_at"]
    );
    let page = manager.page(SHARED_WEB_MODEL_KEY).await.expect("test page");
    let result = manager
        .submit_prompt_with_attachment(
            SHARED_WEB_MODEL_KEY,
            &url,
            "member report",
            None,
            "test-busy",
        )
        .await
        .expect("submission result");
    assert!(matches!(
        result,
        prompt_submission::PromptSubmissionOutcome::Deferred(_)
    ));
    let draft: String = page
        .evaluate("document.querySelector('textarea').value")
        .await
        .expect("read composer")
        .into_value()
        .expect("composer string");
    assert_eq!(
        draft, "my unsent draft",
        "busy submission changed the user's draft"
    );
    let guard = manager.web_model_operation.lock().await;
    let other = std::sync::Arc::clone(&manager);
    let navigation = tokio::spawn(async move {
        other
            .command(SHARED_WEB_MODEL_KEY, &json!({"t":"text","text":"later"}))
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !navigation.is_finished(),
        "manual interaction bypassed an in-flight browser transaction"
    );
    drop(guard);
    navigation.await.expect("join").expect("manual interaction");
    assert_eq!(manager.sessions.lock().await.len(), 1);
    manager
        .close(SHARED_WEB_MODEL_KEY)
        .await
        .expect("close shared browser");
    server.abort();
}

#[tokio::test]
async fn cross_chat_delivery_does_not_navigate_away_from_a_draft() {
    require_chrome!();
    let (source,source_server)=local_page(r#"<!doctype html><textarea id="prompt-textarea" style="width:500px;height:100px">UNSENT_A_DRAFT</textarea>"#).await;
    let (destination,destination_server)=local_page(r#"<!doctype html><textarea id="prompt-textarea" style="width:500px;height:100px"></textarea><button aria-label="Send prompt" onclick="const t=document.querySelector('textarea');const p=document.createElement('div');p.dataset.messageAuthorRole='user';p.textContent=t.value;document.body.append(p);t.value='';window.sent=(window.sent||0)+1">Send</button>"#).await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = std::sync::Arc::new(BrowserSurfaces::default());
    manager
        .open(
            "draft-routing",
            &temp.path().join("profile"),
            &source,
            800,
            600,
        )
        .await
        .expect("browser");
    let work = std::sync::Arc::clone(&manager);
    let outcome=tokio::spawn(async move {
        let result=work.submit_prompt_with_attachment("draft-routing",&destination,"REPORT_B",None,"cross-draft").await.expect("submission check");
        assert!(matches!(result,prompt_submission::PromptSubmissionOutcome::Deferred(_)),"B delivery navigated away from A's unsent draft");
        let page=work.page("draft-routing").await.expect("test page");
        assert_eq!(page.url().await.expect("url").expect("URL"),format!("{source}/"));
        let draft:String=page.evaluate("document.querySelector('textarea').value").await.expect("draft").into_value().expect("text");
        assert_eq!(draft,"UNSENT_A_DRAFT");
        page.evaluate("document.querySelector('textarea').value=''").await.expect("user clears fixture draft");
        let resumed=work.submit_prompt_with_attachment("draft-routing",&destination,"REPORT_B",None,"cross-draft").await.expect("resumed submission");
        assert!(matches!(resumed,prompt_submission::PromptSubmissionOutcome::Verified(_)),"delivery did not recover when the draft was cleared");
        let count:u64=page.evaluate("window.sent||0").await.expect("send count").into_value().expect("count");
        assert_eq!(count,1);
        let again=work.submit_prompt_with_attachment("draft-routing",&destination,"REPORT_B",None,"cross-draft").await.expect("repeat observation");
        assert!(matches!(again,prompt_submission::PromptSubmissionOutcome::Verified(_)));
        let count:u64=page.evaluate("window.sent||0").await.expect("send count").into_value().expect("count");
        assert_eq!(count,1,"echo reconciliation submitted the same report again");
        eprintln!("REAL_CHROME: A draft blocks B navigation; clear -> one B submission -> duplicate observation does not resend");
    }).await;
    let _ = manager.close("draft-routing").await;
    source_server.abort();
    destination_server.abort();
    outcome.expect("cross-chat assertions");
}

#[tokio::test]
async fn submission_does_not_wait_for_background_intersection_observers() {
    require_chrome!();
    let (url,server)=local_page(r#"<!doctype html><html><body><form onsubmit="event.preventDefault();window.sent=(window.sent||0)+1;sessionStorage.setItem('send-count',String(Number(sessionStorage.getItem('send-count')||0)+1));sessionStorage.setItem('last-prompt',document.querySelector('textarea').value);const turn=document.createElement('section');turn.dataset.testid='conversation-turn-1';turn.dataset.turnId='request-client-0';const e=document.createElement('div');e.dataset.messageAuthorRole='user';e.textContent=document.querySelector('textarea').value;turn.append(e);document.body.append(turn);document.querySelector('textarea').value='';setTimeout(()=>{const answer=document.createElement('div');answer.dataset.messageAuthorRole='assistant';answer.dataset.messageId='server-answer';answer.textContent='Received';turn.append(answer);window.accepted=true},600)"><textarea id="prompt-textarea" style="width:500px;height:100px"></textarea><button id="composer-submit-button" type="submit" aria-label="Send prompt">Send</button></form><script>if(sessionStorage.getItem('cccc-refresh-receipt')){const turn=document.createElement('section');turn.dataset.testid='conversation-turn-stable';turn.dataset.turnId='server-stable-turn';const user=document.createElement('div');user.dataset.messageAuthorRole='user';user.textContent='';const answer=document.createElement('div');answer.dataset.messageAuthorRole='assistant';answer.dataset.messageId='server-stable-answer';answer.textContent='Received';turn.append(user,answer);document.body.append(turn)}</script></body></html>"#).await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let key = "background-submit";
    let prompt = "Browser batch background events=background-once";
    manager
        .open(key, &temp.path().join("profile"), &url, 800, 600)
        .await
        .expect("browser");
    let result = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
        let page = manager.page(key).await.expect("page");
        // Inactive tabs may never deliver visual observer callbacks.
        page.evaluate("window.IntersectionObserver=class{observe(){} unobserve(){} disconnect(){}};")
            .await.expect("suspend visual observer");
        let submit = || manager.submit_prompt_with_attachment(key, &url, prompt, None, "background-once");
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(6), submit())
            .await.expect("background send timeout").expect("send");
        let PromptSubmissionOutcome::Verified(evidence) = outcome else {
            panic!("background send must wait for server evidence, not visual observers");
        };
        assert!(page.evaluate("Boolean(window.accepted)").await.expect("server acknowledgement")
            .into_value::<bool>().expect("boolean"), "optimistic echo is not acceptance");
        let started = std::time::Instant::now();
        assert!(manager.relay_receipt_stable_before_close(key, &evidence).await.expect("close check"));
        assert!(started.elapsed() >= std::time::Duration::from_millis(800), "receipt was not observed long enough");
        assert!(matches!(submit().await.expect("repeat"), PromptSubmissionOutcome::Verified(_)));
        page.evaluate("document.querySelector('[data-turn-id]').dataset.turnId='request-client-pending'; const old=document.querySelector('[data-message-author-role=assistant]'); document.body.prepend(old)")
            .await.expect("provisional echo with unrelated old answer");
        for thinking_placeholder in [false, true] {
            if thinking_placeholder {
                page.evaluate("const placeholder=document.createElement('div'); placeholder.dataset.messageAuthorRole='assistant'; placeholder.dataset.messageId='request-placeholder-request-client-pending'; placeholder.textContent='正在思考'; document.querySelector('[data-turn-id]').append(placeholder)")
                    .await.expect("nonempty thinking placeholder");
            }
            assert!(!manager.relay_receipt_stable_before_close(key, &evidence).await.expect("provisional close check"));
            assert!(matches!(submit().await.expect("provisional duplicate"), PromptSubmissionOutcome::Ambiguous(_)),
                "provisional echo must neither be accepted nor sent again");
            assert_eq!(page.evaluate("window.sent||0").await.expect("send counter")
                .into_value::<u64>().expect("count"), 1, "duplicate submission");
        }
        for response_started in [false, true] {
            let stored = json!({
                "baseline":{"user_message_count":0},
                "observed":{"user_message_count":1,"echo_found":true,
                    "latest_turn_id":"request-client-pending","response_started":response_started}
            });
            assert_eq!(prompt_submission::stored_verified_submission_evidence(&stored).is_some(),
                response_started, "only a server response can confirm a provisional container");
        }
        page.evaluate("sessionStorage.setItem('cccc-refresh-receipt','1')")
            .await.expect("arm one-time server receipt after reload");
        let stale = json!({
            "submitted":false,
            "input_selector":"textarea",
            "send_selector":"form.requestSubmit",
            "submission_evidence":"optimistic_echo_unconfirmed",
            "baseline":{"url":url,"user_message_count":0},
            "observed":{"url":url,"user_message_count":1,"echo_found":true,
                "latest_turn_id":"request-client-stale","response_started":false}
        });
        let refreshed = manager
            .refresh_optimistic_submission(key, &url, prompt, &stale)
            .await;
        assert_eq!(refreshed["submitted"], true, "{refreshed}");
        assert_eq!(refreshed["reconciled_by"], "single_page_refresh");
        assert_eq!(
            refreshed["submission_evidence"],
            "user_message_count_increased"
        );
        let refreshed_started = std::time::Instant::now();
        assert!(manager
            .relay_receipt_stable_before_close(key, &refreshed)
            .await
            .expect("count-based close check"));
        assert!(
            refreshed_started.elapsed() >= std::time::Duration::from_millis(800),
            "server-backed message count was not observed long enough"
        );
        assert_eq!(page.evaluate("Number(sessionStorage.getItem('send-count'))")
            .await.expect("send count after refresh").into_value::<u64>().expect("count"), 1,
            "reconciliation must reload only; it must never submit again");
    })).await;
    manager.close(key).await.expect("close browser");
    server.abort();
    result.expect("background submission and close assertions");
}

#[tokio::test]
async fn redirect_before_send_never_dispatches_to_another_conversation() {
    require_chrome!();
    let (url, server)=local_page(r#"<!doctype html><body><textarea id="prompt-textarea" placeholder="Message"></textarea><button data-testid="send-button">Send</button><script>globalThis.sends=0;document.querySelector('button').onclick=()=>{sends++;const n=document.createElement('div');n.dataset.messageAuthorRole='user';n.textContent=document.querySelector('textarea').value;document.body.append(n);document.querySelector('textarea').value='';history.replaceState({},'','/wrong-after-send')}</script></body>"#).await;
    let url = format!("{url}/");
    let temp = tempfile::tempdir().expect("temp");
    let manager = BrowserSurfaces::default();
    manager
        .ensure_open("redirect", &temp.path().join("chrome"), &url, 800, 600)
        .await
        .expect("chrome");
    let page = manager.page("redirect").await.expect("test page");
    let result = manager
        .submit_prompt_with_attachment_before_dispatch(
            "redirect",
            &url,
            "ORIGINAL_ROUTE_REPORT",
            None,
            "route-test",
            || async {
                page.evaluate("history.replaceState({},'','/wrong-before-send')")
                    .await?;
                Ok(())
            },
        )
        .await
        .expect("redirect check");
    assert!(matches!(result, PromptSubmissionOutcome::Deferred(_)));
    assert_eq!(
        page.evaluate("globalThis.sends")
            .await
            .expect("counter")
            .into_value::<u64>()
            .expect("count"),
        0
    );
    page.evaluate("history.replaceState({},'','/');document.querySelector('textarea').value=''")
        .await
        .expect("restore fixture route");
    let result = manager
        .submit_prompt_with_attachment(
            "redirect",
            &url,
            "ORIGINAL_ROUTE_REPORT",
            None,
            "route-test",
        )
        .await
        .expect("post-click change");
    assert!(
        matches!(result, PromptSubmissionOutcome::Ambiguous(_)),
        "a different conversation was treated as the target receipt"
    );
    assert_eq!(
        page.evaluate("globalThis.sends")
            .await
            .expect("counter")
            .into_value::<u64>()
            .expect("count"),
        1
    );
    manager.close("redirect").await.expect("close");
    server.abort();
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn macos_system_browser_restarts_preserve_storage_and_submission() {
    require_chrome!();
    let (url, server) = local_page(
        r#"<!doctype html><body>
<textarea id="prompt-textarea"></textarea><button data-testid="send-button">Send</button>
<script>document.querySelector('button').onclick=()=>{const i=document.querySelector('textarea');
const m=document.createElement('div');m.dataset.messageAuthorRole='user';m.textContent=i.value;
document.body.append(m);i.value=''}</script></body>"#,
    )
    .await;
    let temp = tempfile::tempdir().expect("isolated profile");
    let profile = temp.path().join("profile");
    for cycle in 0..3 {
        let manager = BrowserSurfaces::default();
        let opened = manager
            .open_seeded_system("exit-restart", &profile, &url, 800, 600, None)
            .await
            .expect("system Chrome opens");
        let pid = opened["metadata"]["pid"]
            .as_u64()
            .expect("actual browser pid");
        {
            let mut sessions = manager.sessions.lock().await;
            let session = sessions.get_mut("exit-restart").expect("connected session");
            assert!(session.browser.get_mut_child().is_none());
            assert!(
                session
                    .browser
                    .wait()
                    .await
                    .expect("connected wait")
                    .is_none()
            );
        }
        let page = manager.page("exit-restart").await.expect("test page");
        let work = async {
            if cycle == 0 {
                page.evaluate("document.cookie='cccc_exit_cookie=retained; path=/; max-age=600';localStorage.setItem('cccc-exit','retained')")
                    .await.expect("save test storage");
            } else {
                let retained: bool = page.evaluate("document.cookie.includes('cccc_exit_cookie=retained') && localStorage.getItem('cccc-exit')==='retained'")
                    .await.expect("read storage").into_value().expect("storage boolean");
                assert!(retained, "restart lost persistent browser storage");
            }
            let prompt = format!("EXIT_RESTART_REPORT_{cycle}");
            for _ in 0..2 {
                assert!(matches!(
                    manager
                        .submit_prompt_with_attachment("exit-restart", &url, &prompt, None, &prompt)
                        .await
                        .expect("submit"),
                    PromptSubmissionOutcome::Verified(_)
                ));
            }
            let count: u64 = page
                .evaluate("document.querySelectorAll('[data-message-author-role=user]').length")
                .await
                .expect("message count")
                .into_value()
                .expect("count");
            let message_texts: Vec<String> = page.evaluate("Array.from(document.querySelectorAll('[data-message-author-role=user]'), e => e.textContent)")
                .await.expect("message diagnostics").into_value().expect("message texts");
            assert_eq!(
                count, 1,
                "repeated submission duplicated the report: {message_texts:?}"
            );
        };
        let outcome =
            futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(work)).await;
        crate::shutdown::browser_surfaces(&manager).await;
        let process = tokio::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "stat="])
            .output()
            .await
            .expect("inspect actual pid");
        let remaining = String::from_utf8_lossy(&process.stdout);
        let exited = !process.status.success()
            || remaining.trim().is_empty()
            || remaining.trim().starts_with('Z');
        if outcome.is_err() || !exited {
            server.abort();
        }
        outcome.expect("storage and submission assertions");
        assert!(exited, "system browser {pid} survived completed shutdown");
        assert!(
            !manager.info("exit-restart").await["active"]
                .as_bool()
                .unwrap_or(false)
        );
    }
    server.abort();
}

#[tokio::test]
async fn archived_page_does_not_block_other_targets_or_trigger_reloads() {
    require_chrome!();
    let (archived_url, archived_server) = local_page(r#"<!doctype html><body><nav><button>查看工作目录停止</button></nav><main><p>This conversation is archived.</p><button onclick="window.unarchived=true">Unarchive</button></main></body>"#).await;
    let (ready_url, ready_server) = local_page(r#"<!doctype html><body><form><textarea id="prompt-textarea"></textarea><button type="button" aria-label="Send prompt" onclick="window.sent=(window.sent||0)+1;const n=document.createElement('div');n.dataset.messageAuthorRole='user';n.textContent=document.querySelector('textarea').value;document.body.append(n);document.querySelector('textarea').value=''">Send</button></form></body>"#).await;
    let temp = tempfile::tempdir().expect("profile");
    let manager = BrowserSurfaces::default();
    manager
        .open(
            "archived",
            &temp.path().join("profile"),
            &archived_url,
            800,
            600,
        )
        .await
        .expect("browser");
    let result = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
        let page = manager.page("archived").await.expect("page");
        let blocked = manager
            .relay_target_deferral("archived", &archived_url)
            .await
            .expect("same target")
            .expect("archive blocker");
        assert_eq!(
            blocked["submission_evidence"],
            "not_sent_conversation_archived"
        );
        assert!(
            manager
                .relay_target_deferral("archived", &ready_url)
                .await
                .expect("other target")
                .is_none()
        );
        let own = manager
            .submit_prompt_with_attachment("archived", &archived_url, "ARCHIVED_REPORT", None, "a")
            .await
            .expect("archived submission");
        assert!(matches!(own, PromptSubmissionOutcome::Deferred(_)));
        assert!(
            !page
                .evaluate("Boolean(window.unarchived)")
                .await
                .expect("untouched archive")
                .into_value::<bool>()
                .expect("bool")
        );
        let other = manager
            .submit_prompt_with_attachment("archived", &ready_url, "OTHER_GROUP_REPORT", None, "b")
            .await
            .expect("other group submission");
        assert!(
            matches!(other, PromptSubmissionOutcome::Verified(_)),
            "archived A blocked B"
        );
        assert_eq!(
            page.evaluate("window.sent")
                .await
                .expect("count")
                .into_value::<u64>()
                .expect("number"),
            1
        );
    }))
    .await;
    manager.close("archived").await.expect("close");
    archived_server.abort();
    ready_server.abort();
    result.expect("archive isolation assertions")
}

// The same browser controls have one gate: real page state, not quoted message text.
#[tokio::test]
async fn page_state_gates_preserve_blockers_and_ignore_quoted_controls() {
    require_chrome!();
    let (url, server) = local_page("<!doctype html><body><main></main></body>").await;
    let temp = tempfile::tempdir().expect("profile");
    let manager = BrowserSurfaces::default();
    let key = "page-state";
    manager
        .open(key, &temp.path().join("profile"), &url, 800, 600)
        .await
        .expect("browser");
    let result = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
        let page = manager.page(key).await.expect("page");
        let cases = [
            (r#"<main><form><textarea id="prompt-textarea"></textarea><button aria-label="Stop streaming">Stop</button></form></main>"#, "not_sent_chat_busy"),
            (r#"<main><form><textarea id="prompt-textarea">HUMAN DRAFT</textarea></form></main>"#, "not_sent_composer_occupied"),
            (r#"<button data-mobile-auth-entry-action="login">登录</button><main><form><textarea id="prompt-textarea"></textarea><button aria-label="Send prompt" onclick="window.sent++">Send</button></form></main>"#, "not_sent_login_required"),
            ("<main><div role=alert>Access denied</div></main>", "not_sent_access_denied"),
            ("<main><div role=alert>Verify you are human</div></main>", "not_sent_verification_required"),
            ("<main><div role=alert>操作过于频繁</div></main>", "not_sent_rate_limited"),
        ];
        for (html, reason) in cases {
            page.evaluate(format!("document.body.innerHTML={};window.sent=0", serde_json::to_string(html).expect("HTML")))
                .await.expect("fixture state");
            if reason == "not_sent_login_required" {
                let readiness = manager.prompt_readiness(key).await.expect("guest readiness");
                assert_eq!(readiness["ready"], false);
                assert_eq!(readiness["login_required"], true);
            }
            let before = page.evaluate("JSON.stringify({text:document.body.innerText,draft:document.querySelector('textarea')?.value||''})").await.expect("original body")
                .into_value::<String>().expect("body");
            let target = if matches!(reason, "not_sent_access_denied" | "not_sent_verification_required" | "not_sent_rate_limited") {
                "http://127.0.0.1:1/other"
            } else {
                &url
            };
            let outcome = manager.submit_prompt_with_attachment(key, target, "DO_NOT_SEND", None, "blocked")
                .await.unwrap_or_else(|error| panic!("page state {reason}: {error}"));
            let PromptSubmissionOutcome::Deferred(evidence) = outcome else { panic!("not deferred: {reason}"); };
            assert_eq!(evidence["submission_evidence"], reason);
            assert_eq!(page.url().await.expect("URL").expect("present URL").trim_end_matches('/'), url.trim_end_matches('/'));
            assert_eq!(page.evaluate("JSON.stringify({text:document.body.innerText,draft:document.querySelector('textarea')?.value||''})").await.expect("unchanged body").into_value::<String>().expect("body"), before,
                "blocked delivery changed the human draft or page: {reason}");
            assert_eq!(page.evaluate("window.sent").await.expect("send count").into_value::<u64>().expect("count"), 0);
            assert!(!manager.relay_surface_idle(key).await.expect("not idle"));
        }
        page.evaluate("document.body.innerHTML='<main>Loading</main>'").await.expect("missing composer");
        assert!(!manager.relay_surface_idle(key).await.expect("not ready"));
        assert_eq!(manager.relay_surface_deferral(key).await.expect("probe").expect("blocked")["submission_evidence"],
            "not_sent_composer_unavailable");
        // A real active Stop control must block; sidebar/history/quoted controls must not.
        page.evaluate(r#"document.body.innerHTML=`<nav><button aria-label="置顶 查看工作目录停止 状态">Pin</button>
<button data-testid="history-item-0-options" aria-label="打开“查看工作目录停止”的对话选项">Options</button>
<button aria-label="Stop" title="Pinned chat">Stop</button></nav><main><article><button>Stop</button>
<div data-message-author-role="assistant"><button data-testid="login-button">Log in</button><div role="alert">Too many requests</div></div></article>
<form><textarea id="prompt-textarea"></textarea><button type="button" aria-label="Send prompt">Send</button>
<button id="busy" data-testid="stop-button" aria-label="Stop streaming">Stop</button></form></main>`;
window.sent=0;document.querySelector('form button').onclick=()=>{window.sent++;const t=document.querySelector('textarea');const n=document.createElement('div');n.dataset.messageAuthorRole='user';n.textContent=t.value;document.body.append(n);t.value=''}"#)
            .await.expect("quoted controls fixture");
        assert_eq!(manager.relay_surface_deferral(key).await.expect("busy probe").expect("real Stop")["submission_evidence"], "not_sent_chat_busy");
        page.evaluate("document.querySelector('#busy').remove()").await.expect("answer ends");
        assert_eq!(manager.prompt_readiness(key).await.expect("signed in readiness")["ready"], true);
        assert!(manager.relay_surface_idle(key).await.expect("idle after recovery"));
        assert!(manager.relay_surface_deferral(key).await.expect("quoted controls ignored").is_none());
        for _ in 0..2 {
            assert!(matches!(manager.submit_prompt_with_attachment(key, &url, "RECOVERED_REPORT", None, "recovered").await.expect("send"),
                PromptSubmissionOutcome::Verified(_)));
        }
        assert_eq!(page.evaluate("window.sent").await.expect("counter").into_value::<u64>().expect("count"), 1, "repeat sent twice");
    })).await;
    manager.close(key).await.expect("close");
    server.abort();
    result.expect("page-state gate assertions");
}
