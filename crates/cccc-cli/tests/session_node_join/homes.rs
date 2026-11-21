use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::{HomeLayout, session_nodes};
use std::{
    process::{Output, Stdio},
    time::Duration,
};
use tokio::{
    process::{Child, Command},
    task::JoinHandle,
};

#[path = "../session_node_support/daemon.rs"]
mod daemon;

pub struct Homes {
    temp: tempfile::TempDir,
    pub node: HomeLayout,
    pub hub: HomeLayout,
    tasks: Vec<JoinHandle<anyhow::Result<()>>>,
}

impl Drop for Homes {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl Homes {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().expect("isolated device homes");
        #[cfg(windows)]
        for dir in ["AppData/Roaming", "AppData/Local/Temp"] {
            std::fs::create_dir_all(temp.path().join(dir))
                .expect("isolated Windows profile folders");
        }
        let node = HomeLayout::from_path(temp.path().join("node")).expect("node");
        let hub = HomeLayout::from_path(temp.path().join("hub")).expect("hub");
        Self {
            temp,
            node,
            hub,
            tasks: vec![],
        }
    }

    pub async fn start(&mut self) {
        for h in [&self.hub, &self.node] {
            daemon::start(h.clone(), &mut self.tasks).await;
        }
    }

    #[cfg(unix)]
    pub fn install_fake_codex(&self) {
        use std::os::unix::fs::PermissionsExt;
        let bin = self.temp.path().join("bin");
        std::fs::create_dir_all(&bin).expect("fixture PATH");
        std::fs::create_dir_all(self.temp.path().join("codex")).expect("fixture provider");
        let exe = bin.join("codex");
        std::fs::write(&exe, include_str!("../session_node_setup/provider.sh"))
            .expect("controlled provider");
        std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o700))
            .expect("fixture executable");
    }

    #[cfg(unix)]
    pub fn native_calls(&self) -> String {
        std::fs::read_to_string(self.temp.path().join("native-calls")).unwrap_or_default()
    }

    pub fn command(&self, args: &[&str]) -> Child {
        let mut c = Command::new(env!("CARGO_BIN_EXE_cccc"));
        c.args(["sessions", "node"])
            .args(args)
            .env_clear()
            .env("HOME", self.temp.path())
            .env("USERPROFILE", self.temp.path())
            .env("CCCC_HOME", self.node.root())
            .env("NO_PROXY", "*")
            .env("PATH", "")
            .env("CODEX_HOME", self.temp.path().join("codex"))
            .env("CLAUDE_CONFIG_DIR", self.temp.path().join("claude"))
            .current_dir(self.temp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        c.env(
            "PATH",
            format!("{}:/usr/bin:/bin", self.temp.path().join("bin").display()),
        );
        #[cfg(windows)]
        {
            let profile = self.temp.path();
            let local = profile.join("AppData/Local");
            c.env("COMPUTERNAME", "Fixture-PC")
                .env("APPDATA", profile.join("AppData/Roaming"))
                .env("LOCALAPPDATA", &local)
                .env("TEMP", local.join("Temp"))
                .env("TMP", local.join("Temp"));
            if let Some(windows) = std::env::var_os("SystemRoot") {
                let system = std::path::PathBuf::from(windows).join("System32");
                c.env("PATH", system);
            }
        }
        for key in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(key) {
                c.env(key, value);
            }
        }
        c.spawn().expect("start real CLI")
    }

    pub fn descriptor_bytes(&self) -> Vec<u8> {
        std::fs::read(self.node.root().join("state/session_nodes/client.json"))
            .expect("committed client state")
    }

    pub async fn close(&mut self) {
        cccc_web::session_node::stop(&self.hub).await;
        for h in [&self.node, &self.hub] {
            let _ = DaemonClient::new(h.clone())
                .call(&DaemonRequest {
                    v: 1,
                    op: "shutdown".into(),
                    args: Default::default(),
                })
                .await;
        }
        for t in self.tasks.drain(..) {
            tokio::time::timeout(Duration::from_secs(6), t)
                .await
                .expect("daemon deadline")
                .expect("daemon task")
                .expect("daemon exit");
        }
    }
}

pub async fn paste(child: &mut Child, value: &str) {
    use tokio::io::AsyncWriteExt;
    child
        .stdin
        .take()
        .expect("ticket pipe")
        .write_all(format!("{value}\n").as_bytes())
        .await
        .expect("paste device ticket");
}

pub async fn progress(child: &mut Child, prefix: &str, forbidden: &[&str]) -> String {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut reader = BufReader::new(child.stderr.take().expect("CLI diagnostics"));
    let mut text = String::new();
    tokio::time::timeout(Duration::from_secs(18), async {
        loop {
            let start = text.len();
            assert!(
                reader
                    .read_line(&mut text)
                    .await
                    .expect("connector progress")
                    > 0,
                "CLI exited before the expected progress: {prefix}"
            );
            assert!(
                text.len() < 48 * 1024 && forbidden.iter().all(|value| !text.contains(value)),
                "unexpected private data or unbounded connector diagnostics"
            );
            if text[start..].starts_with(prefix) {
                break;
            }
        }
    })
    .await
    .expect("terminal connector progress");
    child.stderr = Some(reader.into_inner());
    text
}

pub async fn connection(
    home: &HomeLayout,
    child: &mut Child,
    forbidden: &[&str],
) -> session_nodes::ClientDescriptor {
    let pid = child.id().expect("connector pid");
    let messages = progress(child, "Session node connected.", forbidden).await;
    assert!(
        messages.contains("Encrypted connection established. Waiting for hub confirmation."),
        "the terminal must distinguish a working transport from completed device enrollment"
    );
    let descriptor = session_nodes::client(home)
        .expect("read connector state")
        .expect("connected requires a committed descriptor");
    assert_eq!(descriptor.owner.pid, pid);
    descriptor
}

pub async fn stop(mut child: Child, forbidden: &[&str]) {
    child.start_kill().expect("stop fixture connector");
    output(child, forbidden).await;
}

pub async fn output(child: Child, private: &[&str]) -> (bool, String) {
    let result: Output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .expect("child exit deadline")
        .expect("child result");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        private.iter().all(|secret| !text.contains(secret)),
        "join output disclosed a fixture credential"
    );
    (result.status.success(), text)
}
