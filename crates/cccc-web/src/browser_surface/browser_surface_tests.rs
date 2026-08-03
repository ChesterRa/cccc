use super::*;
use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
    if !chrome_available() {
        return;
    }
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
            800,
            600,
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
    assert_eq!(frame["width"], 800);
    assert_eq!(frame["height"], 600);
    assert!(manager.close("g_test::slot-1").await.expect("close"));
    server.abort();
}

#[tokio::test]
async fn frame_recreates_a_tab_closed_from_the_system_browser() {
    if !chrome_available() {
        return;
    }
    let (url, server) = local_page("CCCC recovered browser tab").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let key = "closed-tab-recovery";
    manager
        .open(key, &temp.path().join("profile"), &url, 800, 600)
        .await
        .expect("open");
    let page = manager
        .sessions
        .lock()
        .await
        .get(key)
        .expect("session")
        .page
        .clone();
    page.close().await.expect("close browser tab");

    let frame = manager.frame(key).await.expect("recover and capture frame");

    assert!(!frame["data_base64"].as_str().unwrap_or_default().is_empty());
    assert_eq!(
        frame["url"]
            .as_str()
            .unwrap_or_default()
            .trim_end_matches('/'),
        url.trim_end_matches('/')
    );
    assert!(manager.close(key).await.expect("close"));
    server.abort();
}

#[tokio::test]
async fn closed_auth_tab_is_not_recreated() {
    if !chrome_available() {
        return;
    }
    let (url, server) = local_page("CCCC close auth tab").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();
    let key = "closed-auth-tab";
    manager
        .open(key, &temp.path().join("profile"), &url, 800, 600)
        .await
        .expect("open");
    let page = {
        let mut sessions = manager.sessions.lock().await;
        let session = sessions.get_mut(key).expect("session");
        session.recover_closed_page = false;
        session.page.clone()
    };
    page.close().await.expect("close auth tab");

    assert!(manager.frame(key).await.is_err());
    let status = manager.info(key).await;
    assert_eq!(status["active"], false);
    assert_eq!(status["state"], "closed");
    assert!(manager.sessions.lock().await.get(key).is_none());
    server.abort();
}

#[tokio::test]
async fn open_completes_after_dom_content_loaded_without_waiting_for_subresources() {
    if !chrome_available() {
        return;
    }
    let (url, server) = page_with_stalled_subresource().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let manager = BrowserSurfaces::default();

    let state = tokio::time::timeout(
        std::time::Duration::from_secs(5),
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
    assert!(manager.close("dom-content-loaded").await.expect("close"));
    server.abort();
}

#[tokio::test]
async fn concurrent_ensure_open_reuses_one_profile_owner() {
    if !chrome_available() {
        return;
    }
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
    if !chrome_available() {
        return;
    }
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
    if !chrome_available() {
        return;
    }
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
    if !chrome_available() {
        return;
    }
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
    if !chrome_available() {
        return;
    }
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
    if !chrome_available() {
        return;
    }
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
    if !chrome_available() {
        return;
    }
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
    if !chrome_available() {
        return;
    }
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
