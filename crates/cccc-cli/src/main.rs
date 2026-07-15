mod args;
mod commands;

use anyhow::{Context, Result, bail};
use args::{Cli, CommandKind, DaemonAction, RuntimeAction};
use cccc_client::DaemonClient;
use cccc_core::{HomeLayout, active};
use clap::Parser;
use commands::common::{call, print};
use serde_json::json;
use std::process::Command;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let home = HomeLayout::resolve()?;
    let client = DaemonClient::new(home.clone());
    let web_endpoint = web_endpoint(cli.host.as_deref(), cli.port);
    match cli.command {
        None | Some(CommandKind::Web) => launch(home, cli.host, cli.port).await,
        Some(CommandKind::Mcp) => cccc_mcp::run_stdio(home).await,
        Some(CommandKind::Version) => {
            println!("cccc {} (rust)", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(CommandKind::Home) => {
            println!("{}", home.root().display());
            Ok(())
        }
        Some(CommandKind::Attach { path, group_id }) => print(
            call(
                &client,
                "attach",
                json!({"path":path,"group_id":group_id,"by":"user"}),
            )
            .await?,
        ),
        Some(CommandKind::Group(args)) => commands::group::run(&client, &home, args).await,
        Some(CommandKind::Groups) => print(call(&client, "group_list", json!({})).await?),
        Some(CommandKind::Use { group_id }) => {
            print(call(&client, "group_use", json!({"group_id":group_id})).await?)
        }
        Some(CommandKind::Active) => show_active(&client, &home).await,
        Some(CommandKind::Actor(args)) => commands::actor::run(&client, &home, args).await,
        Some(CommandKind::Prompt(args)) => {
            commands::integrations::prompt(&client, &home, args).await
        }
        Some(CommandKind::Im(args)) => commands::integrations::im(&home, &web_endpoint, args).await,
        Some(CommandKind::Space(args)) => commands::integrations::space(&client, &home, args).await,
        Some(CommandKind::Send(args)) => commands::messaging::send(&client, &home, args).await,
        Some(CommandKind::TrackedSend(args)) => {
            commands::messaging::tracked(&client, &home, args).await
        }
        Some(CommandKind::Reply(args)) => commands::messaging::reply(&client, &home, args).await,
        Some(CommandKind::Tail(args)) => commands::messaging::tail(&client, &home, args).await,
        Some(CommandKind::Inbox(args)) => commands::messaging::inbox(&client, &home, args).await,
        Some(CommandKind::Read(args)) => commands::messaging::read(&client, &home, args).await,
        Some(CommandKind::Ledger(args)) => commands::messaging::ledger(&client, &home, args).await,
        Some(CommandKind::Daemon { action }) => daemon(action, home, &client).await,
        Some(CommandKind::Runtime { action }) => runtime(action),
        Some(CommandKind::Status) => status(&client).await,
        Some(CommandKind::Doctor) => doctor(&home),
        Some(CommandKind::Setup) => setup(),
    }
}

fn web_endpoint(host: Option<&str>, port: Option<u16>) -> String {
    let host = host
        .map(str::to_owned)
        .or_else(|| std::env::var("CCCC_WEB_HOST").ok())
        .unwrap_or_else(|| "127.0.0.1".into());
    let host = match host.as_str() {
        "0.0.0.0" | "::" => "127.0.0.1",
        value => value,
    };
    let port = port
        .or_else(|| {
            std::env::var("CCCC_WEB_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(8848);
    let url_host = if host.starts_with('[') && host.ends_with(']') {
        host.to_owned()
    } else if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    format!("http://{url_host}:{port}")
}

async fn launch(home: HomeLayout, host: Option<String>, port: Option<u16>) -> Result<()> {
    home.initialize()?;
    let client =
        DaemonClient::new(home.clone()).with_timeout(std::time::Duration::from_millis(250));
    replace_incompatible_daemon(&client).await?;
    let mut embedded_daemon = None;
    if !ping(&client).await {
        let daemon_home = home.clone();
        embedded_daemon = Some(tokio::spawn(
            async move { cccc_daemon::run(daemon_home).await },
        ));
        wait_for_daemon(&client, std::time::Duration::from_secs(30)).await;
    }
    if !ping(&client).await {
        finish_embedded_daemon(&client, embedded_daemon.take()).await;
        bail!("embedded Rust daemon failed to start");
    }
    let monitor = DaemonClient::new(home.clone()).with_timeout(std::time::Duration::from_secs(2));
    let daemon_address = home.daemon_dir().join("ccccd.addr.json");
    let host = host
        .or_else(|| std::env::var("CCCC_WEB_HOST").ok())
        .unwrap_or_else(|| "127.0.0.1".into());
    let port = port
        .or_else(|| {
            std::env::var("CCCC_WEB_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(8848);
    let shutdown = async move {
        wait_for_daemon_loss(&monitor, &daemon_address).await;
        eprintln!("CCCC daemon stopped; Web server closed");
    };
    let result = cccc_web::serve_until(home, &host, port, shutdown)
        .await
        .map(|_| ());
    finish_embedded_daemon(&client, embedded_daemon.take()).await;
    result
}

async fn finish_embedded_daemon(
    client: &DaemonClient,
    daemon: Option<tokio::task::JoinHandle<Result<()>>>,
) {
    let Some(mut daemon) = daemon else {
        return;
    };
    if !daemon.is_finished() {
        let _ = call(client, "shutdown", json!({})).await;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(10), &mut daemon).await {
        Ok(Ok(Err(error))) => eprintln!("embedded daemon stopped: {error}"),
        Ok(Err(error)) if !error.is_cancelled() => {
            eprintln!("embedded daemon task failed: {error}")
        }
        Err(_) => {
            eprintln!("embedded daemon did not stop in time; cancelling it");
            daemon.abort();
            let _ = daemon.await;
        }
        _ => {}
    }
}

async fn daemon(action: DaemonAction, home: HomeLayout, client: &DaemonClient) -> Result<()> {
    match action {
        DaemonAction::Run => cccc_daemon::run(home).await,
        DaemonAction::Stop => print(call(client, "shutdown", json!({})).await?),
        DaemonAction::Status => {
            if ping(client).await {
                println!("ccccd: running");
                Ok(())
            } else {
                bail!("ccccd: not running")
            }
        }
        DaemonAction::Start => {
            replace_incompatible_daemon(client).await?;
            if ping(client).await {
                println!("ccccd: already running");
                return Ok(());
            }
            let executable = daemon_executable()?;
            let status = Command::new(&executable)
                .arg("start")
                .status()
                .with_context(|| format!("start {}", executable.display()))?;
            if status.success() {
                Ok(())
            } else {
                bail!("ccccd start failed with {status}")
            }
        }
    }
}

async fn show_active(client: &DaemonClient, home: &HomeLayout) -> Result<()> {
    let group_id = active::get(home)?.ok_or_else(|| anyhow::anyhow!("no active group"))?;
    print(call(client, "group_show", json!({"group_id":group_id})).await?)
}

async fn status(client: &DaemonClient) -> Result<()> {
    if !ping(client).await {
        bail!("ccccd: not running");
    }
    print(call(client, "group_list", json!({})).await?)
}

fn runtime(action: RuntimeAction) -> Result<()> {
    match action {
        RuntimeAction::List => println!(
            "{}",
            serde_json::to_string_pretty(&cccc_runtime::detect_runtimes())?
        ),
    }
    Ok(())
}

fn doctor(home: &HomeLayout) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "implementation":"rust","home":home.root(),"runtimes":cccc_runtime::detect_runtimes()
        }))?
    );
    Ok(())
}

fn setup() -> Result<()> {
    let executable = std::env::current_exe()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "mcpServers":{"cccc":{"command":executable,"args":["mcp"],"env":{"CCCC_HOME":HomeLayout::resolve()?.root()}}}
        }))?
    );
    Ok(())
}

