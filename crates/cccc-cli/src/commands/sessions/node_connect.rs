//! Optional native setup composes existing operations; pairing remains interactive.
use super::ensure_daemon;
use crate::commands::sessions::setup;
use anyhow::{Context, Result, ensure};
use cccc_contracts::session_node::SessionNodeTicket;
use cccc_core::HomeLayout;
use cccc_session_node::ConnectTarget;
use tokio::io::{AsyncBufReadExt, AsyncReadExt};

pub(super) async fn run(
    home: &HomeLayout,
    ticket_stdin: bool,
    name: Option<String>,
    runtime: Option<&str>,
) -> Result<()> {
    let name = device_name(name)?;
    let target = ConnectTarget::load(home)?;
    let ticket = read_ticket(ticket_stdin).await?;
    ensure!(
        ticket.is_some() || target.can_resume(),
        "Pairing ticket required for the first connection. Run cccc sessions node connect --runtime <codex|claude|pi|opencode> --ticket-stdin and paste a ticket from the hub administrator"
    );
    target.check_current()?;
    let daemon_runtimes = ensure_daemon(home).await?;
    if let Some(runtime) = runtime {
        let daemon_runtimes = if target.needs_relay(ticket.as_ref()) {
            cccc_session_node::verify_daemon(home, true).await?
        } else {
            daemon_runtimes
        };
        ensure!(
            daemon_runtimes
                .iter()
                .any(|supported| supported.as_str() == runtime),
            "The running daemon on this computer does not support {runtime} device sessions. Update CCCC and restart the daemon before enabling this integration"
        );
        // `--runtime` requires `--ticket-stdin`, so a ticket is always in hand here.
        let enabled = setup::enable(home, runtime)
            .await
            .context("Native setup failed; device pairing was not attempted and the pairing ticket was not used. Fix the native setup, then paste the same ticket again before it expires")?;
        eprintln!(
            "Native {runtime} integration enabled; device connection has not completed yet. Native hooks still require your review; no native session was started."
        );
        enabled.print_note();
    }
    eprintln!("Connecting to the hub. Keep this terminal running.");
    let result = target.connect(ticket, &name, runtime).await;
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
    let line = if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        read_hidden_line().await?
    } else {
        eprintln!("{TICKET_PROMPT}");
        let mut line = String::new();
        tokio::io::BufReader::new(tokio::io::stdin().take(TICKET_LIMIT + 1))
            .read_line(&mut line)
            .await?;
        line
    };
    // Deserialization diagnostics can include invalid field values. Never echo
    // any part of the supplied capability through an error chain or a log.
    cccc_session_node::decode_ticket(&line)
        .map(Some)
        .map_err(|_| anyhow::anyhow!("Invalid or missing session-node pairing ticket; paste a fresh device ticket at the prompt"))
}

const TICKET_PROMPT: &str =
    "Paste the hub's session-node ticket, then press Enter (do not put it in the command):";

/// The ticket is a pairing secret: on a terminal, echo is off before the prompt
/// appears, so a paste never reaches the screen or its scrollback. An interrupt
/// restores the terminal before the command ends.
async fn read_hidden_line() -> Result<String> {
    use std::io::{BufRead, Read};
    // Listen first: once echo is off, an interrupt must not end the process
    // before the terminal is restored.
    let mut interrupt = interrupt_signal()?;
    let hidden = EchoOff::new()?;
    eprintln!("{TICKET_PROMPT}");
    // A blocking terminal read cannot be cancelled, so it runs on its own
    // thread and the guard stays here, where cancellation can drop it.
    let (sender, received) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let read = std::io::stdin()
            .lock()
            .take(TICKET_LIMIT + 1)
            .read_line(&mut line)
            .map(|_| line);
        let _ = sender.send(read);
    });
    let line = tokio::select! {
        read = received => read.context("Ticket input stopped")??,
        _ = interrupt.recv() => {
            drop(hidden);
            eprintln!();
            anyhow::bail!("Pairing cancelled");
        }
    };
    drop(hidden);
    if cfg!(windows) {
        eprintln!();
    }
    Ok(line)
}

const TICKET_LIMIT: u64 = 16 * 1024;

#[cfg(unix)]
fn interrupt_signal() -> std::io::Result<tokio::signal::unix::Signal> {
    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
}

#[cfg(windows)]
fn interrupt_signal() -> std::io::Result<tokio::signal::windows::CtrlC> {
    tokio::signal::windows::ctrl_c()
}

/// Turns off only the terminal's echo, keeping its line editing, and restores
/// the exact original mode when dropped.
struct EchoOff {
    #[cfg(unix)]
    original: nix::sys::termios::Termios,
    #[cfg(windows)]
    original: u32,
}

impl EchoOff {
    #[cfg(unix)]
    fn new() -> Result<Self> {
        use nix::sys::termios::{LocalFlags, SetArg, tcgetattr, tcsetattr};
        use std::os::fd::AsFd;
        let stdin = std::io::stdin();
        let original = tcgetattr(stdin.as_fd())?;
        let mut hidden = original.clone();
        hidden.local_flags.remove(LocalFlags::ECHO);
        hidden.local_flags.insert(LocalFlags::ECHONL);
        tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &hidden)?;
        Ok(Self { original })
    }

    #[cfg(windows)]
    fn new() -> Result<Self> {
        const ENABLE_ECHO_INPUT: u32 = 0x0004;
        let console =
            crossterm_winapi::ConsoleMode::from(crossterm_winapi::Handle::input_handle()?);
        let original = console.mode()?;
        console.set_mode(original & !ENABLE_ECHO_INPUT)?;
        Ok(Self { original })
    }
}

impl Drop for EchoOff {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsFd;
            let _ = nix::sys::termios::tcsetattr(
                std::io::stdin().as_fd(),
                nix::sys::termios::SetArg::TCSANOW,
                &self.original,
            );
        }
        #[cfg(windows)]
        if let Ok(handle) = crossterm_winapi::Handle::input_handle() {
            let _ = crossterm_winapi::ConsoleMode::from(handle).set_mode(self.original);
        }
    }
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
