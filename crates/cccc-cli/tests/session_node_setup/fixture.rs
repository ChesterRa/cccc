#[path = "../session_node_support/daemon.rs"]
mod daemon;
use cccc_contracts::{DaemonRequest, session_node::SessionNodeEndpoint};
use cccc_core::{HomeLayout, local_sessions::NativeHost, session_nodes::ClientDescriptor};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Output, Stdio},
    time::Duration,
};
use tokio::{io::AsyncWriteExt, task::JoinHandle};

pub struct Fixture {
    temp: tempfile::TempDir,
    pub node: HomeLayout,
    pub hub: HomeLayout,
    daemons: Vec<JoinHandle<anyhow::Result<()>>>,
}

impl Fixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().expect("isolated setup fixture");
        let node = HomeLayout::from_path(temp.path().join("node")).expect("node");
        let hub = HomeLayout::from_path(temp.path().join("hub")).expect("hub");
        let bin = temp.path().join("bin");
        std::fs::create_dir(&bin).expect("bin");
        for runtime in ["codex", "claude"] {
            std::fs::create_dir(temp.path().join(runtime)).expect("provider");
            let path = bin.join(runtime);
            std::fs::write(&path, include_str!("provider.sh"))
                .expect("native configuration boundary");
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .expect("executable");
        }
        Self {
            temp,
            node,
            hub,
            daemons: Vec::new(),
        }
    }

    pub fn root(&self) -> &Path {
        self.temp.path()
    }
    pub fn provider(&self, runtime: &str) -> PathBuf {
        self.root().join(runtime)
    }
    pub fn calls(&self) -> String {
        std::fs::read_to_string(self.root().join("native-calls")).unwrap_or_default()
    }
    pub fn client_bytes(&self) -> Vec<u8> {
        std::fs::read(self.node.root().join("state/session_nodes/client.json"))
            .expect("client bytes")
    }

    pub async fn start(&mut self) {
        for home in [self.hub.clone(), self.node.clone()] {
            self.start_home(home).await;
        }
    }

    pub async fn start_home(&mut self, home: HomeLayout) {
        daemon::start(home, &mut self.daemons).await;
    }

    pub fn spawn(&mut self, task: JoinHandle<anyhow::Result<()>>) {
        self.daemons.push(task);
    }

    pub async fn ticket(&self) -> String {
        let ticket = cccc_web::session_node::create_invitation(&self.hub, 0)
            .await
            .expect("isolated invitation");
        cccc_web::session_node::encode_ticket(&ticket).expect("encode test invitation")
    }

    pub fn saved(&self, live: bool) -> ClientDescriptor {
        let mut owner = NativeHost::capture(std::process::id()).expect("fixture owner");
        if !live {
            owner.started.push_str("-old-process");
        }
        ClientDescriptor {
            join_binding: None,
            endpoint: SessionNodeEndpoint {
                endpoint_id: "previous-hub".into(),
                addresses: vec!["127.0.0.1:12345".into()],
                relay_url: None,
            },
            node_id: "previous-node".into(),
            grant_id: "previous-grant".into(),
            owner,
            address: "127.0.0.1:12346".parse().expect("loopback"),
            secret: "fixture-broker-secret".into(),
            connected: live,
            memberships: Vec::new(),
        }
    }

    pub fn command(&self, args: &[&str]) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc"));
        command
            .args(["sessions", "node", "connect"])
            .args(args)
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root().join("bin").display()),
            )
            .env("HOME", self.root())
            .env("USERPROFILE", self.root())
            .env("CCCC_HOME", self.node.root())
            .env("CODEX_HOME", self.provider("codex"))
            .env("CLAUDE_CONFIG_DIR", self.provider("claude"))
            .current_dir(self.root())
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    pub async fn run(&self, args: &[&str], ticket: &str) -> Output {
        let mut child = self.command(args).spawn().expect("real CLI");
        // Argument validation can close stdin before this write; status and
        // filesystem observations below, not a successful pipe write, are the contract.
        let _ = child
            .stdin
            .take()
            .expect("stdin")
            .write_all(format!("{ticket}\n").as_bytes())
            .await;
        tokio::time::timeout(Duration::from_secs(15), child.wait_with_output())
            .await
            .expect("bounded CLI completion")
            .expect("CLI result")
    }

    pub fn assert_private(&self, response: &Output, ticket: &str) {
        for text in [
            String::from_utf8_lossy(&response.stdout),
            String::from_utf8_lossy(&response.stderr),
        ] {
            assert!(!text.contains(ticket), "pairing ticket must not be printed");
        }
        assert!(
            !self.calls().contains(ticket),
            "ticket must not reach native command argv"
        );
    }

    pub async fn close(&mut self) {
        cccc_web::session_node::stop(&self.hub).await;
        for home in [&self.node, &self.hub] {
            let _ = cccc_client::DaemonClient::new(home.clone())
                .call(&DaemonRequest {
                    v: 1,
                    op: "shutdown".into(),
                    args: Default::default(),
                })
                .await;
        }
        for task in self.daemons.drain(..) {
            tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .expect("daemon stops")
                .expect("daemon task")
                .expect("daemon result");
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for daemon in &self.daemons {
            daemon.abort();
        }
    }
}
