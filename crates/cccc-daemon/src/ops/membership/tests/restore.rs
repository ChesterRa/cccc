use super::*;
#[cfg(unix)]
use cccc_core::{fs, web_runtime_proof};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::thread;

const CREDENTIALS: &str =
    r#"{"hostname":"https://fixture.example.test","tunnel_token":"isolated-tunnel-fixture"}"#;

struct HttpFixture {
    port: u16,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl HttpFixture {
    fn new(handler: impl Fn(&str) -> (u16, String) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let port = listener.local_addr().expect("address").port();
        listener.set_nonblocking(true).expect("nonblocking");
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let worker = thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .expect("timeout");
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 4096];
                    let count = stream.read(&mut buffer).expect("request");
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                        let length = header
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .map_or(0, |length| length.trim().parse::<usize>().expect("length"));
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(bytes).expect("utf8");
                captured.lock().expect("capture").push(request.clone());
                let (status, body) = handler(&request);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Self {
            port,
            stop,
            requests,
            worker: Some(worker),
        }
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    #[cfg(unix)]
    fn web(home: &HomeLayout) -> Self {
        let fixture = Self::new(|request| {
            let target = request.split_whitespace().nth(1).expect("target");
            let challenge = target
                .strip_prefix("/api/v1/ready?challenge=")
                .expect("challenge");
            let proof = web_runtime_proof::sign("isolated-proof", challenge).expect("proof");
            (200, json!({"ok":true,"result":{"web":"ready","runtime_id":"fixture-web","proof":proof}}).to_string())
        });
        fs::write_json(&home.daemon_dir().join("web_runtime.json"), &json!({
            "pid":std::process::id(), "runtime_id":"fixture-web", "runtime_proof_key":"isolated-proof",
            "host":"127.0.0.1", "port":fixture.port
        })).expect("web identity");
        fixture
    }
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker
            .take()
            .expect("worker")
            .join()
            .expect("fixture finished");
    }
}

struct Installation {
    home: HomeLayout,
    _temp: tempfile::TempDir,
}

impl Installation {
    fn new(origin: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        AccessTokenStore::new(home.clone())
            .expect("tokens")
            .create("admin", vec![], true, Some("acc_restore_fixture"))
            .expect("admin");
        membership::save(
            &home,
            &membership::MembershipState {
                logged_in: true,
                account_origin: Some(origin.into()),
                device_id: Some("fixture-device".into()),
                device_token: Some("isolated-device-fixture".into()),
                ..Default::default()
            },
        )
        .expect("membership");
        settings::update(&home, |global| {
            global
                .remote_access
                .insert("provider".into(), "reach".into());
            global.remote_access.insert("enabled".into(), true.into());
            global.remote_access.insert("web_port".into(), 1.into());
            Ok(())
        })
        .expect("enabled Reach");
        Self { home, _temp: temp }
    }
}

impl Drop for Installation {
    fn drop(&mut self) {
        membership_cloudflared::stop(&self.home).expect("stop isolated helper");
    }
}

fn attempt(
    home: &HomeLayout,
    start: impl FnOnce(&HomeLayout, &str) -> Result<(), RuntimeError>,
) -> Result<(), OpError> {
    restore_once(
        home,
        &DispatchLocks::default(),
        &AtomicBool::new(false),
        |_| Ok(9123),
        |_| Ok(()),
        start,
    )
}

