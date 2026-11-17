use super::*;
use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_daemon::session_owner::{AgentIdentity, AgentObservations};
use std::time::{Duration, Instant};

const KEY: &str = "web-model::g_kept::member";
const OTHER: &str = "web-model::g_kept::other";
const CREATED_AT: &str = "2026-11-18T00:00:00Z";

/// A process that exists and has not exited; an unreaped child counts as
/// exited.
fn running(pid: u64) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .is_some_and(|(_, rest)| !rest.trim_start().starts_with('Z'))
    })
}

async fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn target(surfaces: &BrowserSurfaces) -> String {
    target_of(surfaces, KEY).await
}

async fn target_of(surfaces: &BrowserSurfaces, key: &str) -> String {
    surfaces
        .sessions
        .lock()
        .await
        .get(key)
        .expect("member window")
        .page
        .target_id()
        .inner()
        .clone()
}

fn pid(surface: &Value) -> u64 {
    surface["metadata"]["pid"].as_u64().expect("browser pid")
}

async fn call(client: &DaemonClient, op: &str) -> serde_json::Map<String, Value> {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: Default::default(),
        })
        .await
        .expect(op);
    assert!(response.ok, "{op}: {:?}", response.error);
    response.result
}

/// Starts a daemon for `home` and returns its task with its first answer.
async fn start_daemon(
    home: &cccc_core::HomeLayout,
    client: &DaemonClient,
) -> (
    tokio::task::JoinHandle<anyhow::Result<()>>,
    serde_json::Map<String, Value>,
) {
    let daemon = tokio::spawn({
        let home = home.clone();
        async move { cccc_daemon::run(home).await }
    });
    let ping = DaemonRequest {
        v: 1,
        op: "ping".into(),
        args: Default::default(),
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(response) = client.call(&ping).await
            && response.ok
        {
            return (daemon, response.result);
        }
        assert!(Instant::now() < deadline, "the daemon did not answer");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn observe(home: &cccc_core::HomeLayout) -> AgentObservations {
    cccc_daemon::session_owner::observations(home, Duration::from_secs(5))
        .await
        .expect("member observations")
}

/// Ends this Web's connection to the kept browser, as a broken connection
/// would, while the browser keeps running.
async fn lose_connection(surfaces: &BrowserSurfaces) {
    let owners = surfaces
        .owners
        .lock()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(owners.len(), 1, "one kept browser");
    let deadline = Instant::now() + Duration::from_secs(15);
    for owner in owners {
        owner.read().await.handler.abort();
        while !owner.read().await.handler.is_finished() {
            assert!(Instant::now() < deadline, "the connection did not end");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// The session owner keeps a Web model member's shared browser across a Web
/// and a daemon restart: the restarted daemon reports the member retained,
/// and running again only once a new Web takes back the same window in the
/// same browser process, even at another size. When the Web's connection
/// ends, a status read changes nothing, opening again reconnects to the same
/// window, and closing one window leaves the browser and the other member's
/// window. The window opens again after it is closed or its browser process
/// ends, and stopping the daemon stops the browser it kept.
#[tokio::test]
async fn kept_browser_returns_to_the_next_web_and_reopens_after_it_ends() {
    require_chrome!();
    if !std::path::Path::new("/usr/bin/Xvfb").is_file() {
        return;
    }
    let temp = tempfile::tempdir().expect("temp");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let client = DaemonClient::new(home.clone());
    let (daemon, _) = start_daemon(&home, &client).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let url = format!("http://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route(
                "/",
                axum::routing::get(|| async { axum::response::Html("<h1>member</h1>") }),
            ),
        )
        .await
    });
    let profile = temp.path().join("profile");
    let web = || {
        let surfaces = BrowserSurfaces::default();
        surfaces.hold_shared_browsers(client.clone());
        surfaces
    };

    // The owner starts the browser for the first Web, which then stops.
    let first = web();
    let opened = first
        .ensure_open_shared_actor(KEY, &profile, &url, (800, 600), "gen", CREATED_AT)
        .await
        .expect("open through the session owner");
    let browser = pid(&opened);
    let window = target(&first).await;
    crate::shutdown::browser_surfaces(&first).await;
    assert!(running(browser), "the kept browser outlives the Web");

    // The daemon stops keeping its members and starts again. The owner
    // reports the window's member as retained, but not yet as running: no
    // Web has taken the window back since.
    call(&client, "shutdown_preserving_agents").await;
    daemon.await.expect("daemon task").expect("daemon stopped");
    assert!(running(browser), "the kept browser outlives the daemon");
    let (daemon, ping) = start_daemon(&home, &client).await;
    let retained: Vec<AgentIdentity> =
        serde_json::from_value(ping["retained_agents"].clone()).expect("retained members");
    assert_eq!(retained.len(), 1, "{retained:?}");
    assert_eq!(
        (
            retained[0].group_id.as_str(),
            retained[0].actor_id.as_str(),
            retained[0].actor_created_at.as_str()
        ),
        ("g_kept", "member", CREATED_AT)
    );
    let generation = retained[0].generation.as_deref().expect("generation");
    assert!(generation.ends_with(&format!(":{window}")), "{generation}");
    let observed = observe(&home).await;
    assert!(observed.running.is_empty(), "{:?}", observed.running);
    assert_eq!(observed.unconfirmed, retained);

    // The next Web asks for another size and gets the same window back,
    // which makes the member running again with the same generation.
    let second = web();
    let reopened = second
        .ensure_open_shared_actor(KEY, &profile, &url, (1024, 700), "gen", CREATED_AT)
        .await
        .expect("take the kept window back");
    assert_eq!(pid(&reopened), browser);
    assert_eq!(target(&second).await, window);
    let observed = observe(&home).await;
    assert_eq!(observed.running, retained);
    assert!(
        observed.unconfirmed.is_empty(),
        "{:?}",
        observed.unconfirmed
    );

    // Another member's window opens in the same browser.
    let other = second
        .ensure_open_shared_actor(OTHER, &profile, &url, (800, 600), "gen", CREATED_AT)
        .await
        .expect("open another member's window");
    assert_eq!(pid(&other), browser);
    let other_window = target_of(&second, OTHER).await;

    // The connection ends while the browser runs: reading the status reports
    // it and stops nothing; opening again reconnects to the same window.
    lose_connection(&second).await;
    let status = second.info(KEY).await;
    assert_eq!(status["error"]["code"], "browser_surface_disconnected");
    assert!(running(browser), "a status read leaves the kept browser");
    let reconnected = second
        .ensure_open_shared_actor(KEY, &profile, &url, (800, 600), "gen", CREATED_AT)
        .await
        .expect("reconnect to the kept window");
    assert_eq!(pid(&reconnected), browser);
    assert_eq!(target(&second).await, window);
    assert_eq!(target_of(&second, OTHER).await, other_window);

    // Closing a window without a connection closes only that window.
    lose_connection(&second).await;
    assert!(second.close(KEY).await.expect("close without a connection"));
    assert!(
        running(browser),
        "the other member's window keeps the browser"
    );
    assert_eq!(second.info(OTHER).await["active"], true);
    assert_eq!(target_of(&second, OTHER).await, other_window);
    let owner = second
        .owners
        .lock()
        .await
        .values()
        .next()
        .cloned()
        .expect("kept browser");
    let targets = owner
        .read()
        .await
        .browser
        .execute(GetTargetsParams::default())
        .await
        .expect("list windows");
    assert!(
        !targets
            .result
            .target_infos
            .iter()
            .any(|target| target.target_id.inner() == &window),
        "the closed window is gone"
    );

    // Its windows closed, the member's window opens again.
    assert!(second.close(OTHER).await.expect("close the other window"));
    let after_close = second
        .ensure_open_shared_actor(KEY, &profile, &url, (800, 600), "gen", CREATED_AT)
        .await
        .expect("open after the windows closed");
    let browser = pid(&after_close);

    // The browser process ended: the owner starts another one.
    let killed = std::process::Command::new("kill")
        .args(["-KILL", &browser.to_string()])
        .status()
        .expect("run kill");
    assert!(killed.success(), "end the kept browser");
    wait_until("the browser to end", || !running(browser)).await;
    let mut restarted = None;
    let deadline = Instant::now() + Duration::from_secs(15);
    while restarted.is_none() {
        // Reconnecting finds the old browser ended once the owner sees it.
        match second
            .ensure_open_shared_actor(KEY, &profile, &url, (800, 600), "gen", CREATED_AT)
            .await
        {
            Ok(surface) if pid(&surface) != browser => restarted = Some(pid(&surface)),
            result => {
                assert!(
                    Instant::now() < deadline,
                    "no new browser after the old one ended: {result:?}"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    let browser = restarted.expect("new browser");
    assert!(running(browser));

    // Stopping the daemon stops the browser the owner kept.
    crate::shutdown::browser_surfaces(&second).await;
    assert!(running(browser), "a stopping Web leaves the kept browser");
    client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await
        .expect("stop daemon");
    daemon.await.expect("daemon task").expect("daemon stopped");
    wait_until("the daemon to stop the kept browser", || !running(browser)).await;
    server.abort();
}

/// The process ID of the one session owner of this home that is running. A
/// killed owner's address stays until the next daemon start retires it.
fn owner_pid(home: &cccc_core::HomeLayout) -> u64 {
    let running_owners = std::fs::read_dir(home.daemon_dir().join("session-owners"))
        .expect("session owners")
        .map(|entry| entry.expect("owner entry").path().join("address.json"))
        .filter(|address| address.is_file())
        .map(|address| {
            let address: Value =
                serde_json::from_slice(&std::fs::read(&address).expect("owner address"))
                    .expect("owner address json");
            address["pid"].as_u64().expect("owner pid")
        })
        .filter(|pid| running(*pid))
        .collect::<Vec<_>>();
    assert_eq!(
        running_owners.len(),
        1,
        "one running session owner: {running_owners:?}"
    );
    running_owners[0]
}

/// A session owner killed outright stops nothing it kept, so its browser
/// keeps running, and the running daemon reports the owner unreachable
/// instead of replacing it. The next daemon start recovers the dead owner;
/// the owner that start creates then verifies the browser left behind
/// through the profile lease and stops it before starting another one.
#[tokio::test]
async fn next_owner_stops_the_browser_a_killed_owner_left() {
    require_chrome!();
    if !std::path::Path::new("/usr/bin/Xvfb").is_file() {
        return;
    }
    let temp = tempfile::tempdir().expect("temp");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let client = DaemonClient::new(home.clone());
    let (daemon, _) = start_daemon(&home, &client).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let url = format!("http://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route(
                "/",
                axum::routing::get(|| async { axum::response::Html("<h1>member</h1>") }),
            ),
        )
        .await
    });
    let profile = temp.path().join("profile");
    let web = || {
        let surfaces = BrowserSurfaces::default();
        surfaces.hold_shared_browsers(client.clone());
        surfaces
    };

    let first = web();
    let opened = first
        .ensure_open_shared_actor(KEY, &profile, &url, (800, 600), "gen", CREATED_AT)
        .await
        .expect("open through the session owner");
    let left = pid(&opened);
    crate::shutdown::browser_surfaces(&first).await;

    // Nothing a stopping owner does runs after SIGKILL.
    let owner = owner_pid(&home);
    let killed = std::process::Command::new("kill")
        .args(["-KILL", &owner.to_string()])
        .status()
        .expect("run kill");
    assert!(killed.success(), "kill the session owner");
    wait_until("the session owner to end", || !running(owner)).await;
    assert!(running(left), "a killed owner leaves its browser running");

    // The running daemon does not replace an owner it cannot reach.
    let unreachable = web()
        .ensure_open_shared_actor(KEY, &profile, &url, (800, 600), "gen", CREATED_AT)
        .await;
    assert!(
        unreachable.is_err(),
        "no browser while the owner is unreachable: {unreachable:?}"
    );
    assert!(running(left));

    // The daemon stops; the next start recovers the dead owner.
    let stopped = client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await;
    let exited = tokio::time::timeout(Duration::from_secs(30), daemon)
        .await
        .expect("the daemon exits with its owner unreachable");
    eprintln!("stop with an unreachable owner: {stopped:?}; daemon task: {exited:?}");
    let (daemon, _) = start_daemon(&home, &client).await;

    let second = web();
    let reopened = second
        .ensure_open_shared_actor(KEY, &profile, &url, (800, 600), "gen", CREATED_AT)
        .await
        .expect("open through the next owner");
    let browser = pid(&reopened);
    assert_ne!(browser, left, "a new browser");
    assert_ne!(owner_pid(&home), owner, "a new session owner");
    wait_until("the left browser to be stopped", || !running(left)).await;
    assert!(running(browser));

    crate::shutdown::browser_surfaces(&second).await;
    client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await
        .expect("stop daemon");
    daemon.await.expect("daemon task").expect("daemon stopped");
    wait_until("the daemon to stop the new browser", || !running(browser)).await;
    server.abort();
}
