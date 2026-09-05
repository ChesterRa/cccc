mod args;
mod commands;
mod confirm;
#[cfg(windows)]
mod console_encoding;
mod daemon_lifecycle;
mod daemon_takeover;
#[cfg(any(windows, test))]
mod detached_daemon_owner;
mod shutdown;
mod web_endpoint;
mod web_host;
mod web_instance;
mod web_launch;

use anyhow::{Result, bail};
use args::{Cli, CommandKind, DaemonAction, HermesAction, RuntimeAction, WebModeArg};
use cccc_client::DaemonClient;
use cccc_core::{GroupStore, HomeLayout, active};
use cccc_daemon::{DetachedDaemon, StartOutcome};
use clap::Parser;
use commands::common::{call, print};
use serde_json::json;

const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(windows)]
    let _console_encoding = console_encoding::use_utf8();
    // rustls refuses to choose a crypto backend once the graph enables both of
    // them, and this one does: serenity pulls in `ring` beside the workspace's
    // `aws-lc-rs`. Without a provider every TLS handshake panics its own task,
    // which leaves a daemon alive but unable to reach any IM bridge. Install it
    // at the single entry point every subcommand shares -- `daemon run` and
    // `mcp` run from this same binary -- so no process reaches TLS without one.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cli = Cli::parse();
    let home = HomeLayout::resolve()?;
    let client = DaemonClient::new(home.clone());
    match cli.command {
        None => web_host::launch(home, cli.host, cli.port, None).await,
        Some(CommandKind::Web(args)) => {
            if args.reload {
                eprintln!(
                    "[cccc] --reload is accepted for compatibility; the standalone Rust server does not use a source reloader"
                );
            }
            if args.log_level != "info" {
                eprintln!(
                    "[cccc] --log-level is accepted for compatibility; configure Rust logging with RUST_LOG"
                );
            }
            let mode = if args.exhibit {
                Some(cccc_web::WebMode::Exhibit)
            } else {
                args.mode.map(|mode| match mode {
                    WebModeArg::Normal => cccc_web::WebMode::Normal,
                    WebModeArg::Exhibit => cccc_web::WebMode::Exhibit,
                })
            };
            web_host::launch(home, cli.host, cli.port, mode).await
        }
        Some(CommandKind::Mcp) => cccc_mcp::run_stdio(home).await,
        Some(CommandKind::Version) => {
            println!("cccc {PRODUCT_VERSION}");
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
            select_active_group(&home, &group_id)?;
            show_active(&client, &home).await
        }
        Some(CommandKind::Active) => show_active(&client, &home).await,
        Some(CommandKind::Actor(args)) => commands::actor::run(&client, &home, args).await,
        Some(CommandKind::Prompt(args)) => {
            commands::integrations::prompt(&client, &home, args).await
        }
        Some(CommandKind::Im(args)) => {
            let binding = web_launch::resolve(&home, cli.host.as_deref(), cli.port)?;
            let web_endpoint = web_endpoint::format(&binding.host, binding.port);
            commands::integrations::im(&client, &home, &web_endpoint, args).await
        }
        Some(CommandKind::Space(args)) => {
            let binding = web_launch::resolve(&home, cli.host.as_deref(), cli.port)?;
            let web_endpoint = web_endpoint::format(&binding.host, binding.port);
            commands::integrations::space(&client, &home, &web_endpoint, args).await
        }
        Some(CommandKind::Send(args)) => commands::messaging::send(&client, &home, args).await,
        Some(CommandKind::TrackedSend(args)) => {
            commands::messaging::tracked(&client, &home, args).await
        }
        Some(CommandKind::Reply(args)) => commands::messaging::reply(&client, &home, args).await,
        Some(CommandKind::Deliver(args)) => {
            commands::messaging::deliver(&client, &home, args).await
        }
        Some(CommandKind::CancelReply(args)) => {
            commands::messaging::cancel_reply(&client, &home, args).await
        }
        Some(CommandKind::Tail(args)) => commands::messaging::tail(&client, &home, args).await,
        Some(CommandKind::Inbox(args)) => commands::messaging::inbox(&client, &home, args).await,
        Some(CommandKind::Ledger(args)) => commands::messaging::ledger(&client, &home, args).await,
        Some(CommandKind::Daemon { action }) => daemon(action, home, &client).await,
        Some(CommandKind::Runtime { action }) => runtime(&client, action).await,
        Some(CommandKind::Login) => commands::membership::login(&client).await,
        Some(CommandKind::Logout) => commands::membership::logout(&client).await,
        Some(CommandKind::Reach { action }) => commands::membership::reach(&client, action).await,
        Some(CommandKind::Status) => commands::status::run(&home, PRODUCT_VERSION).await,
        Some(CommandKind::Doctor(args)) => {
            commands::doctor::run(&home, PRODUCT_VERSION, args.all).await
        }
        Some(CommandKind::Setup(args)) => commands::setup::run(&home, args),
        Some(CommandKind::Update(args)) => commands::update::run(args).await,
    }
}

