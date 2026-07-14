# Rust Backend Migration

The `rust` branch replaces the Python backend while keeping the React/TypeScript
frontend and its external product contracts stable.

## Data isolation

The Rust implementation intentionally does not read `CCCC_HOME`.

- Rust configuration: `CCCC_RUST_HOME`
- Rust default: `~/.cccc-rust`
- Python default: `~/.cccc`

Rust refuses to initialize the Python default directory or anything below it.
Every Rust home contains a `.cccc-rust-v1` marker. A non-empty custom directory
without this marker is rejected, preventing accidental adoption of existing data.

Switching Git branches selects the implementation. Do not run Python and Rust
daemons against the same directory.

## Dependency boundaries

```text
cccc-contracts <- cccc-core <- cccc-daemon
cccc-contracts <- cccc-client <- cccc-cli
```

Ports communicate with the daemon through the versioned IPC contract. The
daemon is the only writer for group state and ledgers.

## Migration completion gate

- Rust owns CLI, daemon, kernel, MCP, Web API, runners, and integrations.
- The existing Web UI builds unchanged against the Rust HTTP/WebSocket surface.
- Linux, macOS, and Windows builds and platform smoke tests pass.
- The final runtime and Docker image contain no Python backend.
- No command reads or writes the legacy `~/.cccc` directory.
