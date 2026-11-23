use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum SessionNodeAction {
    /// Host an encrypted device-session gateway and print one pairing ticket.
    Host {
        /// UDP port. Omit to let the OS choose an available port.
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    /// Connect this computer's existing Codex/Claude sessions to a hub.
    #[command(
        after_help = "To pair Codex, run cccc sessions node connect --runtime codex --ticket-stdin. For Claude Code, use --runtime claude. To reconnect, run cccc sessions node connect."
    )]
    Connect {
        /// Read one pairing ticket from stdin; omit to resume the saved pairing.
        #[arg(long)]
        ticket_stdin: bool,
        /// Enable this native integration before pairing; does not filter exported sessions.
        #[arg(long, value_parser = ["codex", "claude"], requires = "ticket_stdin")]
        runtime: Option<String>,
        /// Name shown on the hub (defaults to this computer's hostname).
        #[arg(long)]
        name: Option<String>,
    },
    /// Reconnect an existing hub binding or paste a device pairing ticket in this terminal.
    #[command(hide = true)]
    Join {
        /// Previously paired hub origin (HTTPS, or HTTP on loopback).
        #[arg(value_name = "HUB_URL")]
        hub: String,
        /// Device name shown to the hub administrator (defaults to this computer's hostname).
        #[arg(long)]
        name: Option<String>,
        /// Enable this native integration before connecting.
        #[arg(long, value_parser = ["codex", "claude"])]
        runtime: Option<String>,
        /// Accepted for compatibility. Pairing takes place in the terminal.
        #[arg(long, hide = true)]
        no_browser: bool,
        /// Read a fresh pairing ticket even when this hub has a saved binding.
        #[arg(long)]
        reauthorize: bool,
    },
    /// Inspect paired devices without exposing keys or pairing secrets.
    Status,
    /// Revoke a device. Existing sessions keep running but lose hub access.
    Revoke {
        #[arg(long)]
        node_id: String,
    },
}
