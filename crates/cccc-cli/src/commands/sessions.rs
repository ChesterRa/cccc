use super::common::{call, print};
use crate::args::SessionsAction;
use anyhow::{Context, Result, bail};
use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, local_session::LocalSessionRuntime};
use cccc_core::{
    HomeLayout,
    local_sessions::{self, NativeHost, NativeSession, RetireReason},
};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod extension;
pub(crate) mod setup;

pub async fn run(home: &HomeLayout, action: SessionsAction) -> Result<()> {
    let client = DaemonClient::new(home.clone());
    match action {
        SessionsAction::Enable { runtime } => setup::enable(home, &runtime).await?.print(),
        SessionsAction::Disable { runtime } => setup::disable(home, &runtime).await?.print(),
        SessionsAction::List { include_offline } => print(
            call(
                &client,
                "local_session_list",
                json!({"include_offline":include_offline}),
            )
            .await?,
        ),
        SessionsAction::Create { sources, title } => print(
            call(
                &client,
                "local_session_conversation",
                json!({"source_ids":sources,"title":title}),
            )
            .await?,
        ),
        SessionsAction::Join { group, sources } => print(
            call(
                &client,
                "local_session_join",
                json!({"group_id":group,"source_ids":sources}),
            )
            .await?,
        ),
        SessionsAction::Leave { group, source_id } => print(
            call(
                &client,
                "local_session_leave",
                json!({"group_id":group,"source_id":source_id}),
            )
            .await?,
        ),
        SessionsAction::Recovery {
            group,
            actor,
            refresh,
        } => print(
            call(
                &client,
                if refresh {
                    "local_session_refresh"
                } else {
                    "local_session_recovery"
                },
                json!({"group_id":group,"actor_id":actor,"by":"user"}),
            )
            .await?,
        ),
        SessionsAction::Send {
            to,
            group,
            source_id,
            text,
        } => {
            let mut args = json!({"source_id":source_id,"text":text});
            if let Some(group) = group {
                args["group_id"] = json!(group);
            } else {
                args["source_ids"] = json!(to);
            }
            print(call(&client, "local_session_send", args).await?)
        }
        SessionsAction::Hook {
            runtime,
            host_pid,
            host_shell_pid,
            cccc_home,
            provider_home,
        } => {
            let home = cccc_home
                .map(HomeLayout::from_path)
                .transpose()?
                .unwrap_or_else(|| home.clone());
            hook(&home, &runtime, host_pid, host_shell_pid, provider_home).await
        }
        SessionsAction::Receive { host_pid } => receive(home, host_pid).await,
        SessionsAction::ExtensionBridge {
            runtime,
            host_pid,
            session_id,
            cwd,
            provider_home,
            backend_pid,
        } => {
            extension::bridge(
                home,
                &runtime,
                host_pid,
                &session_id,
                &cwd,
                &provider_home,
                backend_pid,
            )
            .await
        }
    }
}

fn native_home(name: &str, folder: &str) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(name).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    Ok(PathBuf::from(
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .context("User home is unavailable")?,
    )
    .join(folder))
}

fn native_host(
    runtime: &str,
    host_pid: Option<u32>,
    host_shell_pid: Option<u32>,
) -> Result<NativeHost> {
    if let Some(pid) = host_pid {
        return Ok(NativeHost::capture(pid)?);
    }
    if let Some(pid) = host_shell_pid {
        anyhow::ensure!(runtime == "codex", "Unexpected native shell wrapper");
        return codex_windows_host(pid);
    }
    if runtime == "codex" {
        // The POSIX hook uses exec, replacing the provider's hook shell.
        return Ok(NativeHost::parent(std::process::id())?);
    }
    let pid = std::env::var("CLAUDE_PID")
        .context("Claude did not supply its native CLAUDE_PID")?
        .parse::<u32>()
        .context("Invalid native CLAUDE_PID")?;
    let host = NativeHost::capture(pid)?;
    // Hooks/Monitor supply the authoritative PID. Check native ancestry rather
    // than trusting a shell's PPID or finding a process by executable name.
    let mut ancestor = NativeHost::parent(std::process::id())?;
    for _ in 0..16 {
        if ancestor == host {
            return Ok(host);
        }
        ancestor = NativeHost::parent(ancestor.pid)?;
    }
    bail!("The native Claude host is not a verified ancestor of this integration")
}