#[cfg(unix)]
#[test]
fn delayed_web_then_restore_starts_one_helper_and_leaves_it_running() {
    let account = HttpFixture::new(|_| (200, CREDENTIALS.into()));
    let installation = Installation::new(&account.origin());
    let home = &installation.home;
    let locks = DispatchLocks::default();
    let cancelled = AtomicBool::new(false);
    let start = |home: &HomeLayout, _: &str| membership_cloudflared::start_restore_fixture(home);
    assert!(
        restore_once(
            home,
            &locks,
            &cancelled,
            web_runtime::live_web_port,
            |_| Ok(()),
            start
        )
        .is_err()
    );
    assert!(account.requests.lock().expect("requests").is_empty());
    let web = HttpFixture::web(home);
    restore_once(
        home,
        &locks,
        &cancelled,
        web_runtime::live_web_port,
        |_| Ok(()),
        start,
    )
    .expect("restore");
    assert!(membership_cloudflared::status(home).running);
    let request = account.requests.lock().expect("requests")[0].clone();
    assert!(request.starts_with("POST /v1/reach "));
    assert!(request.contains(&format!(r#""origin_port":{}"#, web.port)));
    assert!(membership::load(home).expect("state").last_error.is_none());
    assert_eq!(
        settings::load(home).expect("settings").remote_access["web_public_url"],
        "https://fixture.example.test"
    );
    restore_once(
        home,
        &locks,
        &cancelled,
        |_| panic!("running helper needs no readiness poll"),
        |_| panic!("running helper needs no hash check"),
        |_, _| panic!("must not restart"),
    )
    .expect("already running");
    assert_eq!(account.requests.lock().expect("requests").len(), 1);
}

#[test]
fn disabled_unlinked_and_other_providers_do_not_restore() {
    for state in ["off", "unlinked", "cut", "manual", "tailscale"] {
        let installation = Installation::new("http://127.0.0.1:1");
        let home = &installation.home;
        settings::update(home, |global| {
            if state == "off" {
                global.remote_access.insert("enabled".into(), false.into());
            }
            if matches!(state, "manual" | "tailscale") {
                global.remote_access.insert("provider".into(), state.into());
            }
            Ok(())
        })
        .expect("settings");
        membership::update(home, |member| {
            if state == "unlinked" {
                member.logged_in = false;
            }
            if state == "cut" {
                member.disabled = true;
            }
            Ok(())
        })
        .expect("state");
        restore_once(
            home,
            &DispatchLocks::default(),
            &AtomicBool::new(false),
            |_| panic!("ineligible Reach must not perform work"),
            |_| Ok(()),
            |_, _| panic!("start"),
        )
        .expect("skip");
    }
}

#[test]
fn missing_helper_does_not_provision_or_download() {
    let account = HttpFixture::new(|_| panic!("must not issue credentials without helper"));
    let installation = Installation::new(&account.origin());
    let error = restore_once(
        &installation.home,
        &DispatchLocks::default(),
        &AtomicBool::new(false),
        |_| Ok(9123),
        |home| {
            membership_cloudflared::installed_binary(home)
                .map(|_| ())
                .map_err(runtime_error)
        },
        |_, _| panic!("start"),
    )
    .expect_err("missing pinned helper");
    assert!(error.message.contains("pinned cloudflared"));
    assert!(account.requests.lock().expect("requests").is_empty());
    assert!(boolean(
        &settings::load(&installation.home)
            .expect("settings")
            .remote_access,
        "enabled",
        false
    ));
}

#[test]
fn transient_account_failure_preserves_intent_and_recovers() {
    let failed = AtomicBool::new(false);
    let account = HttpFixture::new(move |_| {
        if !failed.swap(true, Ordering::AcqRel) {
            (503, "{}".into())
        } else {
            (200, CREDENTIALS.into())
        }
    });
    let installation = Installation::new(&account.origin());
    assert!(
        attempt(&installation.home, |_, _| panic!(
            "failed request cannot start"
        ))
        .is_err()
    );
    let state = membership::load(&installation.home).expect("state");
    assert!(state.logged_in && !state.disabled && state.last_error.is_some());
    attempt(&installation.home, |_, token| {
        assert_eq!(token, "isolated-tunnel-fixture");
        Ok(())
    })
    .expect("recover");
    assert!(
        membership::load(&installation.home)
            .expect("state")
            .last_error
            .is_none()
    );
}

#[test]
fn definitive_account_rejection_cuts_saved_intent() {
    let account = HttpFixture::new(|_| (403, r#"{"error":"device_disabled"}"#.into()));
    let installation = Installation::new(&account.origin());
    attempt(&installation.home, |_, _| {
        panic!("rejected device cannot start")
    })
    .expect_err("disabled");
    assert!(
        membership::load(&installation.home)
            .expect("state")
            .disabled
    );
    assert!(!boolean(
        &settings::load(&installation.home)
            .expect("settings")
            .remote_access,
        "enabled",
        true
    ));
}

#[test]
fn failed_logout_keeps_retirement_credentials_without_reopening_reach() {
    let failed = AtomicBool::new(false);
    let account = HttpFixture::new(move |request| {
        if request.starts_with("POST /v1/device/disable ") {
            if !failed.swap(true, Ordering::AcqRel) {
                (503, "{}".into())
            } else {
                (200, r#"{"disabled":true}"#.into())
            }
        } else {
            (200, CREDENTIALS.into())
        }
    });
    let installation = Installation::new(&account.origin());
    let home = &installation.home;
    let request = DaemonRequest {
        v: 1,
        op: "membership_logout".into(),
        args: json!({"by":"user"}).as_object().cloned().expect("args"),
    };
    let locks = DispatchLocks::default();
    {
        let _permit = locks.try_global_write().expect("logout permit");
        super::super::logout(home, &request).expect_err("remote retirement temporarily fails");
    }
    let state = membership::load(home).expect("state");
    assert!(state.logged_in && state.device_token.is_some());
    restore_once(
        home,
        &locks,
        &AtomicBool::new(false),
        |_| Ok(9123),
        |_| Ok(()),
        |_, _| panic!("failed remote retirement must not undo the user's local logout stop"),
    )
    .expect("Reach remains stopped while logout can be retried");
    assert!(!boolean(
        &settings::load(home).expect("settings").remote_access,
        "enabled",
        true
    ));
    assert_eq!(account.requests.lock().expect("requests").len(), 1);
    super::super::logout(home, &request).expect("retry retirement using the preserved credential");
    assert!(!membership::load(home).expect("state").logged_in);
    assert_eq!(account.requests.lock().expect("requests").len(), 2);
}

#[test]
fn failed_logout_preserves_an_unrelated_manual_provider() {
    let account = HttpFixture::new(|_| (503, "{}".into()));
    let installation = Installation::new(&account.origin());
    let home = &installation.home;
    settings::update(home, |global| {
        global
            .remote_access
            .insert("provider".into(), "manual".into());
        global.remote_access.insert(
            "web_public_url".into(),
            "https://manual.example.test".into(),
        );
        Ok(())
    })
    .expect("manual provider");
    let before = settings::load(home).expect("settings").remote_access;
    let request = DaemonRequest {
        v: 1,
        op: "membership_logout".into(),
        args: Map::new(),
    };
    super::super::logout(home, &request).expect_err("remote retirement temporarily fails");
    assert_eq!(
        settings::load(home).expect("settings").remote_access,
        before
    );
}

#[test]
fn in_flight_response_cannot_resurrect_off_logout_or_replaced_binding() {
    for change in [
        "off", "logout", "device", "provider", "cancel", "shutdown", "port",
    ] {
        let installation = Installation::new("http://127.0.0.1:1");
        let home = installation.home.clone();
        let changed_home = home.clone();
        let locks = DispatchLocks::default();
        let changed_locks = locks.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        let changed_port = Arc::new(AtomicBool::new(false));
        let port = changed_port.clone();
        let account = HttpFixture::new(move |_| {
            let _permit = changed_locks
                .try_global_write()
                .expect("account I/O must not hold the dispatcher");
            match change {
                "off" => {
                    settings::update(&changed_home, |global| {
                        global.remote_access.insert("enabled".into(), false.into());
                        Ok(())
                    })
                    .expect("off");
                }
                "logout" => membership::clear(&changed_home).expect("logout"),
                "device" => membership::update(&changed_home, |state| {
                    state.device_token = Some("new-device-fixture".into());
                    Ok(())
                })
                .expect("relink"),
                "provider" => {
                    settings::update(&changed_home, |global| {
                        global
                            .remote_access
                            .insert("provider".into(), "manual".into());
                        Ok(())
                    })
                    .expect("provider");
                }
                "cancel" => cancel.store(true, Ordering::Release),
                "shutdown" => crate::runtime_start_gate::prevent(&changed_home).expect("shutdown"),
                "port" => port.store(true, Ordering::Release),
                _ => unreachable!(),
            }
            (200, CREDENTIALS.into())
        });
        membership::update(&home, |state| {
            state.account_origin = Some(account.origin());
            Ok(())
        })
        .expect("fixture issuer");
        let result = restore_once(
            &home,
            &locks,
            &cancelled,
            |_| {
                Ok(if changed_port.load(Ordering::Acquire) {
                    9124
                } else {
                    9123
                })
            },
            |_| Ok(()),
            |_, _| panic!("late credentials cannot start after {change}"),
        );
        assert_eq!(result.is_err(), matches!(change, "shutdown" | "port"));
        assert!(
            membership::load(&home)
                .expect("state")
                .tunnel_token
                .is_none()
        );
    }
}

#[tokio::test]
async fn background_restore_never_queues_a_writer_behind_actor_startup() {
    let installation = Installation::new("http://127.0.0.1:1");
    let locks = DispatchLocks::default();
    let _startup = locks.global_read().await;
    let worker_locks = locks.clone();
    let home = installation.home.clone();
    tokio::task::spawn_blocking(move || {
        restore_once(
            &home,
            &worker_locks,
            &AtomicBool::new(false),
            |_| panic!("busy dispatcher must skip"),
            |_| Ok(()),
            |_, _| panic!("start"),
        )
    })
    .await
    .expect("worker")
    .expect("skip busy startup");
    tokio::time::timeout(Duration::from_millis(100), locks.global_read())
        .await
        .expect("nested discovery read must remain available");
}

#[cfg(unix)]
fn restore_with_fixture_helper(
    home: &HomeLayout,
    locks: &DispatchLocks,
    cancelled: &AtomicBool,
) -> Result<(), OpError> {
    restore_once(
        home,
        locks,
        cancelled,
        web_runtime::live_web_port,
        |_| Ok(()),
        |home, _| membership_cloudflared::start_restore_fixture(home),
    )
}

#[cfg(unix)]
#[tokio::test]
async fn service_automatically_retries_after_web_starts_late() {
    let account = HttpFixture::new(|_| (200, CREDENTIALS.into()));
    let installation = Installation::new(&account.origin());
    let home = &installation.home;
    let service = ReachRestore::start_with(
        home.clone(),
        DispatchLocks::default(),
        restore_with_fixture_helper,
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while membership::load(home).expect("state").last_error.is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("first attempt waits for Web");
    assert!(account.requests.lock().expect("requests").is_empty());
    let _web = HttpFixture::web(home);
    tokio::time::timeout(Duration::from_secs(8), async {
        while !membership_cloudflared::status(home).running {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("daemon retries without any status request or manual start");
    drop(service);
    assert_eq!(account.requests.lock().expect("requests").len(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn service_shutdown_during_account_io_cannot_launch_a_late_helper() {
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered));
    let (release, held) = std::sync::mpsc::channel();
    let account = HttpFixture::new(move |_| {
        entered
            .lock()
            .expect("sender")
            .take()
            .expect("one request")
            .send(())
            .expect("notify");
        held.recv_timeout(Duration::from_secs(2))
            .expect("release account request");
        (200, CREDENTIALS.into())
    });
    let installation = Installation::new(&account.origin());
    let _web = HttpFixture::web(&installation.home);
    let service = ReachRestore::start_with(
        installation.home.clone(),
        DispatchLocks::default(),
        restore_with_fixture_helper,
    );
    tokio::time::timeout(Duration::from_secs(2), waiting)
        .await
        .expect("request arrives")
        .expect("entered");
    let cancellation = service.cancelled.clone();
    drop(service);
    release.send(()).expect("release");
    tokio::time::timeout(Duration::from_secs(2), async {
        while Arc::strong_count(&cancellation) != 1 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cancelled blocking worker finishes");
    assert!(!membership_cloudflared::status(&installation.home).running);
    assert!(
        membership::load(&installation.home)
            .expect("state")
            .tunnel_token
            .is_none()
    );
}
