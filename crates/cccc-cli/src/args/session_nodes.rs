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
    Connect {
        /// Read one pairing ticket from stdin; omit to resume the saved pairing.
        #[arg(long)]
        ticket_stdin: bool,
        /// Enable this native integration before pairing; does not filter exported sessions.
        #[arg(long, value_parser = ["codex", "claude"], requires = "ticket_stdin")]
        runtime: Option<String>,
        /// Name shown on the hub for this computer.
        #[arg(long, default_value = "Remote computer")]
        name: String,
    },
    /// Join this hub after browser approval, or reconnect an already approved device.
    Join {
        /// Trusted hub origin (HTTPS, or HTTP on loopback).
        #[arg(value_name = "HUB_URL")]
        hub: String,
        /// Device name shown to the hub administrator (defaults to this computer's hostname).
        #[arg(long)]
        name: Option<String>,
        /// Enable this native integration after approval; does not start the native app.
        #[arg(long, value_parser = ["codex", "claude"])]
        runtime: Option<String>,
        /// Print the confirmation link and code without opening a browser.
        #[arg(long)]
        no_browser: bool,
        /// Ask the administrator for a new approval even if this hub was already paired.
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
