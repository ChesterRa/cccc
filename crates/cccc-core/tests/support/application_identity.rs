//! Shared process check for the three executable entry points. Each package
//! compares its executable against the same canonical workspace fingerprint.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[path = "../../build_support/source.rs"]
mod source;

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct DaemonTask(tokio::task::JoinHandle<anyhow::Result<()>>);

impl Drop for DaemonTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn isolated_command(root: &Path) -> Command {
    let user = root.join("user");
    for leaf in ["AppData/Roaming", "AppData/Local"] {
        std::fs::create_dir_all(user.join(leaf)).expect("isolated user home");
    }
    for leaf in ["cccc", "codex", "claude", "tmp"] {
        std::fs::create_dir_all(root.join(leaf)).expect("isolated runtime home");
    }
    // A different checkout and environment must not replace the compiled id.
    std::fs::write(root.join("Cargo.toml"), "not the compiled workspace").expect("other workspace");
    let mut command = Command::new(super::BINARY);
    command.args(super::ARGUMENTS).env_clear().current_dir(root);
    for key in ["PATH", "SystemRoot", "WINDIR", "COMSPEC", "PATHEXT"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("HOME", &user)
        .env("USERPROFILE", &user)
        .env("APPDATA", user.join("AppData/Roaming"))
        .env("LOCALAPPDATA", user.join("AppData/Local"))
        .env("CCCC_HOME", root.join("cccc"))
        .env("CODEX_HOME", root.join("codex"))
        .env("CLAUDE_CONFIG_DIR", root.join("claude"))
        .env("TMPDIR", root.join("tmp"))
        .env("TMP", root.join("tmp"))
        .env("TEMP", root.join("tmp"))
        .env("CCCC_SOURCE_ID", "not-the-compiled-id")
        .stderr(Stdio::null());
    command
}

fn mcp_identity(root: &Path) -> String {
    let mut process = Process(
        isolated_command(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start MCP executable"),
    );
    let request = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2024-11-05", "capabilities":{},
        "clientInfo":{"name":"source-identity-fixture","version":"1"}
    }});
    let mut input = process.0.stdin.take().expect("MCP stdin");
    writeln!(input, "{request}").expect("initialize MCP");
    input.flush().expect("flush initialize");
    let output = process.0.stdout.take().expect("MCP stdout");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break };
            let Ok(response) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if response["id"] == 1 {
                let _ = sender.send(response);
                break;
            }
        }
    });
    let response = receiver
        .recv_timeout(Duration::from_secs(20))
        .expect("MCP initialize response");
    assert!(response.get("error").is_none());
    response["result"]["_meta"]["cccc/build"]["source_id"]
        .as_str()
        .expect("MCP executable advertises its source identity")
        .to_owned()
}

async fn web_identity(root: &Path) -> String {
    let mut command = isolated_command(root);
    let home = cccc_core::HomeLayout::from_path(root.join("cccc")).expect("isolated daemon home");
    let mut daemon = DaemonTask(tokio::spawn(cccc_daemon::run(home.clone())));
    let client = cccc_client::DaemonClient::new(home).with_timeout(Duration::from_secs(1));
    let request = |op: &str| cccc_contracts::DaemonRequest {
        v: 1,
        op: op.into(),
        args: Default::default(),
    };
    let ready_by = Instant::now() + Duration::from_secs(20);
    while client.call(&request("ping")).await.is_err() {
        assert!(!daemon.0.is_finished(), "fixture daemon exited before ping");
        assert!(Instant::now() < ready_by, "fixture daemon did not start");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut process = Process(
        command
            .env("CCCC_WEB_HOST", "127.0.0.1")
            .env("CCCC_WEB_PORT", "0")
            .env("CCCC_WEB_MODE", "exhibit")
            .stdout(Stdio::null())
            .spawn()
            .expect("start Web executable"),
    );
    let runtime_file = root.join("cccc/daemon/web_runtime.json");
    let deadline = Instant::now() + Duration::from_secs(20);
    let port = loop {
        let runtime = std::fs::read(&runtime_file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        if let Some(runtime) = runtime
            && runtime["pid"] == process.0.id()
            && let Some(port) = runtime["port"].as_u64().filter(|port| *port > 0)
        {
            break port;
        }
        assert!(
            process.0.try_wait().expect("poll Web").is_none(),
            "Web exited before binding"
        );
        assert!(
            Instant::now() < deadline,
            "Web did not publish its ephemeral port"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let response: Value = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("local Web client")
        .get(format!("http://127.0.0.1:{port}/api/v1/ping"))
        .send()
        .await
        .expect("Web ping")
        .error_for_status()
        .expect("successful Web ping")
        .json()
        .await
        .expect("Web ping JSON");
    let source_id = response["result"]["build"]["source_id"]
        .as_str()
        .expect("Web executable advertises its source identity")
        .to_owned();
    drop(process);
    client
        .call(&request("shutdown"))
        .await
        .expect("stop fixture daemon");
    tokio::time::timeout(Duration::from_secs(10), &mut daemon.0)
        .await
        .expect("fixture daemon shutdown deadline")
        .expect("fixture daemon task")
        .expect("fixture daemon shutdown");
    source_id
}

#[tokio::test]
async fn executable_reports_its_compiled_workspace_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let expected = source::fingerprint(&root, &source::inputs(&root).expect("source inputs"))
        .expect("workspace fingerprint");
    assert_eq!(expected.len(), 64);
    let isolated = tempfile::tempdir().expect("isolated process state");
    let observed = if super::WEB {
        web_identity(isolated.path()).await
    } else {
        mcp_identity(isolated.path())
    };
    assert_eq!(
        observed,
        env!("CCCC_SOURCE_ID"),
        "the executable must report its compiled identity"
    );
    assert_eq!(
        env!("CCCC_SOURCE_ID"),
        expected,
        "workspace source changed after the application was built"
    );
}