#[cfg(windows)]
fn codex_windows_host(shell_pid: u32) -> Result<NativeHost> {
    let bridge = NativeHost::parent(std::process::id())?;
    anyhow::ensure!(
        bridge.pid == shell_pid,
        "Native hook wrapper identity changed"
    );
    let image_name = |pid| -> Result<String> {
        Ok(cccc_windows_process::image_path(pid)?
            .file_name()
            .context("Native shell executable unavailable")?
            .to_string_lossy()
            .to_ascii_lowercase())
    };
    anyhow::ensure!(
        image_name(bridge.pid)? == "powershell.exe",
        "Unexpected native hook bridge"
    );
    let shell = NativeHost::parent(bridge.pid)?;
    anyhow::ensure!(
        matches!(
            image_name(shell.pid)?.as_str(),
            "cmd.exe" | "powershell.exe" | "pwsh.exe" | "bash.exe" | "sh.exe" | "zsh.exe"
        ),
        "Native Codex hook shell could not be verified; no host was registered"
    );
    Ok(NativeHost::parent(shell.pid)?)
}

#[cfg(not(windows))]
fn codex_windows_host(_shell_pid: u32) -> Result<NativeHost> {
    bail!("The native Windows hook bridge is unavailable on this platform")
}

async fn hook(
    home: &HomeLayout,
    runtime: &str,
    host_pid: Option<u32>,
    host_shell_pid: Option<u32>,
    provider_home: Option<PathBuf>,
) -> Result<()> {
    if local_sessions::cccc_owned_environment() {
        return Ok(());
    }
    let runtime = match runtime {
        "codex" => LocalSessionRuntime::Codex,
        "claude" => LocalSessionRuntime::Claude,
        _ => bail!("Unsupported native runtime"),
    };
    if local_sessions::integration_disabled(home, runtime)? {
        return Ok(());
    }
    let mut input = String::new();
    tokio::io::stdin()
        .take(16 * 1024 * 1024)
        .read_to_string(&mut input)
        .await?;
    let event: Value = serde_json::from_str(&input)?;
    let native_id = event["session_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .context("Native hook did not supply session_id")?;
    let host = native_host(runtime.as_str(), host_pid, host_shell_pid)?;
    let identity = if runtime == LocalSessionRuntime::Codex {
        native_id.to_owned()
    } else {
        host.key()
    };
    let source_id = runtime.source_id(&identity);
    if event["hook_event_name"] == "SessionEnd" {
        local_sessions::retire(home, &source_id, native_id, &host, RetireReason::SessionEnd)?;
        return Ok(());
    }
    if !matches!(
        event["hook_event_name"].as_str(),
        Some("SessionStart" | "UserPromptSubmit")
    ) {
        return Ok(());
    }
    let cwd = event["cwd"]
        .as_str()
        .context("Native hook did not supply cwd")?
        .to_owned();
    let default_title = format!(
        "{} · {} · {}",
        if runtime == LocalSessionRuntime::Codex {
            "Codex"
        } else {
            "Claude"
        },
        std::path::Path::new(&cwd)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        if runtime == LocalSessionRuntime::Codex {
            native_id
                .get(native_id.len().saturating_sub(6)..)
                .unwrap_or(native_id)
                .to_owned()
        } else {
            host.pid.to_string()
        }
    );
    let title = event["session_title"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&default_title)
        .chars()
        .take(160)
        .collect();
    let (codex_home, codex_can_receive, codex_program, origin) =
        if runtime == LocalSessionRuntime::Codex {
            let codex_home = std::path::absolute(
                provider_home
                    .clone()
                    .map(Ok)
                    .unwrap_or_else(|| native_home("CODEX_HOME", ".codex"))?,
            )?;
            let program = cccc_runtime::resolve_executable_in_path(
                "codex",
                std::env::var("PATH").ok().as_deref(),
            );
            let info = if let Some(program) = &program {
                cccc_runtime::native_codex_session_info(
                    program,
                    &codex_home,
                    std::path::Path::new(&cwd),
                    native_id,
                )
                .await
                .ok()
            } else {
                None
            };
            let ready = info
                .as_ref()
                .map(|info| info.can_receive)
                .or_else(|| program.is_none().then_some(false));
            let origin = info.and_then(|info| info.origin);
            (Some(codex_home), ready, program, origin)
        } else {
            (None, Some(false), None, None)
        };
    let registered = local_sessions::write_with_codex_readiness(
        home,
        NativeSession {
            source_id,
            runtime,
            native_session_id: native_id.into(),
            host,
            cwd,
            title,
            origin,
            revision: uuid::Uuid::new_v4().to_string(),
            codex_home,
            codex_can_receive: codex_can_receive.unwrap_or(false),
            codex_program,
            claude_config_dir: if runtime == LocalSessionRuntime::Claude {
                Some(std::path::absolute(provider_home.map(Ok).unwrap_or_else(
                    || native_home("CLAUDE_CONFIG_DIR", ".claude"),
                )?)?)
            } else {
                None
            },
            pi_agent_dir: None,
            opencode_config_dir: None,
            opencode_server: None,
            retired: None,
        },
        codex_can_receive,
    );
    if let Err(error) = registered
        && !local_sessions::integration_disabled(home, runtime)?
    {
        return Err(error.into());
    }
    Ok(())
}

async fn request(client: &DaemonClient, op: &str, args: Value) -> Result<Value> {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_default(),
        })
        .await?;
    if let Some(error) = response.error {
        bail!("{}: {}", error.code, error.message)
    }
    Ok(Value::Object(response.result))
}