async fn daemon(action: DaemonAction, home: HomeLayout, client: &DaemonClient) -> Result<()> {
    match action {
        DaemonAction::Run => cccc_daemon::run(home).await,
        DaemonAction::Stop => {
            let response = daemon_lifecycle::stop(client, &home).await?;
            if response.ok {
                // A combined `cccc` process owns both Web and daemon. Do not
                // report process-wide shutdown while that executable is still
                // serving Web (or still locked against a Windows update).
                drop(
                    web_instance::wait_until_free(&home, daemon_lifecycle::DAEMON_SHUTDOWN_TIMEOUT)
                        .await?,
                );
            }
            print(response)
        }
        DaemonAction::Status => {
            if daemon_lifecycle::ping(client).await {
                println!("CCCC daemon: running");
                Ok(())
            } else {
                bail!("CCCC daemon: not running")
            }
        }
        DaemonAction::Start => {
            daemon_lifecycle::replace_incompatible(&home, client).await?;
            if daemon_lifecycle::ping(client).await {
                println!("CCCC daemon: already running");
                return Ok(());
            }
            let executable = std::env::current_exe()?;
            match DetachedDaemon::new(executable, ["daemon", "run"])
                .start(&home)
                .await?
            {
                StartOutcome::AlreadyRunning => println!("CCCC daemon: already running"),
                StartOutcome::Started(pid) => println!("CCCC daemon: started pid={pid}"),
            }
            Ok(())
        }
    }
}

async fn show_active(client: &DaemonClient, home: &HomeLayout) -> Result<()> {
    let group_id = active::get(home)?.ok_or_else(|| anyhow::anyhow!("no active group"))?;
    print(call(client, "group_show", json!({"group_id":group_id})).await?)
}

fn select_active_group(home: &HomeLayout, group_id: &str) -> Result<()> {
    GroupStore::new(home.clone())?
        .load(group_id)
        .map_err(|_| anyhow::anyhow!("group not found: {group_id}"))?;
    active::set(home, group_id)?;
    Ok(())
}

async fn runtime(client: &DaemonClient, action: RuntimeAction) -> Result<()> {
    match action {
        RuntimeAction::List { all } => {
            let mut runtimes = cccc_runtime::detect_runtimes();
            if !all {
                runtimes.retain(|runtime| runtime.name != "custom");
            }
            println!("{}", serde_json::to_string_pretty(&runtimes)?);
        }
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

#[cfg(test)]
mod tests {
    use super::select_active_group;
    use cccc_core::{GroupStore, HomeLayout, active};

    #[test]
    fn top_level_use_selects_a_group_without_overloading_scope_selection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("active", "")
            .expect("group");

        select_active_group(&home, &group.group_id).expect("select group");

        assert_eq!(
            active::get(&home).expect("active").as_deref(),
            Some(group.group_id.as_str())
        );
        assert!(select_active_group(&home, "g_missing").is_err());
    }
}
