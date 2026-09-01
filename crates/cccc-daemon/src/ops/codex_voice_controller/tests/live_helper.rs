use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;

pub(super) struct LiveVoiceHelper {
    child: Child,
    input: Arc<tokio::sync::Mutex<ChildStdin>>,
    events: mpsc::Receiver<Value>,
}

impl LiveVoiceHelper {
    pub(super) async fn start(path: &Path) -> Result<Self> {
        let mut command = Command::new(path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        let mut child = command.spawn().context("spawn voice helper")?;
        let input = Arc::new(tokio::sync::Mutex::new(
            child.stdin.take().context("voice helper stdin")?,
        ));
        let stdout = child.stdout.take().context("voice helper stdout")?;
        let (sender, events) = mpsc::channel(512);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(event) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if sender.send(event).await.is_err() {
                    break;
                }
            }
        });
        let mut helper = Self {
            child,
            input,
            events,
        };
        let ready = helper.wait_for(|event| event["type"] == "ready").await?;
        if ready["version"] != 5 {
            bail!("voice helper protocol is not version 5");
        }
        Ok(helper)
    }

    pub(super) fn input(&self) -> Arc<tokio::sync::Mutex<ChildStdin>> {
        Arc::clone(&self.input)
    }

    pub(super) async fn send(&self, command: Value) -> Result<()> {
        send_helper_command(&self.input, &command).await
    }

    pub(super) async fn next_event(&mut self) -> Option<Value> {
        self.events.recv().await
    }

    pub(super) async fn wait_for(&mut self, predicate: impl Fn(&Value) -> bool) -> Result<Value> {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let event = self.events.recv().await.context("voice helper closed")?;
                if event["type"] == "error" {
                    bail!("voice helper failed: {}", event["message"]);
                }
                if predicate(&event) {
                    return Ok(event);
                }
            }
        })
        .await
        .context("voice helper event timeout")?
    }

    pub(super) async fn shutdown(mut self) -> Result<()> {
        self.send(serde_json::json!({"type":"shutdown"})).await?;
        if tokio::time::timeout(Duration::from_secs(3), self.child.wait())
            .await
            .is_err()
        {
            self.child.kill().await.context("kill voice helper")?;
            self.child.wait().await.context("wait for voice helper")?;
        }
        Ok(())
    }
}

pub(super) async fn send_helper_command(
    input: &Arc<tokio::sync::Mutex<ChildStdin>>,
    command: &Value,
) -> Result<()> {
    let mut input = input.lock().await;
    let mut line = serde_json::to_vec(command)?;
    line.push(b'\n');
    input.write_all(&line).await?;
    input.flush().await?;
    Ok(())
}

pub(super) fn required_env_path(name: &str) -> PathBuf {
    let path = std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} is required for the live Codex Voice test"));
    assert!(path.is_file(), "{name} does not point to a file");
    path
}
