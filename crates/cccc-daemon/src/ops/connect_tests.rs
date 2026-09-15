use super::*;
use cccc_contracts::connect::{ConnectDirectory, ConnectInstance};
use std::{
    io::{Read as _, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
};

fn fixture() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("fixture operation");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("fixture operation");
    (temp, home)
}

fn server(
    home: HomeLayout,
    reject: bool,
    change_binding: bool,
) -> (
    String,
    mpsc::Receiver<serde_json::Value>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fixture operation");
    let origin = format!(
        "http://{}",
        listener.local_addr().expect("fixture operation")
    );
    let (tx, rx) = mpsc::channel();
    let task = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("fixture operation");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("fixture operation");
        let mut bytes = Vec::new();
        let split = loop {
            let mut chunk = [0; 2048];
            let count = stream.read(&mut chunk).expect("fixture operation");
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(split) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break split + 4;
            }
        };
        let headers = String::from_utf8(bytes[..split].to_vec())
            .expect("fixture operation")
            .to_lowercase();
        assert!(headers.starts_with("post /v1/connect/instances "));
        assert!(headers.contains(&format!(
            "cccc-client-version: {}",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(headers.contains("authorization: bearer fixture-device-token"));
        let length: usize = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .expect("fixture operation")
            .trim()
            .parse()
            .expect("fixture operation");
        while bytes.len() - split < length {
            let mut chunk = [0; 2048];
            let count = stream.read(&mut chunk).expect("fixture operation");
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
        }
        let body: serde_json::Value =
            serde_json::from_slice(&bytes[split..split + length]).expect("fixture operation");
        let registration: ConnectRegistration =
            serde_json::from_value(body.clone()).expect("fixture operation");
        tx.send(body).expect("fixture operation");
        if change_binding {
            membership::update(&home, |state| {
                state.device_id = Some("new-binding".into());
                state.account_label = Some("replacement@example.test".into());
                Ok(())
            })
            .expect("fixture operation");
        }
        let mut payload = if reject {
            json!({"error":{"code":"device_disabled","message":"Device retired"}})
        } else {
            serde_json::to_value(ConnectDirectory {
                protocol_version: 1,
                account_id: "owner".into(),
                device_id: "device-a".into(),
                issued_at: Utc::now().to_rfc3339(),
                expires_at: (Utc::now() + chrono::Duration::seconds(119)).to_rfc3339(),
                instances: vec![ConnectInstance {
                    instance_id: registration.instance_id,
                    public_key: registration.public_key,
                    client_version: registration.client_version,
                    public_origin: registration.public_origin,
                    display_name: "A".into(),
                    device_id: "device-a".into(),
                    registered_at: Utc::now().to_rfc3339(),
                }],
            })
            .expect("fixture operation")
        };
        if !reject {
            payload["account_label"] = json!("owner@example.test");
        }
        let body = serde_json::to_string(&payload).expect("fixture operation");
        let status = if reject { "403 Forbidden" } else { "200 OK" };
        write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("fixture operation");
    });
    (origin, rx, task)
}

fn bind(home: &HomeLayout, origin: &str) {
    membership::save(
        home,
        &membership::MembershipState {
            logged_in: true,
            account_origin: Some(origin.into()),
            device_id: Some("device-a".into()),
            device_token: Some("fixture-device-token".into()),
            hostname: Some("https://d-fixture.cccc.foo".into()),
            ..Default::default()
        },
    )
    .expect("fixture operation");
}

#[test]
fn refresh_registers_identity_without_web_tokens_and_status_is_read_only() {
    let (_temp, home) = fixture();
    let (origin, rx, server) = server(home.clone(), false, false);
    bind(&home, &origin);
    let requested = intent(&home)
        .expect("fixture operation")
        .expect("fixture operation");
    assert!(requested.public_origin.is_none());
    refresh(&home, &requested, &AtomicBool::new(false)).expect("fixture operation");
    let request = rx.recv().expect("fixture operation");
    server.join().expect("fixture operation");
    assert!(request["public_origin"].is_null());
    let first = connect::load(&home)
        .expect("fixture operation")
        .expect("fixture operation");
    assert!(first.directory.is_some());
    // The account listener has gone away. Reads must still work without any network.
    let request = DaemonRequest {
        v: 1,
        op: "connect_status".into(),
        args: Default::default(),
    };
    assert!(matches!(
        resolve_operation(&request)
            .expect("fixture operation")
            .policy,
        Read
    ));
    assert_eq!(
        status(&home, &request).expect("status")["account_label"],
        "owner@example.test"
    );
    let saved = membership::load(&home).expect("member");
    assert_eq!(saved.account_label.as_deref(), Some("owner@example.test"));
    membership::update(&home, |member| {
        member.disabled = true;
        Ok(())
    })
    .expect("cut");
    assert!(status(&home, &request).expect("cut status")["account_label"].is_null());
    membership::save(&home, &saved).expect("restore fixture");
    assert_eq!(
        first.checked_at,
        connect::load(&home)
            .expect("fixture operation")
            .expect("fixture operation")
            .checked_at
    );
    assert!(refresh(&home, &requested, &AtomicBool::new(false)).is_err());
    assert_eq!(
        status(&home, &request).expect("offline status")["account_label"],
        "owner@example.test",
        "a transport failure does not reject the account identity"
    );
}

