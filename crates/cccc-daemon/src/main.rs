use anyhow::{Context, Result};
use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use clap::{Parser, Subcommand};
use serde_json::Map;
use std::fs::OpenOptions;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(name = "ccccd", version, about = "CCCC Rust daemon")]
struct Args {
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Debug, Subcommand)]
enum CommandKind {
    Run,
    Start,
    Stop,
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let home = HomeLayout::resolve()?;
    match Args::parse().command {
        CommandKind::Run => cccc_daemon::run(home).await,
        CommandKind::Start => start(home).await,
        CommandKind::Stop => stop(home).await,
        CommandKind::Status => status(home).await,
    }
}

async fn start(home: HomeLayout) -> Result<()> {
    home.initialize()?;
    if ping(&home).await {
        println!("ccccd: already running");
        return Ok(());
    }
    let paths = cccc_daemon::DaemonPaths::new(home.clone());
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log)?;
    let error_log = log.try_clone()?;
    let executable = std::env::current_exe()?;
    let mut command = detached_command(&executable);
    let child = command
        .arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(error_log))
        .current_dir(home.root())
        .spawn()
        .context("spawn Rust daemon")?;
    for _ in 0..40 {
        if ping(&home).await {
            println!("ccccd: started pid={}", child.id());
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!(
        "Rust daemon failed to become ready; see {}",
        paths.log.display()
    )
}

async fn stop(home: HomeLayout) -> Result<()> {
    let response = client(&home).call(&request("shutdown")).await;
    match response {
        Ok(response) if response.ok => {
            println!("ccccd: shutdown requested");
            Ok(())
        }
        _ => anyhow::bail!("ccccd: not running"),
    }
}

async fn status(home: HomeLayout) -> Result<()> {
    if ping(&home).await {
        println!("ccccd: running");
        Ok(())
    } else {
        anyhow::bail!("ccccd: not running")
    }
}

async fn ping(home: &HomeLayout) -> bool {
    client(home)
        .call(&request("ping"))
        .await
        .is_ok_and(|response| response.ok)
}

fn client(home: &HomeLayout) -> DaemonClient {
    DaemonClient::new(home.clone()).with_timeout(Duration::from_millis(300))
}

fn request(op: &str) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: Map::new(),
    }
}

#[cfg(unix)]
fn detached_command(executable: &std::path::Path) -> Command {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new("nohup");
    command.arg(executable);
    command.process_group(0);
    command
}

#[cfg(windows)]
fn detached_command(executable: &std::path::Path) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let mut command = Command::new(executable);
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    command
}
