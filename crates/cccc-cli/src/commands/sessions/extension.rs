//! Native extensions own input admission; this child owns one fenced receive lease.
use super::*;
use tokio::io::{AsyncBufReadExt, BufReader};

pub(super) async fn bridge(
    home: &HomeLayout,
    runtime: &str,
    host_pid: u32,
    session_id: &str,
    cwd: &std::path::Path,
    provider_home: &std::path::Path,
    backend_pid: Option<u32>,
) -> Result<()> {
    if local_sessions::cccc_owned_environment() {
        return Ok(());
    }
    let runtime = match runtime {
        "pi" => LocalSessionRuntime::Pi,
        "opencode" => LocalSessionRuntime::Opencode,
        _ => bail!("Unsupported native extension runtime"),
    };
    if runtime == LocalSessionRuntime::Pi {
        uuid::Uuid::parse_str(session_id).context("Pi supplied an invalid session ID")?;
    } else {
        anyhow::ensure!(
            session_id.starts_with("ses_")
                && session_id.len() <= 256
                && session_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "OpenCode supplied an invalid session ID"
        );
    }
    anyhow::ensure!(
        cwd.is_absolute() && cwd.is_dir(),
        "Native working directory is unavailable"
    );
    anyhow::ensure!(
        provider_home.is_absolute(),
        "Native configuration directory must be absolute"
    );
    let host = NativeHost::parent(std::process::id())?;
    anyhow::ensure!(
        host.pid == host_pid,
        "The extension host is not this bridge's native parent"
    );
    let backend = if runtime == LocalSessionRuntime::Opencode {
        Some(NativeHost::capture(
            backend_pid.context("OpenCode did not supply its backend PID")?,
        )?)
    } else {
        None
    };
    let source_key = if runtime == LocalSessionRuntime::Opencode {
        session_id.to_owned()
    } else {
        host.key()
    };
    let source_id = runtime.source_id(&source_key);
    let session = local_sessions::write(
        home,
        NativeSession {
            source_id: source_id.clone(),
            runtime,
            native_session_id: session_id.into(),
            host: host.clone(),
            cwd: cwd.to_string_lossy().into_owned(),
            title: format!(
                "{} · {} · {}",
                if runtime == LocalSessionRuntime::Pi {
                    "Pi"
                } else {
                    "OpenCode"
                },
                cwd.file_name().unwrap_or_default().to_string_lossy(),
                &session_id[session_id.len().saturating_sub(6)..]
            ),
            origin: None,
            revision: uuid::Uuid::new_v4().to_string(),
            codex_home: None,
            codex_can_receive: false,
            codex_program: None,
            claude_config_dir: None,
            pi_agent_dir: (runtime == LocalSessionRuntime::Pi).then(|| provider_home.into()),
            opencode_config_dir: (runtime == LocalSessionRuntime::Opencode)
                .then(|| provider_home.into()),
            opencode_server: backend,
            retired: None,
        },
    )?;
    let connection_id = uuid::Uuid::new_v4().to_string();
    let binding = json!({"source_id":source_id,"connection_id":connection_id});
    let client = DaemonClient::new(home.clone()).with_timeout(Duration::from_secs(2));
    let mut output = tokio::io::stdout();
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let mut stop = std::pin::pin!(stop_signal());
    let mut attached = false;
    let mut heartbeat = tokio::time::Instant::now() - Duration::from_secs(5);
    let run: Result<()> = async {
        output.write_all(b"{\"type\":\"ready\"}\n").await?;
        output.flush().await?;
        loop {
            tokio::select! {
                () = &mut stop => break,
                line = input.next_line() => {
                    let Some(line) = line? else { break };
                    let ack: Value = serde_json::from_str(&line).context("Invalid native admission receipt")?;
                    let id = ack["handoff_id"].as_str().context("Native admission receipt has no handoff ID")?;
                    let state = ack["state"].as_str().filter(|v| matches!(*v,"accepted"|"rejected"|"ambiguous")).context("Invalid native admission state")?;
                    // A lost ACK is never a reason to send native input a second time.
                    let _ = request(&client,"local_session_receiver_ack",json!({
                        "source_id":source_id,"connection_id":connection_id,"handoff_id":id,"state":if state=="rejected" {"failed"} else {state},
                        "reason":if state=="rejected" {Some("The native runtime refused this input")} else {None},
                    })).await;
                }
                () = tokio::time::sleep(Duration::from_millis(200)) => {
                    if !host.alive() || session.opencode_server.as_ref().is_some_and(|server|!server.alive()) { break }
                    let current = match local_sessions::read(home, &source_id) {
                        Ok(current) => current,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                        Err(error) => return Err(error.into()),
                    };
                    if current.revision != session.revision { break }
                    if !attached || heartbeat.elapsed() >= Duration::from_secs(5) {
                        let op = if attached {"local_session_receiver_heartbeat"} else {"local_session_receiver_attach"};
                        if request(&client,op,binding.clone()).await.is_err() {
                            attached=false;
                            continue;
                        }
                        attached=true;
                        heartbeat=tokio::time::Instant::now();
                    }
                    let next = match request(&client,"local_session_receiver_next",binding.clone()).await {
                        Ok(next)=>next,
                        Err(_)=>{attached=false;continue;}
                    };
                    if next["handoff"].is_null() {continue}
                    let line=serde_json::to_vec(&json!({"type":"handoff","handoff":next["handoff"]}))?;
                    output.write_all(&line).await?;
                    output.write_all(b"\n").await?;
                    output.flush().await?;
                }
            }
        }
        Ok(())
    }.await;
    // Retire before waiting for daemon cleanup, fenced against a reloaded context.
    let retired = local_sessions::retire_revision(
        home,
        &source_id,
        session_id,
        &host,
        Some(&session.revision),
        RetireReason::SessionEnd,
    );
    let _ = request(&client, "local_session_receiver_detach", binding).await;
    run?;
    retired?;
    Ok(())
}