async fn receive(home: &HomeLayout, host_pid: Option<u32>) -> Result<()> {
    if local_sessions::cccc_owned_environment() {
        return Ok(());
    }
    let host = native_host("claude", host_pid, None)?;
    let source_id = format!("claude:{}", host.key());
    let connection_id = uuid::Uuid::new_v4().to_string();
    let client = DaemonClient::new(home.clone()).with_timeout(Duration::from_secs(2));
    let mut attached = false;
    let mut attached_revision = String::new();
    let mut heartbeat_at = tokio::time::Instant::now() - Duration::from_secs(5);
    let mut output = tokio::io::stdout();
    let mut stop = std::pin::pin!(stop_signal());
    let result = async {
        loop {
            tokio::select! {
                ()=&mut stop=>break,
                ()=tokio::time::sleep(Duration::from_millis(200))=>{},
            }
            if !host.alive() {
                break;
            }
            // SessionEnd/SessionStart can temporarily retire lifecycle identity.
            // Waiting for the new native callback never starts a model turn.
            let current = match local_sessions::read(home, &source_id) {
                Ok(current) => current,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    attached = false;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            // A long-lived Claude Monitor survives /clear and /resume. A new
            // conversation revision must cross the true attachment boundary,
            // rather than only renewing its resource-owned heartbeat lease.
            if current.revision != attached_revision {
                attached = false;
            }
            if !attached || heartbeat_at.elapsed() >= Duration::from_secs(5) {
                let op = if attached {
                    "local_session_receiver_heartbeat"
                } else {
                    "local_session_receiver_attach"
                };
                match request(
                    &client,
                    op,
                    json!({"source_id":source_id,"connection_id":connection_id}),
                )
                .await
                {
                    Ok(_) => {
                        attached = true;
                        attached_revision.clone_from(&current.revision);
                        heartbeat_at = tokio::time::Instant::now();
                    }
                    Err(_) => {
                        // Plugin reload may start the replacement before the old
                        // Monitor detaches or its lease expires. Retrying this
                        // idempotent registration never repeats a native message.
                        attached = false;
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                }
            }
            let next = match request(
                &client,
                "local_session_receiver_next",
                json!({"source_id":source_id,"connection_id":connection_id}),
            )
            .await
            {
                Ok(next) => next,
                Err(_) => {
                    attached = false;
                    continue;
                }
            };
            if next["handoff"].is_null() {
                continue;
            }
            let handoff = &next["handoff"];
            // One stdout event per handoff; native Monitor consumes this in order.
            let line = serde_json::to_vec(&json!({"message":handoff["text"]}))?;
            output.write_all(&line).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
            // Never replay stdout if an ACK is lost. The daemon retains and settles
            // the existing delivery claim, then continues with later messages.
            let _ = request(
                &client,
                "local_session_receiver_ack",
                json!({"source_id":source_id,"connection_id":connection_id,
            "handoff_id":handoff["handoff_id"],"state":"accepted"}),
            )
            .await;
        }
        Ok(())
    }
    .await;
    let _ = request(
        &client,
        "local_session_receiver_detach",
        json!({"source_id":source_id,"connection_id":connection_id}),
    )
    .await;
    result
}

async fn stop_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {_=tokio::signal::ctrl_c()=>{},_=signal.recv()=>{}}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
