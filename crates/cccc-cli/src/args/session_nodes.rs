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
        /// Name shown on the hub for this computer.
        #[arg(long, default_value = "Remote computer")]
        name: String,
    },
    /// Inspect paired devices without exposing keys or pairing secrets.
    Status,
    /// Revoke a device. Existing sessions keep running but lose hub access.
    Revoke {
        #[arg(long)]
        node_id: String,
    },
}
