#![cfg(unix)]

use cccc_core::{HomeLayout, access_tokens::AccessTokenStore};
use serde_json::json;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn cli_override_is_replaced_by_saved_binding_when_apply_is_requested() {
    const ACCESS_TOKEN: &str = "acc_web_restart_test";

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    AccessTokenStore::new(home.clone())
        .expect("token store")
        .create("admin", Vec::new(), true, Some(ACCESS_TOKEN))
        .expect("admin token");
    let live_port = free_port().await;
    let desired_port = free_port().await;

    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc"));
    command
        .arg("--port")
        .arg(live_port.to_string())
        .env("CCCC_HOME", home.root())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().expect("CCCC CLI");
    wait_for_port(live_port).await;

    let configured = request(
        live_port,
        "PUT",
        "/api/v1/remote_access",
        &json!({
            "web_host":"127.0.0.1",
            "web_port":desired_port,
            "by":"user"
        })
        .to_string(),
        ACCESS_TOKEN,
    )
    .await;
    assert!(configured.starts_with("HTTP/1.1 200"), "{configured}");

    let applied = request(
        live_port,
        "POST",
        "/api/v1/remote_access/apply",
        "",
        ACCESS_TOKEN,
    )
    .await;
    assert!(applied.starts_with("HTTP/1.1 200"), "{applied}");
    assert!(applied.contains(r#""accepted":true"#), "{applied}");

    wait_for_port(desired_port).await;
    wait_for_port_to_close(live_port).await;
    assert!(child.try_wait().expect("CLI status").is_none());

    let state = request(
        desired_port,
        "GET",
        "/api/v1/remote_access",
        "",
        ACCESS_TOKEN,
    )
    .await;
    assert!(state.starts_with("HTTP/1.1 200"), "{state}");
    assert!(state.contains(r#""apply_supported":true"#), "{state}");
    assert!(state.contains(r#""restart_required":false"#), "{state}");
    assert!(state.contains(&format!(r#""live_runtime_port":{desired_port}"#)));

    child.start_kill().expect("stop CLI");
    child.wait().await.expect("wait for CLI");
}

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind port");
    listener.local_addr().expect("address").port()
}

async fn wait_for_port(port: u16) {
    for _ in 0..200 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("Web did not listen on port {port}");
}

async fn wait_for_port_to_close(port: u16) {
    for _ in 0..200 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err()
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("old Web listener on port {port} stayed open");
}

async fn request(port: u16, method: &str, path: &str, body: &str, token: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect Web");
    stream
        .write_all(
            format!(
                "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .expect("write request");
    let mut response = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        stream.read_to_end(&mut response),
    )
    .await
    .expect("response timeout")
    .expect("read response");
    String::from_utf8(response).expect("UTF-8 response")
}

// This exercises the actual CLI shutdown path. Provider traffic stays on
// loopback and the Analyst is a fixture; no user runtime or credentials are used.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn graceful_web_exit_waits_for_the_active_voice_analyst() {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;
    use tokio::net::{TcpListener, TcpStream};

    const TOKEN: &str = "acc_voice_shutdown_fixture";
    let temp = tempfile::Builder::new()
        .prefix("cccc-voice-shutdown-test-")
        .tempdir()
        .expect("isolated fixture");
    for directory in ["home", "codex", "config", "cache", "data", "tmp"] {
        std::fs::create_dir_all(temp.path().join(directory)).expect("fixture directory");
    }
    let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
    home.initialize().expect("initialize");
    AccessTokenStore::new(home.clone())
        .expect("token store")
        .create("fixture admin", Vec::new(), true, Some(TOKEN))
        .expect("fixture token");
    std::fs::write(
        temp.path().join("codex/auth.json"),
        r#"{"tokens":{"access_token":"fixture","account_id":"fixture"}}"#,
    )
    .expect("fixture credentials");

    let source =
        include_str!("../../cccc-daemon/src/ops/codex_voice_analyst/tests/native_acp_fixture.py");
    let marker = "session_id = \"\"";
    assert!(source.contains(marker));
    let injection = r#"
import signal, time
Path(os.environ["HOME"]).mkdir(parents=True, exist_ok=True)
def slow_stop(signum, frame):
    Path(os.environ["HOME"]).joinpath("analyst-received-term").write_text("term")
    time.sleep(6)
signal.signal(signal.SIGTERM, slow_stop)
"#;
    let fake = temp.path().join("devin");
    std::fs::write(
        &fake,
        format!(
            "{}\nimport time\ntime.sleep(3)\n",
            source.replacen(marker, &format!("{injection}\n{marker}"), 1)
        ),
    )
    .expect("fake ACP runtime");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700))
        .expect("fake runtime permissions");

    let provider = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback provider");
    let provider_address = provider.local_addr().expect("provider address");
    let provider_task = tokio::spawn(async move {
        let (mut stream, _) = provider.accept().await.expect("fixture provider request");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 8192];
        let header_end = loop {
            let read = stream
                .read(&mut buffer)
                .await
                .expect("provider request read");
            assert!(read != 0, "provider request ended before headers");
            request.extend_from_slice(&buffer[..read]);
            if let Some(index) = request.windows(4).position(|chunk| chunk == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = std::str::from_utf8(&request[..header_end]).expect("provider headers");
        let length = headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .expect("provider request content length");
        while request.len() - header_end < length {
            let read = stream.read(&mut buffer).await.expect("provider body read");
            assert!(read != 0, "provider request ended before body");
            request.extend_from_slice(&buffer[..read]);
        }
        stream
            .write_all(b"HTTP/1.1 201 Created\r\nContent-Type: application/sdp\r\nContent-Length: 14\r\nConnection: close\r\n\r\nfixture-answer")
            .await
            .expect("fixture response");
    });

    let isolated = |command: &mut tokio::process::Command| {
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .env("HOME", temp.path().join("home"))
            .env("CCCC_HOME", home.root())
            .env("CODEX_HOME", temp.path().join("codex"))
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .env("XDG_CACHE_HOME", temp.path().join("cache"))
            .env("XDG_DATA_HOME", temp.path().join("data"))
            .env("TMPDIR", temp.path().join("tmp"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("CCCC_LAUNCHER_PATH", env!("CARGO_BIN_EXE_cccc"))
            .env(
                "CCCC_CODEX_VOICE_BASE_URL",
                format!("http://{provider_address}"),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
    };
    let mut daemon_command = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc"));
    daemon_command.args(["daemon", "run"]);
    isolated(&mut daemon_command);
    let mut daemon = daemon_command.spawn().expect("isolated daemon");
    tokio::time::timeout(Duration::from_secs(10), async {
        while !home.daemon_dir().join("ccccd.sock").exists() {
            assert!(daemon.try_wait().expect("daemon status").is_none());
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("daemon startup");

    let mut web_command = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc"));
    web_command.args(["--host", "127.0.0.1", "--port", "0", "web"]);
    isolated(&mut web_command);
    let mut web = web_command.spawn().expect("isolated Web");
    let port = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(raw) = std::fs::read(home.daemon_dir().join("web_runtime.json"))
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&raw)
                && let Some(port) = value["port"]
                    .as_u64()
                    .and_then(|port| u16::try_from(port).ok())
                && port != 0
            {
                break port;
            }
            assert!(web.try_wait().expect("Web status").is_none());
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("Web binding");
    wait_for_port(port).await;
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("local HTTP client");
    let settings = client
        .put(format!("{base}/api/v1/codex_voice/analyst-settings"))
        .bearer_auth(TOKEN)
        .json(&json!({"settings":{"runtime":"devin","runtime_mode":"acp","command":[fake]}}))
        .send()
        .await
        .expect("save fake Analyst settings");
    assert!(settings.status().is_success(), "{settings:?}");
    let response: serde_json::Value = client
        .post(format!("{base}/api/v1/codex_voice/calls"))
        .bearer_auth(TOKEN)
        .json(&json!({"client_session_id":"fixture-session","offer_sdp":"v=0\r\n"}))
        .send()
        .await
        .expect("start fake Voice")
        .json()
        .await
        .expect("Voice response");
    assert_eq!(response["ok"], true, "{response}");
    let generation = response["result"]["call"]["generation"]
        .as_str()
        .expect("call generation");
    let mut control = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("control socket");
    control
        .write_all(
            format!(
                "GET /api/v1/codex_voice/calls/{generation}/events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {TOKEN}\r\nOrigin: {base}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .expect("control upgrade");
    let mut headers = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !headers.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
            let mut buffer = [0_u8; 1024];
            let read = control.read(&mut buffer).await.expect("control response");
            assert!(read != 0, "control socket closed before upgrade");
            headers.extend_from_slice(&buffer[..read]);
        }
    })
    .await
    .expect("control upgrade timeout");
    assert!(headers.starts_with(b"HTTP/1.1 101"));
    let active: serde_json::Value = client
        .get(format!("{base}/api/v1/codex_voice/calls/active"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .expect("active call")
        .json()
        .await
        .expect("active call response");
    assert_eq!(active["result"]["call"]["connected"], true, "{active}");

    let pid = web.id().expect("Web PID");
    kill(Pid::from_raw(pid as i32), Signal::SIGTERM).expect("stop fixture Web");
    let status = tokio::time::timeout(Duration::from_secs(8), web.wait())
        .await
        .expect("Web shutdown deadline")
        .expect("Web exit");
    assert!(status.success(), "Web exited {status}");
    assert!(
        temp.path().join("home/analyst-received-term").exists(),
        "graceful Web shutdown must request Analyst termination before forcing it"
    );
    drop(control);
    provider_task.await.expect("fixture provider completed");
    daemon.start_kill().expect("stop isolated daemon");
    daemon.wait().await.expect("reap isolated daemon");
}
