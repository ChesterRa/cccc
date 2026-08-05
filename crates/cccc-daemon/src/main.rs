use anyhow::Result;
use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_daemon::{DetachedDaemon, StartOutcome};
use clap::{Parser, Subcommand};
use serde_json::Map;
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
    let executable = std::env::current_exe()?;
    report_start(
        DetachedDaemon::new(executable, ["run"])
            .start(&home)
            .await?,
    );
    Ok(())
}

fn report_start(outcome: StartOutcome) {
    match outcome {
        StartOutcome::AlreadyRunning => println!("ccccd: already running"),
        StartOutcome::Started(pid) => println!("ccccd: started pid={pid}"),
    }
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
