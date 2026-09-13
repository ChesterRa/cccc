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
    let page = manager
        .sessions
        .lock()
        .await
        .get(SHARED_WEB_MODEL_KEY)
        .expect("shared session")
        .page
        .clone();
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
    eprintln!("shared browser launched, busy draft preserved, manual command serialized");
}

#[tokio::test]
async fn guest_composer_is_not_authenticated_delivery() {
    require_chrome!();
    let (url,server)=local_page(r#"<!doctype html><body><button data-mobile-auth-entry-action="login">登录</button><main><form><textarea id="prompt-textarea" placeholder="Message" style="width:500px;height:100px"></textarea><button type="button" aria-label="Send prompt" onclick="window.sent=(window.sent||0)+1">Send</button></form></main></body>"#).await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = std::sync::Arc::new(BrowserSurfaces::default());
    manager
        .open("guest-check", &temp.path().join("profile"), &url, 800, 600)
        .await
        .expect("browser");
    let work = std::sync::Arc::clone(&manager);
    let outcome=tokio::spawn(async move {
        let readiness=work.prompt_readiness("guest-check").await.expect("readiness");
        assert_eq!(readiness["ready"],false,"guest composer was mistaken for a signed-in browser");
        assert_eq!(readiness["login_required"],true);
        let result=work.submit_prompt_with_attachment("guest-check",&url,"must not enter guest chat",None,"guest-report").await.expect("preflight");
        assert!(matches!(result,prompt_submission::PromptSubmissionOutcome::Deferred(_)),"guest report was submitted");
        let page=work.sessions.lock().await.get("guest-check").expect("session").page.clone();
        let untouched:bool=page.evaluate("!window.sent && document.querySelector('textarea').value === ''").await.expect("read").into_value().expect("bool");
        assert!(untouched,"guest preflight touched the composer or Send");
        page.evaluate("document.querySelector('[data-mobile-auth-entry-action]').remove();document.querySelector('main').insertAdjacentHTML('beforeend','<div data-message-author-role=assistant><button data-testid=login-button>Log in</button></div>')").await.expect("fixture login recovery");
        assert_eq!(work.prompt_readiness("guest-check").await.expect("recovered readiness")["ready"],true,"quoted login text inside a message blocked signed-in use");
        eprintln!("REAL_CHROME: guest login controls block readiness and send; message content is not authentication state");
    }).await;
    let _ = manager.close("guest-check").await;
    server.abort();
    outcome.expect("guest assertions");
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
        let page=work.sessions.lock().await.get("draft-routing").expect("session").page.clone();
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
    let (url,server)=local_page(r#"<!doctype html><html><body><form onsubmit="event.preventDefault();window.sent=(window.sent||0)+1;const turn=document.createElement('section');turn.dataset.testid='conversation-turn-1';turn.dataset.turnId='request-client-0';const e=document.createElement('div');e.dataset.messageAuthorRole='user';e.textContent=document.querySelector('textarea').value;turn.append(e);document.body.append(turn);document.querySelector('textarea').value='';setTimeout(()=>{const answer=document.createElement('div');answer.dataset.messageAuthorRole='assistant';answer.dataset.messageId='server-answer';answer.textContent='Received';turn.append(answer);window.accepted=true},600)"><textarea id="prompt-textarea" style="width:500px;height:100px"></textarea><button id="composer-submit-button" type="submit" aria-label="Send prompt">Send</button></form></body></html>"#).await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    manager
        .open(
            "background-submit",
            &temp.path().join("profile"),
            &url,
            800,
            600,
        )
        .await
        .expect("browser");
    let page = manager
        .sessions
        .lock()
        .await
        .get("background-submit")
        .expect("session")
        .page
        .clone();
    // An inactive tab need not deliver IntersectionObserver callbacks. Model
    // that condition deterministically; never bring a user's window to front.
    page.evaluate("window.IntersectionObserver=class{observe(){} unobserve(){} disconnect(){}};")
        .await
        .expect("suspend observer fixture");
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(6),
        manager.submit_prompt_with_attachment(
            "background-submit",
            &url,
            "Browser batch background events=background-once",
            None,
            "background-once",
        ),
    )
    .await;
    let receipt_evidence = match &outcome {
        Ok(Ok(prompt_submission::PromptSubmissionOutcome::Verified(evidence))) => {
            Some(evidence.clone())
        }
        _ => None,
    };
    let close_check_started = std::time::Instant::now();
    let close_ready = match receipt_evidence.as_ref() {
        Some(evidence) => manager
            .relay_receipt_stable_before_close("background-submit", evidence)
            .await
            .unwrap_or(false),
        None => false,
    };
    let close_check_ms = close_check_started.elapsed().as_millis();
    let confirmed = page
        .evaluate("Boolean(window.accepted)")
        .await
        .expect("server acknowledgement fixture")
        .into_value::<bool>()
        .expect("boolean");
    let count: u64 = page
        .evaluate("window.sent||0")
        .await
        .expect("send count")
        .into_value()
        .expect("count");
    let repeated = if outcome.as_ref().is_ok_and(|result| {
        matches!(
            result,
            Ok(prompt_submission::PromptSubmissionOutcome::Verified(_))
        )
    }) {
        Some(
            manager
                .submit_prompt_with_attachment(
                    "background-submit",
                    &url,
                    "Browser batch background events=background-once",
                    None,
                    "background-once",
                )
                .await,
        )
    } else {
        None
    };
    page.evaluate(
        "document.querySelector('[data-turn-id]').dataset.turnId='request-client-pending'; const old=document.querySelector('[data-message-author-role=assistant]'); document.body.prepend(old)",
    )
    .await
    .expect("provisional duplicate fixture");
    let provisional_close_ready = match receipt_evidence.as_ref() {
        Some(evidence) => manager
            .relay_receipt_stable_before_close("background-submit", evidence)
            .await
            .unwrap_or(false),
        None => false,
    };
    let provisional = manager
        .submit_prompt_with_attachment(
            "background-submit",
            &url,
            "Browser batch background events=background-once",
            None,
            "background-once",
        )
        .await;
    let final_count: u64 = page
        .evaluate("window.sent||0")
        .await
        .expect("final count")
        .into_value()
        .expect("count");
    let _ = manager.close("background-submit").await;
    server.abort();
    assert!(
        matches!(
            outcome,
            Ok(Ok(prompt_submission::PromptSubmissionOutcome::Verified(_)))
        ),
        "background submission waited for a visual observer instead of invoking the checked Send control"
    );
    assert!(
        confirmed,
        "optimistic client echo was mistaken for a completed submission"
    );
    assert!(close_ready, "stable server receipt was not safe to close");
    assert!(
        close_check_ms >= 800,
        "browser close guard observed the receipt for only {close_check_ms}ms"
    );
    assert!(
        !provisional_close_ready,
        "a provisional echo was treated as stable enough to close"
    );
    assert!(
        matches!(
            provisional,
            Ok(prompt_submission::PromptSubmissionOutcome::Ambiguous(_))
        ),
        "a provisional echo must neither be accepted nor sent again"
    );
    assert!(prompt_submission::stored_verified_submission_evidence(&json!({
        "baseline":{"user_message_count":0},
        "observed":{"user_message_count":1,"echo_found":true,"latest_turn_id":"request-client-pending"}
    })).is_none(), "persisted optimistic evidence was promoted to accepted");
    assert!(prompt_submission::stored_verified_submission_evidence(&json!({
        "baseline":{"user_message_count":0},
        "observed":{"user_message_count":1,"echo_found":true,"latest_turn_id":"request-client-pending","response_started":true}
    })).is_some(), "a server response was ignored because its container kept a provisional id");
    assert_eq!(count, 1);
    assert!(matches!(
        repeated,
        Some(Ok(prompt_submission::PromptSubmissionOutcome::Verified(_)))
    ));
    assert_eq!(
        final_count, 1,
        "an existing message echo was sent a second time"
    );
    eprintln!(
        "BACKGROUND_SUBMISSION: suspended visual observer, one verified send, duplicate observation does not resend"
    );
}

#[tokio::test]
async fn relay_idle_probe_requires_an_empty_non_generating_composer() {
    require_chrome!();
    let (url, server) = local_page(
        r#"<!doctype html><body><form><textarea id="prompt-textarea" placeholder="Message"></textarea><button type="button" aria-label="Send prompt">Send</button><button id="busy" type="button" aria-label="Stop streaming">Stop</button></form></body>"#,
    )
    .await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    manager
        .open("relay-idle", &temp.path().join("profile"), &url, 800, 600)
        .await
        .expect("browser");
    assert!(
        !manager
            .relay_surface_idle("relay-idle")
            .await
            .expect("busy probe")
    );
    assert_eq!(
        manager
            .relay_surface_deferral("relay-idle")
            .await
            .expect("busy evidence")
            .expect("busy deferral")["submission_evidence"],
        "not_sent_chat_busy"
    );
    let page = manager
        .sessions
        .lock()
        .await
        .get("relay-idle")
        .expect("session")
        .page
        .clone();
    page.evaluate("document.querySelector('#busy').remove();document.querySelector('textarea').value='HUMAN DRAFT'")
        .await
        .expect("draft fixture");
    assert!(
        !manager
            .relay_surface_idle("relay-idle")
            .await
            .expect("draft probe")
    );
    assert_eq!(
        manager
            .relay_surface_deferral("relay-idle")
            .await
            .expect("draft evidence")
            .expect("draft deferral")["submission_evidence"],
        "not_sent_composer_occupied"
    );
    page.evaluate("document.querySelector('textarea').value=''")
        .await
        .expect("clear fixture");
    assert!(
        manager
            .relay_surface_idle("relay-idle")
            .await
            .expect("idle probe")
    );
    assert!(
        manager
            .relay_surface_deferral("relay-idle")
            .await
            .expect("idle evidence")
            .is_none()
    );
    page.evaluate("document.querySelector('textarea').remove()")
        .await
        .expect("missing input");
    assert!(
        !manager
            .relay_surface_idle("relay-idle")
            .await
            .expect("unready is not idle")
    );
    assert_eq!(
        manager
            .relay_surface_deferral("relay-idle")
            .await
            .expect("unready evidence")
            .expect("deferred")["submission_evidence"],
        "not_sent_composer_unavailable"
    );
    manager.close("relay-idle").await.expect("close browser");
    server.abort();
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
    let page = manager
        .sessions
        .lock()
        .await
        .get("redirect")
        .expect("session")
        .page
        .clone();
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
            eprintln!(
                "CONNECTED_WAIT cycle={cycle} pid={pid} exit_status=None; live page verified next"
            );
        }
        let page = manager
            .sessions
            .lock()
            .await
            .get("exit-restart")
            .expect("session")
            .page
            .clone();
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
            eprintln!("EXIT_RESTART_MESSAGES cycle={cycle} messages={message_texts:?}");
            assert_eq!(
                count, 1,
                "repeated submission duplicated the report: {message_texts:?}"
            );
        };
        let outcome =
            futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(work)).await;
        let started = std::time::Instant::now();
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
        eprintln!(
            "MACOS_EXIT_CYCLE={cycle} pid={pid} shutdown_ms={} exited={exited} storage=retained report_count=1",
            started.elapsed().as_millis()
        );
    }
    server.abort();
}