#[test]
fn retired_or_changed_bindings_do_not_receive_a_cached_grant() {
    for (reject, change_binding) in [(true, false), (false, true), (true, true)] {
        let (_temp, home) = fixture();
        let (origin, rx, server) = server(home.clone(), reject, change_binding);
        bind(&home, &origin);
        membership::update(&home, |member| {
            member.account_label = Some("previous@example.test".into());
            Ok(())
        })
        .expect("seed cached identity");
        let requested = intent(&home)
            .expect("fixture operation")
            .expect("fixture operation");
        let result = refresh(&home, &requested, &AtomicBool::new(false));
        rx.recv().expect("fixture operation");
        server.join().expect("fixture operation");
        assert_eq!(result.is_err(), reject && !change_binding);
        assert!(
            connect::load(&home)
                .expect("fixture operation")
                .and_then(|snapshot| snapshot.directory)
                .is_none()
        );
        let request = DaemonRequest {
            v: 1,
            op: "connect_status".into(),
            args: Default::default(),
        };
        let label =
            status(&home, &request).expect("status after account response")["account_label"]
                .clone();
        if change_binding {
            assert_eq!(label, "replacement@example.test");
        } else {
            assert!(
                label.is_null(),
                "a rejected device must not expose cached identity"
            );
            assert!(
                membership::load(&home)
                    .expect("membership")
                    .account_label
                    .is_none()
            );
        }
    }
}

#[test]
fn remote_access_intent_is_preserved_and_reach_origin_is_not_double_prefixed() {
    let (_temp, home) = fixture();
    bind(&home, "https://account.test");
    let mut configured = settings::load(&home).expect("fixture operation");
    configured.remote_access = json!({"provider":"reach", "enabled":true})
        .as_object()
        .expect("fixture operation")
        .clone();
    settings::save(&home, &configured).expect("fixture operation");
    assert_eq!(
        intent(&home)
            .expect("fixture operation")
            .expect("fixture operation")
            .public_origin
            .as_deref(),
        Some("https://d-fixture.cccc.foo")
    );
    configured.remote_access =
        json!({"provider":"manual", "enabled":true, "web_public_url":"https://custom.test/"})
            .as_object()
            .expect("fixture operation")
            .clone();
    settings::save(&home, &configured).expect("fixture operation");
    assert_eq!(
        intent(&home)
            .expect("fixture operation")
            .expect("fixture operation")
            .public_origin
            .as_deref(),
        Some("https://custom.test")
    );
    configured
        .remote_access
        .insert("enabled".into(), json!(false));
    settings::save(&home, &configured).expect("fixture operation");
    assert!(
        intent(&home)
            .expect("fixture operation")
            .expect("fixture operation")
            .public_origin
            .is_none()
    );
    assert_eq!(
        settings::load(&home)
            .expect("fixture operation")
            .remote_access,
        configured.remote_access
    );
}

