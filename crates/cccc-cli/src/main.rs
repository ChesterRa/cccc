mod args;
mod commands;
mod hook_receiver;
mod web_instance;
mod web_launch;

use anyhow::{Result, bail};
use args::{Cli, CommandKind, DaemonAction, HermesAction, RuntimeAction, WebModeArg};
use cccc_client::DaemonClient;
use cccc_core::{HomeLayout, active};
use cccc_daemon::{DetachedDaemon, StartOutcome};
use clap::Parser;
use commands::common::{call, print};
use serde_json::json;

const PRODUCT_VERSION: &str = env!("CCCC_PRODUCT_VERSION");
const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let home = HomeLayout::resolve()?;
    let client = DaemonClient::new(home.clone());
    match cli.command {
        None => launch(home, cli.host, cli.port, None).await,
        Some(CommandKind::Web(args)) => {
            let mode = if args.exhibit {
                Some(cccc_web::WebMode::Exhibit)
            } else {
                args.mode.map(|mode| match mode {
                    WebModeArg::Normal => cccc_web::WebMode::Normal,
                    WebModeArg::Exhibit => cccc_web::WebMode::Exhibit,
                })
            };
            launch(home, cli.host, cli.port, mode).await
        }
        Some(CommandKind::Mcp) => cccc_mcp::run_stdio(home).await,
        Some(CommandKind::Version) => {
            println!("cccc {PRODUCT_VERSION} (rust)");
            Ok(())
        }
        Some(CommandKind::Home) => {
            println!("{}", home.root().display());
            Ok(())
        }
        Some(CommandKind::Hook { action }) => hook_receiver::run(&home, action),
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
        Some(CommandKind::Im(args)) => {
            let binding = web_launch::resolve(&home, cli.host.as_deref(), cli.port)?;
            let web_endpoint = web_endpoint(&binding.host, binding.port);
            commands::integrations::im(&client, &home, &web_endpoint, args).await
        }
        Some(CommandKind::Space(args)) => {
            let binding = web_launch::resolve(&home, cli.host.as_deref(), cli.port)?;
            let web_endpoint = web_endpoint(&binding.host, binding.port);
            commands::integrations::space(&client, &home, &web_endpoint, args).await
        }
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
        Some(CommandKind::Runtime { action }) => runtime(&client, action).await,
        Some(CommandKind::Status) => status(&client).await,
        Some(CommandKind::Doctor) => commands::doctor::run(&home, PRODUCT_VERSION).await,
        Some(CommandKind::Setup(args)) => commands::setup::run(&home, args),
        Some(CommandKind::Update(args)) => {
            let installed = commands::update::run(args, PRODUCT_VERSION, CRATE_VERSION)?;
            if installed {
                stop_daemon_after_update(&client, &home).await?;
            }
            Ok(())
        }
    }
}

async fn stop_daemon_after_update(client: &DaemonClient, home: &HomeLayout) -> Result<()> {
    if running_daemon_pid(client).await.is_none() {
        return Ok(());
    }
    let response = call(client, "shutdown", json!({})).await?;
    if !response.ok {
        bail!("the update installed, but the previous CCCC daemon could not be stopped");
    }
    wait_for_daemon_loss(client, &home.daemon_dir().join("ccccd.addr.json")).await;
    println!("Stopped the previous CCCC daemon; the next command will start the updated version.");
    Ok(())
}

fn web_endpoint(host: &str, port: u16) -> String {
    let host = match host {
        "0.0.0.0" | "::" => "127.0.0.1",
        value => value,
    };
    let url_host = if host.starts_with('[') && host.ends_with(']') {
        host.to_owned()
    } else if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    format!("http://{url_host}:{port}")
}

async fn launch(
    home: HomeLayout,
    host_override: Option<String>,
    port_override: Option<u16>,
    web_mode: Option<cccc_web::WebMode>,
) -> Result<()> {
    home.initialize()?;
    let mut binding = web_launch::resolve(&home, host_override.as_deref(), port_override)?;
    let client =
        DaemonClient::new(home.clone()).with_timeout(std::time::Duration::from_millis(250));
    let _instance = claim_web_instance(&home, &client).await?;
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
    let shutdown_feedback = tokio::spawn(report_interrupt());
    let mode = web_mode.unwrap_or_else(cccc_web::WebMode::from_env);
    let result = loop {
        let monitor =
            DaemonClient::new(home.clone()).with_timeout(std::time::Duration::from_secs(2));
        let daemon_address = home.daemon_dir().join("ccccd.addr.json");
        let shutdown = async move {
            wait_for_daemon_loss(&monitor, &daemon_address).await;
            eprintln!("CCCC daemon stopped; Web server closed");
        };
        match cccc_web::serve_until_mode_supervised(
            home.clone(),
            &binding.host,
            binding.port,
            mode,
            shutdown,
        )
        .await
        {
            Ok(cccc_web::ServeOutcome::Stopped(_)) => break Ok(()),
            Ok(cccc_web::ServeOutcome::RestartRequested) => {
                binding = match web_launch::resolve(&home, None, None) {
                    Ok(binding) => binding,
                    Err(error) => break Err(error),
                };
                eprintln!(
                    "[cccc] Applying saved Web binding: http://{}:{}",
                    binding.host, binding.port
                );
            }
            Err(error) => break Err(error),
        }
    };
    shutdown_feedback.abort();
    finish_embedded_daemon(&client, embedded_daemon.take()).await;
    result
}

