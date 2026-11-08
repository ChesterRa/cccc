#![cfg(unix)]

use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::{HomeLayout, access_tokens::AccessTokenStore, settings};
use serde_json::{Value, json};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "acc_disposable_loopback_proxy_test";

#[derive(Debug)]
struct Observed {
    request_line: String,
    fixture_admin: bool,
}

struct Fixture {
    port: u16,
    observed: Arc<Mutex<Vec<Observed>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture(redirect: Option<String>) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture listener");
    let port = listener.local_addr().expect("fixture address").port();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let recorded = observed.clone();
    let task = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let recorded = recorded.clone();
            let redirect = redirect.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head_end, length) = loop {
                    let read = stream.read(&mut chunk).await.expect("fixture request");
                    assert!(read > 0 && bytes.len() + read <= 64 * 1024);
                    bytes.extend_from_slice(&chunk[..read]);
                    if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let head_end = index + 4;
                        let head = String::from_utf8_lossy(&bytes[..head_end]);
                        let length = head
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().expect("length"))
                            })
                            .unwrap_or(0);
                        break (head_end, length);
                    }
                };
                while bytes.len() < head_end + length {
                    let read = stream.read(&mut chunk).await.expect("fixture body");
                    assert!(read > 0 && bytes.len() + read <= 64 * 1024);
                    bytes.extend_from_slice(&chunk[..read]);
                }
                let head = String::from_utf8_lossy(&bytes[..head_end]);
                let fixture_admin = head.lines().any(|line| {
                    line.split_once(':').is_some_and(|(name, value)| {
                        name.eq_ignore_ascii_case("authorization")
                            && value.trim() == format!("Bearer {TOKEN}")
                    })
                });
                recorded.lock().expect("observations").push(Observed {
                    request_line: head.lines().next().unwrap_or_default().to_owned(),
                    fixture_admin,
                });
                let body = r#"{"ok":true,"result":{"running":false}}"#;
                let status = if redirect.is_some() {
                    "307 Temporary Redirect"
                } else {
                    "200 OK"
                };
                let location =
                    redirect.map_or_else(String::new, |url| format!("Location: {url}\r\n"));
                let response = format!(
                    "HTTP/1.1 {status}\r\n{location}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("fixture response");
            });
        }
    });
    Fixture {
        port,
        observed,
        task,
    }
}

fn configure_child(command: &mut tokio::process::Command, home: &HomeLayout, proxy_port: u16) {
    let proxy = format!("http://127.0.0.1:{proxy_port}");
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.root())
        .env("CCCC_HOME", home.root())
        .env("CODEX_HOME", home.root().join("codex"))
        .env("HTTP_PROXY", &proxy)
        .env("http_proxy", &proxy)
        .env("HTTPS_PROXY", &proxy)
        .env("https_proxy", &proxy)
        .env("ALL_PROXY", &proxy)
        .env("all_proxy", &proxy)
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
}

async fn call(client: &DaemonClient, op: &str, args: Value) -> Value {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().expect("arguments").clone(),
        })
        .await
        .expect("daemon response");
    assert!(response.ok, "{op}: {:?}", response.error);
    Value::Object(response.result)
}

#[tokio::test]
async fn daemon_and_cli_loopback_web_requests_do_not_use_system_proxy() {
    let web = fixture(None).await;
    let proxy = fixture(None).await;
    let temp = tempfile::tempdir().expect("temporary home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let mut global = settings::load(&home).expect("settings");
    global
        .remote_access
        .insert("web_host".into(), json!("0.0.0.0"));
    global
        .remote_access
        .insert("web_port".into(), json!(web.port));
    settings::save(&home, &global).expect("save settings");
    AccessTokenStore::new(home.clone())
        .expect("store")
        .create("fixture", Vec::new(), true, Some(TOKEN))
        .expect("fixture token");

    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc"));
    configure_child(&mut command, &home, proxy.port);
    command
        .args(["daemon", "run", "--host", "127.0.0.1", "--port", "0"])
        .stdout(Stdio::null())
        .kill_on_drop(true);
    let mut daemon = command.spawn().expect("start isolated daemon");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !home.daemon_dir().join("ccccd.addr.json").exists() {
        assert!(
            daemon.try_wait().expect("daemon status").is_none(),
            "daemon exited"
        );
        assert!(Instant::now() < deadline, "daemon did not start");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let client = DaemonClient::new(home.clone())
        .without_voice_origin()
        .with_timeout(Duration::from_secs(20));
    let group = call(
        &client,
        "group_create",
        json!({"title":"proxy fixture","by":"user"}),
    )
    .await;
    let group = group["group_id"]
        .as_str()
        .or_else(|| group["group"]["group_id"].as_str())
        .expect("group ID");
    call(
        &client,
        "im_set",
        json!({"group_id":group,"platform":"telegram","token_env":"FIXTURE_TOKEN","by":"user"}),
    )
    .await;
    call(&client, "im_start", json!({"group_id":group,"by":"user"})).await;

    let mut cli = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc"));
    configure_child(&mut cli, &home, proxy.port);
    cli.args([
        "im",
        "--host",
        "127.0.0.1",
        "--port",
        &web.port.to_string(),
        "status",
        "--group",
        group,
    ])
    .stdout(Stdio::null());
    let status = tokio::time::timeout(Duration::from_secs(20), cli.status())
        .await
        .expect("CLI timeout")
        .expect("CLI status");
    assert!(status.success(), "fixture CLI failed");
    {
        let observations = web.observed.lock().expect("observations");
        assert_eq!(observations.len(), 2);
        assert!(
            observations[0]
                .request_line
                .starts_with("POST /api/im/start ")
        );
        assert!(
            observations[1]
                .request_line
                .starts_with("GET /api/im/status?")
        );
        assert!(observations.iter().all(|request| request.fixture_admin));
    }
    assert!(
        proxy
            .observed
            .lock()
            .expect("proxy observations")
            .is_empty()
    );

    // Web management requests must not follow a 307 to a different service.
    let redirect_target = fixture(None).await;
    let redirecting = fixture(Some(format!(
        "http://127.0.0.1:{}/redirect-target",
        redirect_target.port
    )))
    .await;
    global
        .remote_access
        .insert("web_port".into(), json!(redirecting.port));
    settings::save(&home, &global).expect("save redirected fixture settings");
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: "im_start".into(),
            args: json!({"group_id":group,"by":"user"})
                .as_object()
                .expect("args")
                .clone(),
        })
        .await
        .expect("daemon response");
    assert!(!response.ok, "daemon followed a Web management redirect");
    let mut cli = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc"));
    configure_child(&mut cli, &home, proxy.port);
    cli.args([
        "im",
        "--host",
        "127.0.0.1",
        "--port",
        &redirecting.port.to_string(),
        "status",
        "--group",
        group,
    ])
    .stdout(Stdio::null());
    let status = tokio::time::timeout(Duration::from_secs(20), cli.status())
        .await
        .expect("CLI timeout")
        .expect("CLI status");
    assert!(!status.success(), "CLI followed a Web management redirect");
    assert_eq!(redirecting.observed.lock().expect("redirects").len(), 2);
    assert!(
        redirect_target
            .observed
            .lock()
            .expect("redirect target")
            .is_empty()
    );
    assert!(
        proxy
            .observed
            .lock()
            .expect("proxy observations")
            .is_empty()
    );

    daemon.start_kill().expect("stop isolated daemon");
    daemon.wait().await.expect("wait for daemon");
}
