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
                Ok(())
            })
            .expect("fixture operation");
        }
        let payload = if reject {
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
    assert!(status(&home, &request).is_ok());
    assert_eq!(
        first.checked_at,
        connect::load(&home)
            .expect("fixture operation")
            .expect("fixture operation")
            .checked_at
    );
}

#[test]
fn retired_or_changed_bindings_do_not_receive_a_cached_grant() {
    for (reject, change_binding) in [(true, false), (false, true)] {
        let (_temp, home) = fixture();
        let (origin, rx, server) = server(home.clone(), reject, change_binding);
        bind(&home, &origin);
        let requested = intent(&home)
            .expect("fixture operation")
            .expect("fixture operation");
        let result = refresh(&home, &requested, &AtomicBool::new(false));
        rx.recv().expect("fixture operation");
        server.join().expect("fixture operation");
        assert_eq!(result.is_err(), reject);
        assert!(
            connect::load(&home)
                .expect("fixture operation")
                .and_then(|snapshot| snapshot.directory)
                .is_none()
        );
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