#[tokio::test]
async fn sidebar_titles_and_history_controls_are_not_generation_stop_controls() {
    require_chrome!();
    let (url, server) = local_page(r#"<!doctype html><body>
<nav><button aria-label="置顶 查看工作目录停止 状态">Pin</button>
<button data-testid="history-item-0-options" aria-label="打开“查看工作目录停止”的对话选项">Options</button>
<button aria-label="Stop" title="Pinned chat">Stop</button></nav>
<main><article><button>Stop</button></article>
<form><textarea id="prompt-textarea" placeholder="Message"></textarea>
<button type="button" aria-label="Send prompt">Send</button>
<button id="busy" data-testid="stop-button" type="button" aria-label="Stop streaming">Stop</button>
</form></main></body>"#).await;
    let temp = tempfile::tempdir().expect("isolated profile");
    let manager = BrowserSurfaces::default();
    manager
        .open("sidebar-stop", &temp.path().join("profile"), &url, 800, 600)
        .await
        .expect("browser");
    let busy = manager
        .relay_surface_deferral("sidebar-stop")
        .await
        .expect("real stop");
    let page = manager
        .sessions
        .lock()
        .await
        .get("sidebar-stop")
        .expect("session")
        .page
        .clone();
    page.evaluate("document.querySelector('#busy').remove()")
        .await
        .expect("finish answer");
    let idle = manager
        .relay_surface_idle("sidebar-stop")
        .await
        .expect("idle inspection");
    let deferred = manager
        .relay_surface_deferral("sidebar-stop")
        .await
        .expect("deferral inspection");
    page.evaluate(r#"window.sent=0;document.querySelector('form button').onclick=()=>{window.sent++;const n=document.createElement('div');n.dataset.messageAuthorRole='user';n.textContent=document.querySelector('textarea').value;document.body.append(n);document.querySelector('textarea').value=''}"#)
        .await.expect("fixture send handler");
    let sent = manager
        .submit_prompt_with_attachment(
            "sidebar-stop",
            &url,
            "SIDEBAR_STOP_ORIGINAL_REPORT",
            None,
            "sidebar-report",
        )
        .await
        .expect("submit through native gates");
    let again = manager
        .submit_prompt_with_attachment(
            "sidebar-stop",
            &url,
            "SIDEBAR_STOP_ORIGINAL_REPORT",
            None,
            "sidebar-report",
        )
        .await
        .expect("duplicate check");
    let count = page
        .evaluate("window.sent")
        .await
        .expect("send count")
        .into_value::<u64>()
        .expect("numeric count");
    manager.close("sidebar-stop").await.expect("browser closed");
    server.abort();
    assert!(
        matches!(sent, PromptSubmissionOutcome::Verified(_)),
        "sidebars blocked Send readiness"
    );
    assert!(matches!(again, PromptSubmissionOutcome::Verified(_)));
    assert_eq!(count, 1, "report must be sent exactly once");
    assert_eq!(
        busy.expect("real generation blocks delivery")["submission_evidence"],
        "not_sent_chat_busy"
    );
    assert!(
        idle,
        "sidebar title or historical Stop button falsely marked the chat as generating"
    );
    assert!(
        deferred.is_none(),
        "unrelated controls blocked delivery: {deferred:?}"
    );
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

#[tokio::test]
async fn refused_pages_do_not_navigate_or_submit() {
    require_chrome!();
    let (url, server) = local_page(r#"<!doctype html><body><main><section data-testid="conversation-turn-1" data-turn-id="turn"><div data-message-author-role="assistant">old answer</div></section><form><textarea id="prompt-textarea"></textarea><button id="busy" aria-label="Stop streaming">Stop</button></form></main></body>"#).await;
    let temp = tempfile::tempdir().expect("profile");
    let manager = BrowserSurfaces::default();
    manager
        .open("refused", &temp.path().join("profile"), &url, 800, 600)
        .await
        .expect("browser");
    let result = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
        let original = manager.page("refused").await.expect("original");
        for (notice, code) in [("Access denied", "access_denied"), ("Verify you are human", "verification_required"), ("操作过于频繁", "rate_limited")] {
            original.evaluate(format!("document.body.innerHTML='<main><div role=alert>{notice}</div></main>'")).await.expect("restriction fixture");
            let attempt = manager.submit_prompt_with_attachment("refused", "http://127.0.0.1:1/other", "DO_NOT_SEND", None, "blocked").await.expect("must defer without navigation");
            let PromptSubmissionOutcome::Deferred(evidence) = attempt else { panic!("refusal was not preserved") };
            assert_eq!(evidence["submission_evidence"], format!("not_sent_{code}"));
            assert_eq!(original.url().await.expect("URL").expect("page URL").trim_end_matches('/'), url.trim_end_matches('/'));
        }
        original.evaluate(r#"document.body.innerHTML='<main><article><div data-message-author-role="assistant"><div role="alert">Too many requests</div></div></article><form><textarea id="prompt-textarea"></textarea></form></main>'"#).await.expect("quoted restriction");
        assert!(manager.relay_surface_deferral("refused").await.expect("quoted text ignored").is_none());
    })).await;
    manager.close("refused").await.expect("close");
    server.abort();
    result.expect("restriction assertions");
}
