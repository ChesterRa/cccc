//! Optional native setup composes existing operations; pairing remains interactive.
use super::ensure_daemon;
use crate::commands::sessions::setup;
use anyhow::{Context, Result, ensure};
use cccc_contracts::session_node::SessionNodeTicket;
use cccc_core::HomeLayout;
use cccc_web::session_node::JoinTarget;
use tokio::io::{AsyncBufReadExt, AsyncReadExt};

pub(super) async fn run(
    home: &HomeLayout,
    legacy_hub: Option<&str>,
    ticket_stdin: bool,
    name: Option<String>,
    runtime: Option<&str>,
) -> Result<()> {
    let name = device_name(name)?;
    let target = match legacy_hub {
        Some(hub) => JoinTarget::prepare(home, hub)?,
        None => JoinTarget::local(home)?,
    };
    let ticket =
        read_ticket(ticket_stdin || (legacy_hub.is_some() && !target.can_resume())).await?;
    ensure!(
        ticket.is_some() || target.can_resume(),
        "Pairing ticket required for the first connection. Run cccc sessions node connect --runtime codex --ticket-stdin (use claude for Claude Code) and paste a ticket from the hub administrator"
    );
    target.check_current()?;
    ensure_daemon(home).await?;
    if let Some(runtime) = runtime {
        if target.needs_relay(ticket.as_ref()) {
            cccc_web::session_node::verify_daemon(home, true).await?;
        }
        setup::enable(home, runtime)
            .await
            .context("Native setup failed; device pairing was not attempted. Inspect native configuration before retrying")?
            .print()?;
        eprintln!(
            "Native {runtime} integration enabled; device connection has not completed yet. Native hooks still require your review; no native session was started."
        );
    }
    eprintln!("Connecting to the hub. Keep this terminal running.");
    let result = target.connect(ticket, &name).await;
    if let Some(runtime) = runtime {
        result.with_context(|| format!(
            "Native {runtime} integration remains enabled; device connection did not complete. No automatic disable was performed. Resolve the connection error before retrying"
        ))
    } else {
        result
    }
}

async fn read_ticket(requested: bool) -> Result<Option<SessionNodeTicket>> {
    if !requested {
        return Ok(None);
    }
    eprintln!(
        "Paste the hub's session-node ticket, then press Enter (do not put it in the command):"
    );
    let mut line = String::new();
    tokio::io::BufReader::new(tokio::io::stdin().take(16 * 1024 + 1))
        .read_line(&mut line)
        .await?;
    // Deserialization diagnostics can include invalid field values. Never echo
    // any part of the supplied capability through an error chain or a log.
    cccc_web::session_node::decode_ticket(&line)
        .map(Some)
        .map_err(|_| anyhow::anyhow!("Invalid or missing session-node pairing ticket; paste a fresh device ticket at the prompt"))
}

fn device_name(explicit: Option<String>) -> Result<String> {
    fn valid(s: &str) -> bool {
        !s.is_empty()
            && s.len() <= 160
            && !s.chars().any(|c| {
                c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
    }
    if let Some(value) = explicit {
        let name = value.trim();
        ensure!(
            valid(name),
            "Choose a nonempty device name of up to 160 bytes without control characters"
        );
        return Ok(name.into());
    }
    #[cfg(unix)]
    let host = nix::unistd::gethostname()
        .ok()
        .map(|name| name.to_string_lossy().into_owned());
    #[cfg(not(unix))]
    let host = std::env::var("COMPUTERNAME").ok();
    Ok(host
        .as_deref()
        .map(str::trim)
        .filter(|value| valid(value))
        .unwrap_or("Remote computer")
        .into())
}
