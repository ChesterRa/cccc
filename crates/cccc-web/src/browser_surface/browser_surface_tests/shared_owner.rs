use super::*;
use chromiumoxide::cdp::browser_protocol::browser::GetWindowForTargetParams;
use std::sync::Arc;

async fn owned_page(manager: &BrowserSurfaces, key: &str) -> Page {
    manager
        .sessions
        .lock()
        .await
        .get(key)
        .expect("owned surface")
        .page
        .clone()
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn shared_windows_submit_independent_batches_while_viewers_capture() {
    require_chrome!();
    if !std::path::Path::new("/usr/bin/Xvfb").is_file() {
        return;
    }
    let (url, server) = local_page(r#"<main><form style="position:fixed;bottom:10px;left:20px;right:20px" onsubmit="event.preventDefault();const t=document.querySelector('textarea');const a=document.createElement('article');a.dataset.messageAuthorRole='user';a.textContent=t.value;document.querySelector('main').prepend(a);t.value='';window.sends++"><textarea id="prompt-textarea" style="width:90%;height:40px;transition:height .2s" oninput="this.style.height=this.value.length>200?'200px':'40px'"></textarea><button id="composer-submit-button" type="submit">Send</button></form></main><script>window.sends=0</script>"#).await;
    let temp = tempfile::tempdir().expect("fixture");
    let profile = temp.path().join("profile");
    let manager = BrowserSurfaces::default();
    let keys = ["web-model::g_fixture::A", "web-model::g_fixture::B"];
    for key in keys {
        manager
            .ensure_open_shared_actor(key, &profile, &url, (800, 600), key)
            .await
            .expect("window");
    }
    for (index, body) in ["long text ".repeat(100), "short text".into()]
        .iter()
        .enumerate()
    {
        let a =
            format!("[cccc] Browser batch webdelivery:A:{index} events=a{index} actor=A\n{body}");
        let b =
            format!("[cccc] Browser batch webdelivery:B:{index} events=b{index} actor=B\n{body}");
        let (a_result, b_result, a_frame, b_frame) = tokio::join!(
            manager.submit_prompt_with_attachment(keys[0], &url, &a, None, "a", None),
            manager.submit_prompt_with_attachment(keys[1], &url, &b, None, "b", None),
            manager.frame(keys[0]),
            manager.frame(keys[1])
        );
        a_frame.expect("A viewer");
        b_frame.expect("B viewer");
        for result in [a_result, b_result] {
            let evidence = match result.expect("submission") {
                PromptSubmissionOutcome::Verified(evidence) => evidence,
                PromptSubmissionOutcome::Ambiguous(evidence) => {
                    panic!("independent send ambiguous: {evidence}")
                }
                PromptSubmissionOutcome::Deferred(evidence) => {
                    panic!("independent send deferred: {evidence}")
                }
            };
            assert_eq!(evidence["submission_evidence"], "message_echo");
        }
    }
    for (key, actor) in keys.into_iter().zip(["A", "B"]) {
        let page = owned_page(&manager, key).await;
        let actual = page.evaluate("({sends:window.sends, messages:[...document.querySelectorAll('[data-message-author-role=user]')].map(e=>e.textContent),draft:document.querySelector('textarea').value})").await.expect("read window").into_value::<Value>().expect("snapshot");
        assert_eq!(actual["sends"], 2);
        assert_eq!(actual["draft"], "");
        assert!(
            actual["messages"]
                .as_array()
                .expect("messages")
                .iter()
                .all(|s| s
                    .as_str()
                    .expect("text")
                    .contains(&format!("actor={actor}")))
        );
    }
    manager.shutdown_all().await.expect("cleanup");
    server.abort();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn shared_windows_keep_login_and_close_only_their_own_surface() {
    require_chrome!();
    if !std::path::Path::new("/usr/bin/Xvfb").is_file() {
        return;
    }
    let (url, server) = local_page("<textarea id='prompt-textarea'></textarea>").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let profile = temp.path().join("profile");
    let manager = BrowserSurfaces::default();
    let a_url = format!("{url}#a");
    let b_url = format!("{url}#b");
    let (a, b) = tokio::join!(
        manager.ensure_open_shared_actor(
            "web-model::g_a::a",
            &profile,
            &a_url,
            (800, 600),
            "gen-a"
        ),
        manager.ensure_open_shared_actor(
            "web-model::g_b::b",
            &profile,
            &b_url,
            (800, 600),
            "gen-b"
        ),
    );
    assert_eq!(
        a.expect("A")["metadata"]["pid"],
        b.expect("B")["metadata"]["pid"]
    );
    assert_eq!(manager.owners.lock().await.len(), 1);
    let a = owned_page(&manager, "web-model::g_a::a").await;
    let b = owned_page(&manager, "web-model::g_b::b").await;
    assert_ne!(a.target_id(), b.target_id());
    let owner = manager
        .sessions
        .lock()
        .await
        .get("web-model::g_a::a")
        .expect("A owner")
        .owner
        .clone();
    let wa = owner
        .read()
        .await
        .browser
        .execute(
            GetWindowForTargetParams::builder()
                .target_id(a.target_id().clone())
                .build(),
        )
        .await
        .expect("A window")
        .result
        .window_id;
    let wb = owner
        .read()
        .await
        .browser
        .execute(
            GetWindowForTargetParams::builder()
                .target_id(b.target_id().clone())
                .build(),
        )
        .await
        .expect("B window")
        .result
        .window_id;
    assert_ne!(wa, wb);
    a.evaluate("document.cookie='fixture_login=shared;Path=/;Max-Age=3600';localStorage.setItem('fixture-user','same-user');document.querySelector('textarea').focus()").await.expect("fixture sign in");
    b.evaluate("document.querySelector('textarea').focus()")
        .await
        .expect("focus B");
    let a_text = json!({"t":"text","text":"A draft"});
    let b_text = json!({"t":"text","text":"B draft"});
    let (ra, rb) = tokio::join!(
        manager.command("web-model::g_a::a", &a_text),
        manager.command("web-model::g_b::b", &b_text),
    );
    ra.expect("type A");
    rb.expect("type B");
    assert!(b.evaluate("document.cookie.includes('fixture_login=shared') && localStorage.getItem('fixture-user')==='same-user' && document.querySelector('textarea').value==='B draft'").await.expect("shared login").into_value::<bool>().expect("bool"));
    assert_eq!(
        a.evaluate("document.querySelector('textarea').value")
            .await
            .expect("A draft")
            .into_value::<String>()
            .expect("text"),
        "A draft"
    );
    assert!(
        manager.frame("web-model::g_a::a").await.expect("A frame")["data_base64"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
    let (same_a, same_again) = tokio::join!(
        manager.ensure_open_shared_actor(
            "web-model::g_a::a",
            &profile,
            &a_url,
            (800, 600),
            "gen-a"
        ),
        manager.ensure_open_shared_actor(
            "web-model::g_a::a",
            &profile,
            &a_url,
            (800, 600),
            "gen-a"
        ),
    );
    same_a.expect("reuse");
    same_again.expect("concurrent reuse");
    assert_eq!(
        owned_page(&manager, "web-model::g_a::a").await.target_id(),
        a.target_id()
    );
    manager
        .ensure_open_shared_actor(
            "web-model::g_a::a",
            &profile,
            &a_url,
            (800, 600),
            "new-gen-a",
        )
        .await
        .expect("new Actor generation");
    assert_ne!(
        owned_page(&manager, "web-model::g_a::a").await.target_id(),
        a.target_id()
    );
    assert_eq!(
        manager.info("web-model::g_a::a").await["metadata"]["actor_generation"],
        "new-gen-a"
    );
    manager.close("web-model::g_a::a").await.expect("close A");
    assert!(manager.page_available("web-model::g_b::b").await);
    assert_eq!(
        b.evaluate("document.querySelector('textarea').value")
            .await
            .expect("kept B draft")
            .into_value::<String>()
            .expect("text"),
        "B draft"
    );
    manager
        .close("web-model::g_b::b")
        .await
        .expect("close last Actor");
    assert!(manager.sessions.lock().await.is_empty());
    assert_eq!(
        manager.owners.lock().await.len(),
        1,
        "shared login browser remains owned"
    );
    let other = BrowserSurfaces::default();
    assert!(
        other
            .ensure_open_shared_system("competitor", &profile, &url, 800, 600)
            .await
            .is_err(),
        "another Web owner must not attach to the profile"
    );
    manager
        .ensure_open_shared_system("web-model::g_a::a", &profile, &url, 800, 600)
        .await
        .expect("reopen A");
    let reopened = owned_page(&manager, "web-model::g_a::a").await;
    assert_ne!(a.target_id(), reopened.target_id());
    assert!(
        reopened
            .evaluate("document.cookie.includes('fixture_login=shared')")
            .await
            .expect("retained sign in")
            .into_value::<bool>()
            .expect("bool")
    );
    let held = owner.write().await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), manager.shutdown_all())
            .await
            .is_err(),
        "stalled owner shutdown is bounded by the caller"
    );
    drop(held);
    assert_eq!(manager.shutdown_all().await.expect("shutdown"), 1);
    assert!(manager.owners.lock().await.is_empty());
    // The Arc above deliberately outlives the browser; cleanup must still release the profile lease.
    other
        .ensure_open_shared_system("restart", &profile, &url, 800, 600)
        .await
        .expect("new owner after shutdown");
    assert!(
        owned_page(&other, "restart")
            .await
            .evaluate("document.cookie.includes('fixture_login=shared')")
            .await
            .expect("persistent sign in")
            .into_value::<bool>()
            .expect("bool")
    );
    other.shutdown_all().await.expect("cleanup");
    server.abort();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn shared_page_commands_do_not_adopt_popups_or_block_other_surfaces() {
    require_chrome!();
    if !std::path::Path::new("/usr/bin/Xvfb").is_file() {
        return;
    }
    let (url, server) = local_page("<button style='position:absolute;left:0;top:0;width:200px;height:100px' onclick=\"window.open('about:blank')\">Popup</button><textarea></textarea>").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let profile = temp.path().join("profile");
    let manager = Arc::new(BrowserSurfaces::default());
    for key in ["web-model::g_fixture::A", "web-model::g_fixture::B"] {
        manager
            .ensure_open_shared_system(key, &profile, &url, 800, 600)
            .await
            .expect("window");
    }
    let a = owned_page(&manager, "web-model::g_fixture::A").await;
    let b = owned_page(&manager, "web-model::g_fixture::B").await;
    manager
        .command(
            "web-model::g_fixture::A",
            &json!({"t":"click","x":40,"y":40}),
        )
        .await
        .expect("click");
    assert_eq!(
        owned_page(&manager, "web-model::g_fixture::A")
            .await
            .target_id(),
        a.target_id(),
        "a popup must not replace the owned business target"
    );
    a.clone().close().await.expect("close target outside CCCC");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if manager.info("web-model::g_fixture::A").await["state"] == "closed" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("closed target observed");
    assert_eq!(
        manager.info("web-model::g_fixture::A").await["state"],
        "closed",
        "manual close stays paused"
    );
    assert!(
        manager.frame("web-model::g_fixture::A").await.is_err(),
        "closed A must not recover by adopting B or a popup"
    );
    assert_eq!(
        owned_page(&manager, "web-model::g_fixture::B")
            .await
            .target_id(),
        b.target_id()
    );
    manager
        .ensure_open_shared_system("web-model::g_fixture::A", &profile, &url, 800, 600)
        .await
        .expect("restore A");
    // A navigation that never produces headers must not hold the surface registry.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("stalled server");
    let stalled_url = format!("http://{}/", listener.local_addr().expect("address"));
    let (started, reached) = tokio::sync::oneshot::channel();
    let stalled = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.expect("navigation");
        let _ = started.send(());
        std::future::pending::<()>().await;
    });
    let pending = {
        let manager = Arc::clone(&manager);
        tokio::spawn(async move {
            manager
                .ensure_open_shared_system("C", &profile, &stalled_url, 800, 600)
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), reached)
        .await
        .expect("navigation began")
        .expect("signal");
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        manager.ensure_open_shared_system("D", &temp.path().join("profile"), &url, 800, 600),
    )
    .await
    .expect("D registration must not wait for C navigation")
    .expect("D open");
    let frame = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        manager.frame("web-model::g_fixture::B"),
    )
    .await
    .expect("B should not wait for A")
    .expect("B frame");
    assert!(frame["data_base64"].as_str().is_some());
    pending.abort();
    let _ = pending.await;
    stalled.abort();
    manager
        .shutdown_all()
        .await
        .expect("cleanup all owned windows and popups");
    server.abort();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn shared_login_open_recovers_blank_without_replacing_actor_pages() {
    require_chrome!();
    if !std::path::Path::new("/usr/bin/Xvfb").is_file() {
        return;
    }
    let (url, server) = local_page("<h1>Shared login fixture</h1><textarea></textarea>").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let profile = temp.path().join("profile");
    let manager = BrowserSurfaces::default();
    manager
        .ensure_open_shared_system("login", &profile, &url, 1000, 700)
        .await
        .expect("login");
    let login = owned_page(&manager, "login").await;
    manager
        .ensure_open_shared_actor(
            "actor",
            &profile,
            &format!("{url}#actor"),
            (1000, 700),
            "gen",
        )
        .await
        .expect("actor");
    let actor = owned_page(&manager, "actor").await;
    actor
        .evaluate("document.querySelector('textarea').value='keep my draft'")
        .await
        .expect("draft");
    login
        .evaluate(
            "location.hash='sign-in';document.querySelector('textarea').value='sign-in draft'",
        )
        .await
        .expect("sign in started");
    manager
        .ensure_open_shared_system("login", &profile, &url, 1000, 700)
        .await
        .expect("show sign in");
    assert!(login.evaluate("location.hash==='#sign-in' && document.querySelector('textarea').value==='sign-in draft'").await.expect("preserve ongoing sign in").into_value::<bool>().expect("bool"));
    login.goto("about:blank").await.expect("blank");
    manager
        .ensure_open_shared_system("login", &profile, &url, 1000, 700)
        .await
        .expect("reopen login");
    let result = login
        .evaluate("location.href")
        .await
        .expect("url")
        .into_value::<String>()
        .expect("string");
    let actor_text = actor
        .evaluate("document.querySelector('textarea').value")
        .await
        .expect("kept actor")
        .into_value::<String>()
        .expect("text");
    let focused = login
        .evaluate("document.hasFocus()")
        .await
        .expect("focus")
        .into_value::<bool>()
        .expect("bool");
    manager.shutdown_all().await.expect("cleanup");
    server.abort();
    assert_eq!(
        result.trim_end_matches('/'),
        url,
        "Open must recover the same login target from blank"
    );
    assert_eq!(actor_text, "keep my draft");
    assert!(focused, "explicit login open must reveal its own window");
}