#[test]
fn sharing_sync_failure_recovery_expiry_and_link_state_stay_distinct() {
    use axum::{
        Json, Router,
        extract::State,
        routing::{get, post},
    };
    use std::sync::atomic::{AtomicU16, AtomicUsize};
    #[derive(Clone)]
    struct Issuer {
        origin: String,
        code: Arc<AtomicU16>,
        reads: Arc<AtomicUsize>,
        links: Arc<std::sync::Mutex<Vec<cccc_contracts::connect_groups::ConnectGroupLink>>>,
        invalidated: Arc<std::sync::Mutex<Vec<String>>>,
    }
    let (_temp, home) = fixture();
    home.initialize().expect("initialize isolated Home");
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    listener.set_nonblocking(true).expect("nonblocking");
    let origin = format!("http://{}", listener.local_addr().expect("address"));
    let issuer = Issuer {
        origin: origin.clone(),
        code: Arc::new(AtomicU16::new(503)),
        reads: Arc::new(AtomicUsize::new(0)),
        links: Arc::default(),
        invalidated: Arc::default(),
    };
    async fn group_response(
        s: Issuer,
        invalidated: Vec<String>,
    ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
        s.reads.fetch_add(1, Ordering::Relaxed);
        let mut links = s.links.lock().expect("fixture links");
        links.retain(|link| !invalidated.contains(&link.id));
        *s.invalidated.lock().expect("fixture invalidations") = invalidated;
        let code = s.code.load(Ordering::Relaxed);
        let body = if code == 200 {
            json!({"protocol_version":1,"account_origin":s.origin,"account_id":"owner","device_id":"device-a",
            "issued_at":Utc::now().to_rfc3339(),"expires_at":(Utc::now()+chrono::Duration::seconds(119)).to_rfc3339(),"links":*links})
        } else {
            json!({"error":{"code":"unavailable","message":"Group service unavailable"}})
        };
        (
            axum::http::StatusCode::from_u16(code).expect("status"),
            Json(body),
        )
    }
    let app = Router::new().route("/v1/connect/instances", post(|Json(r): Json<ConnectRegistration>| async move {
        Json(json!({"protocol_version":1,"account_id":"owner","device_id":"device-a","account_label":"owner@example.test",
            "issued_at":Utc::now().to_rfc3339(),"expires_at":(Utc::now()+chrono::Duration::seconds(119)).to_rfc3339(),
            "instances":[{"instance_id":r.instance_id,"public_key":r.public_key,"client_version":r.client_version,
                "public_origin":r.public_origin,"device_id":"device-a","display_name":"A","registered_at":Utc::now().to_rfc3339()}]}))
    })).route("/v1/connect/groups", get(|State(s): State<Issuer>| async move {group_response(s,vec![]).await})
        .post(|State(s): State<Issuer>,Json(body): Json<serde_json::Value>| async move {
            group_response(s,serde_json::from_value(body["invalidated"].clone()).expect("invalidated IDs")).await
        })).with_state(issuer.clone());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(async move {
                axum::serve(
                    tokio::net::TcpListener::from_std(listener).expect("listener"),
                    app,
                )
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .expect("server");
            });
    });
    bind(&home, &origin);
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .create("Local", "")
        .expect("group");
    let request = DaemonRequest {
        v: 1,
        op: "connect_group_status".into(),
        args: json!({"group_id":group.group_id})
            .as_object()
            .expect("args")
            .clone(),
    };
    let inspect = || group_status(&home, &request).expect("status");
    assert_eq!(inspect()["status"], "syncing");
    let requested = intent(&home).expect("intent").expect("linked");
    for code in [503, 200, 503, 200, 404, 403, 401, 200] {
        issuer.code.store(code, Ordering::Relaxed);
        refresh(&home, &requested, &AtomicBool::new(false))
            .expect("same-account directory remains usable");
        let snapshot = connect::load(&home).expect("load").expect("linked");
        assert!(snapshot.directory.is_some());
        assert!(snapshot.error_code.is_none());
        assert_eq!(
            membership::load(&home)
                .expect("membership")
                .account_label
                .as_deref(),
            if matches!(code, 401 | 403) {
                None
            } else {
                Some("owner@example.test")
            },
            "only a device rejection clears the display identity"
        );
        let before = issuer.reads.load(Ordering::Relaxed);
        let status = inspect();
        assert_eq!(
            status["status"],
            if code == 200 { "ready" } else { "unavailable" }
        );
        assert_eq!(status["error_code"].is_null(), code == 200);
        if code == 404 {
            assert_eq!(status["error_code"], "connect_groups_unsupported");
        }
        assert_eq!(
            issuer.reads.load(Ordering::Relaxed),
            before,
            "GET is read-only"
        );
    }
    issuer.code.store(200, Ordering::Relaxed);
    refresh(&home, &requested, &AtomicBool::new(false)).expect("recover");
    // Actual refresh HTTP requests must retire only deleted/replaced resources.
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let deleted = store
        .create("Delete during refresh", "")
        .expect("deleted Group");
    let healthy = store.create("Unaffected Group", "").expect("healthy Group");
    let snapshot = connect::load(&home).expect("snapshot").expect("linked");
    let own = snapshot.directory.as_ref().expect("directory").instances[0].clone();
    let remote_home = HomeLayout::from_path(_temp.path().join("remote")).expect("remote Home");
    remote_home.initialize().expect("remote fixture");
    let remote = InstanceIdentity::load_or_create(&remote_home).expect("remote identity");
    let links = [&group, &deleted, &healthy]
        .iter()
        .map(|g| cccc_contracts::connect_groups::ConnectGroupLink {
            id: uuid::Uuid::new_v4().to_string(),
            source: cccc_contracts::connect_groups::ConnectGroupEndpoint {
                account_id: "owner".into(),
                instance: own.clone(),
                group_id: g.group_id.clone(),
                group_generation: cccc_core::connect_groups::generation(g),
                title: g.title.clone(),
            },
            target: cccc_contracts::connect_groups::ConnectGroupEndpoint {
                account_id: "external-owner".into(),
                instance: ConnectInstance {
                    instance_id: remote.peer_id.clone(),
                    public_key: remote.public_key_b64.clone(),
                    device_id: "external-device".into(),
                    ..own.clone()
                },
                group_id: "g_external".into(),
                group_generation: "external-generation".into(),
                title: "External".into(),
            },
        })
        .collect::<Vec<_>>();
    *issuer.links.lock().expect("fixture links") = links.clone();
    refresh(&home, &requested, &AtomicBool::new(false)).expect("seed grants");
    let group_path = store
        .group_dir(&group.group_id)
        .expect("directory")
        .join("group.yaml");
    let original = std::fs::read(&group_path).expect("fixture YAML");
    std::fs::write(&group_path, "v: [invalid YAML").expect("temporary read error");
    store.delete(&deleted.group_id).expect("delete Group");
    refresh(&home, &requested, &AtomicBool::new(false)).expect("healthy directory renews");
    assert_eq!(
        *issuer.invalidated.lock().expect("reported"),
        vec![links[1].id.clone()]
    );
    let grant = cccc_core::connect_groups::load(&home)
        .expect("grant")
        .expect("fresh");
    assert_eq!(
        grant.links.iter().map(|l| &l.id).collect::<Vec<_>>(),
        vec![&links[0].id, &links[2].id]
    );
    assert!(
        group_connection_summary(&home).expect("partially unreadable")["counts"][&group.group_id]
            .is_null()
    );
    assert!(
        cccc_core::connect_peer::scoped_binding(&home, &remote.peer_id, Some(&links[0].id))
            .is_err()
    );
    cccc_core::connect_peer::scoped_binding(&home, &remote.peer_id, Some(&links[2].id))
        .expect("healthy Group remains authorized");
    assert_eq!(
        connect::load(&home)
            .expect("status")
            .expect("linked")
            .group_sync
            .expect("diagnostic")
            .error_code
            .as_deref(),
        Some("connect_group_resource_unavailable")
    );
    std::fs::write(&group_path, original).expect("restore same resource");
    refresh(&home, &requested, &AtomicBool::new(false)).expect("recover without new invitation");
    assert!(issuer.invalidated.lock().expect("reported").is_empty());
    assert_eq!(inspect()["status"], "ready");
    let counts = group_connection_summary(&home).expect("summary");
    assert_eq!(counts["counts"][&group.group_id], 1);
    assert_eq!(counts["counts"][&healthy.group_id], 1);
    assert!(counts["counts"].get(&deleted.group_id).is_none());
    cccc_core::connect_peer::scoped_binding(&home, &remote.peer_id, Some(&links[0].id))
        .expect("original link recovers");
    let path = home.root().join("secrets/connect_groups.json");
    let mut grant: serde_json::Value = cccc_core::fs::read_json(&path).expect("grant");
    grant["expires_at"] = json!("2000-01-01T00:00:00Z");
    cccc_core::fs::write_secret_json(&path, &grant).expect("expire fixture grant");
    assert_eq!(inspect()["status"], "unavailable");
    assert!(
        group_connection_summary(&home)
            .expect("expired summary")
            .is_null()
    );
    let mut snapshot = connect::load(&home).expect("snapshot").expect("linked");
    snapshot.directory.as_mut().expect("directory").expires_at = "2000-01-01T00:00:00Z".into();
    connect::save(&home, &snapshot).expect("expire directory");
    let status = inspect();
    assert_eq!(status["status"], "unavailable");
    assert!(
        status["account_id"].is_null(),
        "expired directory is not an unlinked account"
    );
    membership::update(&home, |state| {
        state.logged_in = false;
        Ok(())
    })
    .expect("unlink fixture");
    assert_eq!(inspect()["status"], "not_linked");
    let _ = stop.send(());
    server.join().expect("server stopped");
}
