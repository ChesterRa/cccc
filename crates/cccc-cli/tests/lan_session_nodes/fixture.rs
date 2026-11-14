use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, local_session::LocalSessionRuntime};
use cccc_core::{
    HomeLayout,
    local_sessions::{self, NativeHost, NativeSession},
};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdout},
    task::JoinHandle,
};

pub struct Fixture {
    pub _temp: tempfile::TempDir,
    pub hub: HomeLayout,
    pub node: HomeLayout,
    pub source: NativeSession,
    pub hub_client: DaemonClient,
    pub node_client: DaemonClient,
    pub bridge: Child,
    pub monitor: Child,
    pub lines: Lines<BufReader<ChildStdout>>,
    pub daemons: Vec<JoinHandle<anyhow::Result<()>>>,
}

pub fn command(root: &Path, home: &HomeLayout) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc"));
    command
        .env_clear()
        .env("PATH", "")
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("CCCC_HOME", home.root())
        .env("CLAUDE_CONFIG_DIR", root.join("provider"))
        .current_dir(root)
        .kill_on_drop(true);
    command
}

pub async fn call(client: &DaemonClient, op: &str, args: Value) -> Value {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().expect("args").clone(),
        })
        .await
        .expect("daemon exchange");
    assert!(response.ok, "{op}: {:?}", response.error);
    Value::Object(response.result)
}

pub async fn until(mut predicate: impl AsyncFnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !predicate().await {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("observable state transition");
}

impl Fixture {
    pub async fn start() -> Self {
        let temp = tempfile::tempdir().expect("isolated LAN fixture");
        let hub = HomeLayout::from_path(temp.path().join("hub")).expect("hub");
        let node = HomeLayout::from_path(temp.path().join("node")).expect("node");
        hub.initialize().expect("hub initialize");
        node.initialize().expect("node initialize");
        std::fs::create_dir(temp.path().join("provider")).expect("provider fixture");
        let mut daemons = Vec::new();
        for home in [hub.clone(), node.clone()] {
            daemons.push(tokio::spawn(async move { cccc_daemon::run(home).await }));
        }
        let hub_client = DaemonClient::new(hub.clone());
        let node_client = DaemonClient::new(node.clone());
        for client in [&hub_client, &node_client] {
            until(async || {
                client
                    .call(&DaemonRequest {
                        v: 1,
                        op: "ping".into(),
                        args: Default::default(),
                    })
                    .await
                    .is_ok()
            })
            .await;
        }
        let host = NativeHost::capture(std::process::id()).expect("controlled native host");
        let source = NativeSession {
            source_id: format!("claude:{}", host.key()),
            runtime: LocalSessionRuntime::Claude,
            native_session_id: "original-native-conversation".into(),
            host: host.clone(),
            cwd: temp.path().to_string_lossy().into_owned(),
            title: "Remote original Claude".into(),
            origin: None,
            revision: "original-revision".into(),
            context_epoch: 0,
            codex_home: None,
            codex_can_receive: false,
            codex_program: None,
            claude_config_dir: Some(temp.path().join("provider")),
            pi_agent_dir: None,
            opencode_config_dir: None,
            opencode_server: None,
            retired: None,
        };
        local_sessions::write(&node, source.clone()).expect("native lifecycle");
        let mut monitor = command(temp.path(), &node)
            .args(["sessions", "receive", "--host-pid", &host.pid.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("native Monitor adapter");
        let lines = BufReader::new(monitor.stdout.take().expect("Monitor pipe")).lines();
        until(async || {
            call(&node_client, "local_session_list", json!({})).await["sources"]
                .as_array()
                .is_some_and(|sources| sources.iter().any(|s| s["source_id"] == source.source_id))
        })
        .await;
        let ticket = cccc_web::session_node::create_invitation(&hub, 0)
            .await
            .expect("LAN gateway");
        let mut bridge = command(temp.path(), &node)
            .args([
                "sessions",
                "node",
                "connect",
                "--ticket-stdin",
                "--name",
                "Test laptop",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("native node bridge");
        let encoded = cccc_web::session_node::encode_ticket(&ticket).expect("ticket");
        bridge
            .stdin
            .take()
            .expect("ticket input")
            .write_all(format!("{encoded}\n").as_bytes())
            .await
            .expect("pairing input");
        until(async || {
            call(&hub_client, "local_session_list", json!({})).await["sources"]
                .as_array()
                .is_some_and(|sources| {
                    sources.iter().any(|s| {
                        s["source_id"]
                            .as_str()
                            .is_some_and(|id| id.starts_with("remote:"))
                    })
                })
        })
        .await;
        Self {
            _temp: temp,
            hub,
            node,
            source,
            hub_client,
            node_client,
            bridge,
            monitor,
            lines,
            daemons,
        }
    }

    pub async fn mcp(&self, name: &str, args: Value) -> Value {
        self.mcp_for(&self.source, name, args).await
    }

    pub async fn mcp_for(&self, source: &NativeSession, name: &str, args: Value) -> Value {
        super::native_mcp::mcp_for(self._temp.path(), &self.node, source, name, args).await
    }

    pub async fn close(mut self) {
        if self.bridge.try_wait().expect("bridge state").is_none() {
            self.bridge.start_kill().expect("stop fixture bridge");
        }
        self.bridge.wait().await.expect("bridge exit");
        if self.monitor.try_wait().expect("Monitor state").is_none() {
            self.monitor.start_kill().expect("stop fixture Monitor");
        }
        self.monitor.wait().await.expect("Monitor exit");
        cccc_web::session_node::stop(&self.hub).await;
        for client in [&self.hub_client, &self.node_client] {
            call(client, "shutdown", json!({})).await;
        }
        for daemon in self.daemons.drain(..) {
            tokio::time::timeout(Duration::from_secs(5), daemon)
                .await
                .expect("daemon stops")
                .expect("daemon task")
                .expect("daemon shutdown");
        }
    }
}