async fn claim_web_instance(
    home: &HomeLayout,
    client: &DaemonClient,
) -> Result<web_instance::WebInstance> {
    match web_instance::try_claim(home)? {
        web_instance::Claim::Acquired(instance) => Ok(instance),
        web_instance::Claim::Running(running) => {
            confirm_and_stop_existing(home, client, running.pid).await?;
            wait_for_web_instance_exit(home, std::time::Duration::from_secs(15)).await
        }
    }
}

async fn confirm_and_stop_existing(
    home: &HomeLayout,
    client: &DaemonClient,
    pid: Option<u32>,
) -> Result<()> {
    if !web_instance::confirm_stop(home, pid)? {
        bail!(
            "another CCCC process is already running for CCCC_HOME={}{}",
            home.root().display(),
            pid.map_or_else(String::new, |pid| format!(" (pid={pid})"))
        );
    }
    if running_daemon_pid(client).await.is_some() {
        let response = call(client, "shutdown", json!({})).await?;
        if !response.ok {
            bail!("failed to stop the existing CCCC process");
        }
        wait_for_daemon_loss(client, &home.daemon_dir().join("ccccd.addr.json")).await;
    }
    Ok(())
}

async fn running_daemon_pid(client: &DaemonClient) -> Option<u32> {
    call(client, "ping", json!({}))
        .await
        .ok()?
        .result
        .get("pid")?
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
}

async fn wait_for_web_instance_exit(
    home: &HomeLayout,
    timeout: std::time::Duration,
) -> Result<web_instance::WebInstance> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let web_instance::Claim::Acquired(instance) = web_instance::try_claim(home)? {
            return Ok(instance);
        }
        if tokio::time::Instant::now() >= deadline {
            bail!("existing CCCC process did not stop within 15 seconds");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

async fn report_interrupt() {
    if tokio::signal::ctrl_c().await.is_ok() {
        eprintln!("Stopping CCCC...");
    }
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
            let executable = std::env::current_exe()?;
            match DetachedDaemon::new(executable, ["daemon", "run"])
                .start(&home)
                .await?
            {
                StartOutcome::AlreadyRunning => println!("ccccd: already running"),
                StartOutcome::Started(pid) => println!("ccccd: started pid={pid}"),
            }
            Ok(())
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

async fn runtime(client: &DaemonClient, action: RuntimeAction) -> Result<()> {
    match action {
        RuntimeAction::List => println!(
            "{}",
            serde_json::to_string_pretty(&cccc_runtime::detect_runtimes())?
        ),
        RuntimeAction::Hermes { action } => {
            let (op, args) = match action {
                HermesAction::Status => ("runtime_hermes_status", json!({})),
                HermesAction::Prepare { cwd, yes, force } => (
                    "runtime_hermes_prepare",
                    json!({"cwd":cwd,"yes":yes,"force":force}),
                ),
                HermesAction::McpTest {
                    cwd,
                    group_id,
                    actor_id,
                } => (
                    "runtime_hermes_mcp_test",
                    json!({"cwd":cwd,"group_id":group_id,"actor_id":actor_id}),
                ),
            };
            print(call(client, op, args).await?)?;
        }
    }
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
            .get("compatibility")
            .and_then(serde_json::Value::as_str)
            == Some(cccc_contracts::RUST_DAEMON_COMPATIBILITY)
}

#[cfg(test)]
mod tests {
    use super::{PRODUCT_VERSION, is_compatible_daemon, web_endpoint};
    use cccc_contracts::DaemonResponse;
    use serde_json::json;

    #[test]
    fn distinguishes_rust_from_legacy_daemon_ping() {
        let rust = DaemonResponse::success(
            json!({
                "implementation":"rust",
                "version":PRODUCT_VERSION,
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
            json!({"implementation":"rust","version":PRODUCT_VERSION})
                .as_object()
                .cloned()
                .expect("object"),
        );
        let compatible_other_version = DaemonResponse::success(
            json!({
                "implementation":"rust",
                "version":"0.4.999",
                "compatibility":cccc_contracts::RUST_DAEMON_COMPATIBILITY,
            })
            .as_object()
            .cloned()
            .expect("object"),
        );
        assert!(is_compatible_daemon(&rust));
        assert!(is_compatible_daemon(&compatible_other_version));
        assert!(!is_compatible_daemon(&legacy));
        assert!(!is_compatible_daemon(&stale_rust));
    }

    #[test]
    fn web_endpoint_brackets_ipv6_literals() {
        assert_eq!(web_endpoint("::1", 8848), "http://[::1]:8848");
        assert_eq!(
            web_endpoint("[2001:db8::1]", 9000),
            "http://[2001:db8::1]:9000"
        );
        assert_eq!(web_endpoint("127.0.0.1", 8848), "http://127.0.0.1:8848");
    }
}
