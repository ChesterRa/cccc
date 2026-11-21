//! A public hub URL starts approval; only the existing native connector commits a grant.
use super::{connect, ensure_daemon};
use anyhow::{Context, Result, bail, ensure};
use cccc_contracts::session_node::SessionNodeTicket;
use cccc_contracts::session_node_join::{
    CreateRequest, CreatedRequest, JOIN_POLL_SECONDS, JOIN_TTL_SECONDS, JoinStatus, PollRequest,
    PollResponse,
};
use cccc_core::HomeLayout;
use cccc_web::session_node::{JoinTarget, decode_ticket, stop_signal};
use chrono::{DateTime, Utc};
use std::{fmt::Write as _, process::Stdio, time::Duration};
use tokio::time::Instant;

#[path = "node_join_http.rs"]
mod http;

pub(super) async fn run(
    home: &HomeLayout,
    hub: &str,
    name: Option<String>,
    no_browser: bool,
    reauthorize: bool,
    runtime: Option<&str>,
) -> Result<()> {
    let name = device_name(name)?;
    let target = JoinTarget::prepare(home, hub)?;
    let resume = !reauthorize && target.can_resume();
    let ticket = if resume {
        eprintln!(
            "Resuming this computer's approved connection to {}. If an administrator revoked this device, use --reauthorize to request approval again.",
            target.origin()
        );
        None
    } else {
        tokio::select! {
            () = stop_signal() => {
                eprintln!("Device join cancelled.");
                return Ok(());
            }
            ticket = approval(&target, &name, no_browser) => Some(ticket?),
        }
    };
    target.check_current()?;
    ensure_daemon(home).await?;
    if let Some(runtime) = runtime {
        connect::enable_integration(home, runtime, target.needs_relay(ticket.as_ref())).await?;
    }
    eprintln!("Connecting to the hub. Keep this terminal running.");
    let result = target.connect(ticket, &name).await;
    let result = if resume {
        result.context("The saved device connection did not complete. If the hub revoked access, run join again with --reauthorize")
    } else {
        result
    };
    connect::result_after_setup(result, runtime)
}

async fn approval(target: &JoinTarget, name: &str, no_browser: bool) -> Result<SessionNodeTicket> {
    let http = target.approval_http_client()?;
    let started = Instant::now();
    let request = CreateRequest {
        node_id: target.node_id(),
        name: name.into(),
        device_code: new_device_code()?,
    };
    eprintln!("Requesting device approval from {}.", target.origin());
    let created: CreatedRequest = http::post(
        &http,
        target.origin(),
        http::Operation::Create,
        &request,
        started + Duration::from_secs(30),
    )
    .await?;
    ensure!(
        uuid::Uuid::parse_str(&created.request_id)
            .is_ok_and(|id| id.get_version_num() == 4 && id.to_string() == created.request_id)
            && created.user_code.len() == 8
            && created
                .user_code
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)),
        "The hub returned invalid confirmation details. Check the hub URL and retry"
    );
    let expiry = DateTime::parse_from_rfc3339(&created.expires_at)
        .map_err(|_| anyhow::anyhow!("The hub returned an invalid approval expiry"))?;
    let ttl = Duration::from_secs(JOIN_TTL_SECONDS);
    let remaining = expiry
        .signed_duration_since(Utc::now())
        .to_std()
        .unwrap_or_default()
        .min(ttl);
    let deadline = (Instant::now() + remaining).min(started + ttl);
    ensure!(
        deadline > Instant::now(),
        "Device approval expired. Run join again for a new request"
    );

    let mut confirmation = target.origin().clone();
    confirmation
        .query_pairs_mut()
        .append_pair("session_node_join", &created.request_id);
    eprintln!(
        "Confirmation link: {confirmation}\nDevice: {name}\nVerification code: {}\nWaiting for a hub administrator to approve this computer.",
        created.user_code
    );
    if !no_browser {
        open_confirmation(confirmation);
    }
    let poll = PollRequest {
        request_id: created.request_id,
        device_code: request.device_code,
    };
    loop {
        let status: PollResponse = http::post(
            &http,
            target.origin(),
            http::Operation::Poll,
            &poll,
            deadline,
        )
        .await?;
        match status.status {
            JoinStatus::Pending => {
                tokio::time::sleep_until(
                    (Instant::now() + Duration::from_secs(JOIN_POLL_SECONDS)).min(deadline),
                )
                .await;
            }
            JoinStatus::Approved => {
                let ticket = status.ticket.as_deref().and_then(|value| decode_ticket(value).ok())
                    .context("The hub returned an invalid approval. Update the hub and request approval again")?;
                eprintln!("Device approved.");
                return Ok(ticket);
            }
            JoinStatus::Denied => bail!(
                "Device approval was denied by the hub. Ask its administrator before trying again"
            ),
            JoinStatus::Expired => {
                bail!("Device approval expired. Run join again for a new request")
            }
        }
    }
}

fn new_device_code() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Cannot generate a private device join code"))?;
    let mut code = String::with_capacity(64);
    for byte in bytes {
        write!(&mut code, "{byte:02x}")?;
    }
    Ok(code)
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

fn open_confirmation(url: reqwest::Url) {
    tokio::spawn(async move {
        #[cfg(target_os = "macos")]
        let mut cmd = cccc_runtime::background_tokio_command("/usr/bin/open");
        #[cfg(windows)]
        let mut cmd = {
            let root = std::env::var_os("SystemRoot")
                .map(std::path::PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
            let mut launcher =
                cccc_runtime::background_tokio_command(root.join("System32").join("rundll32.exe"));
            launcher.arg("url.dll,FileProtocolHandler");
            launcher
        };
        #[cfg(not(any(target_os = "macos", windows)))]
        let mut cmd = cccc_runtime::background_tokio_command("xdg-open");
        let result = cmd
            .arg(url.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        if !result.is_ok_and(|status| status.success()) {
            eprintln!(
                "Could not open a browser. Open the confirmation link above on a trusted device."
            );
        }
    });
}
