//! Optional native setup composes existing operations; pairing remains interactive.
use super::ensure_daemon;
use crate::commands::sessions::setup;
use anyhow::{Context, Result, ensure};
use cccc_contracts::session_node::SessionNodeTicket;
use cccc_core::{HomeLayout, session_nodes};
use tokio::io::{AsyncBufReadExt, AsyncReadExt};

pub(super) async fn run(
    home: &HomeLayout,
    ticket_stdin: bool,
    name: &str,
    runtime: Option<&str>,
) -> Result<()> {
    // Existing connect/reconnect does not configure either native application.
    if runtime.is_none() {
        ensure_daemon(home).await?;
    }
    let ticket = read_ticket(ticket_stdin).await?;
    if let Some(runtime) = runtime {
        ensure!(
            !session_nodes::saved_client(home)?.is_some_and(|client| client.owner.alive()),
            "A native session-node connector already runs for this Home; no native configuration was changed"
        );
        ensure_daemon(home).await?;
        if ticket
            .as_ref()
            .is_some_and(|ticket| ticket.relay_url.is_some())
        {
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
    let result = cccc_web::session_node::connect(home, ticket, name).await;
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