async fn ping(client: &DaemonClient) -> bool {
    call(client, "ping", json!({}))
        .await
        .is_ok_and(|response| is_compatible_daemon(&response))
}

async fn wait_for_daemon(client: &DaemonClient, timeout: std::time::Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if ping(client).await {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

async fn wait_for_daemon_loss(client: &DaemonClient, address: &std::path::Path) {
    let mut failures = 0_u8;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if ping(client).await {
            failures = 0;
        } else {
            if !address.exists() {
                return;
            }
            failures += 1;
            if failures >= 12 {
                return;
            }
        }
    }
}

async fn replace_incompatible_daemon(client: &DaemonClient) -> Result<()> {
    let Ok(response) = call(client, "ping", json!({})).await else {
        return Ok(());
    };
    if is_compatible_daemon(&response) {
        return Ok(());
    }
    eprintln!("Switching CCCC daemon from legacy or incompatible implementation to Rust...");
    let shutdown = call(client, "shutdown", json!({})).await?;
    if !shutdown.ok {
        bail!("failed to stop incompatible CCCC daemon");
    }
    for _ in 0..40 {
        if call(client, "ping", json!({})).await.is_err() {
            // The legacy daemon can remove its socket just before releasing the
            // shared process lock. Give shutdown cleanup a brief grace period.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    bail!("incompatible CCCC daemon did not stop")
}

fn is_compatible_daemon(response: &cccc_contracts::DaemonResponse) -> bool {
    response.ok
        && response
            .result
            .get("implementation")
            .and_then(serde_json::Value::as_str)
            == Some("rust")
        && response
            .result
            .get("version")
            .and_then(serde_json::Value::as_str)
            == Some(env!("CARGO_PKG_VERSION"))
        && response
            .result
            .get("compatibility")
            .and_then(serde_json::Value::as_str)
            == Some(cccc_contracts::RUST_DAEMON_COMPATIBILITY)
}

fn daemon_executable() -> Result<std::path::PathBuf> {
    let current = std::env::current_exe()?;
    let name = if cfg!(windows) { "ccccd.exe" } else { "ccccd" };
    let sibling = current.with_file_name(name);
    if sibling.is_file() {
        return Ok(sibling);
    }
    Ok(name.into())
}

#[cfg(test)]
mod tests {
    use super::{is_compatible_daemon, web_endpoint};
    use cccc_contracts::DaemonResponse;
    use serde_json::json;

    #[test]
    fn distinguishes_rust_from_legacy_daemon_ping() {
        let rust = DaemonResponse::success(
            json!({
                "implementation":"rust",
                "version":env!("CARGO_PKG_VERSION"),
                "compatibility":cccc_contracts::RUST_DAEMON_COMPATIBILITY,
            })
            .as_object()
            .cloned()
            .expect("object"),
        );
        let legacy = DaemonResponse::success(
            json!({"version":"0.4.31"})
                .as_object()
                .cloned()
                .expect("object"),
        );
        let stale_rust = DaemonResponse::success(
            json!({"implementation":"rust","version":env!("CARGO_PKG_VERSION")})
                .as_object()
                .cloned()
                .expect("object"),
        );
        assert!(is_compatible_daemon(&rust));
        assert!(!is_compatible_daemon(&legacy));
        assert!(!is_compatible_daemon(&stale_rust));
    }

    #[test]
    fn web_endpoint_brackets_ipv6_literals() {
        assert_eq!(web_endpoint(Some("::1"), Some(8848)), "http://[::1]:8848");
        assert_eq!(
            web_endpoint(Some("[2001:db8::1]"), Some(9000)),
            "http://[2001:db8::1]:9000"
        );
        assert_eq!(
            web_endpoint(Some("127.0.0.1"), Some(8848)),
            "http://127.0.0.1:8848"
        );
    }
}
