# Rust Backend Migration

The `rust` branch replaces the Python backend while keeping the React/TypeScript
frontend and its external product contracts stable.

## Data compatibility

The Rust and Python implementations use the same `CCCC_HOME` and default to
`~/.cccc`.

- Rust configuration: `CCCC_HOME`
- Rust default: `~/.cccc`
- Python default: `~/.cccc`

The registry, group documents, ledgers, and actor contracts are shared. On first
Rust startup, CCCC validates the existing layout and adds a `.cccc-rust-v1`
compatibility marker without moving or deleting existing files. A non-empty
directory that is not already a CCCC home is still rejected.
Python-format access-token entries keep the raw token as the map key; Rust reads
and writes that layout without adding duplicate token fields.
Wrapped `tokens:` documents and the older top-level token map are both accepted.
Custom token values are percent-encoded for Cookie and EventSource transport and
decoded before lookup.
Python blob names in `<sha256>_<safe-filename>` form remain readable alongside
the Rust hash-only form, with path and symlink escape checks applied to both.
Plain `.jsonl` and Python `.jsonl.gz` ledger segments are read in the same event
order. Rust compaction writes `ledger.<UTC>.<sequence>.jsonl` files and updates
the Python manifest contract, so either implementation can read new segments.

Switching Git branches selects the implementation. Stop the active daemon before
switching; Python and Rust daemons must not write the shared home concurrently.

## Dependency boundaries

```text
cccc-contracts <- cccc-core <- cccc-daemon
cccc-contracts <- cccc-client <- cccc-cli
```

Ports communicate with the daemon through the versioned IPC contract. The
daemon is the only writer for group state and ledgers.

## Runtime recovery and delivery

`group.running` stores the operator's desired runtime state. API group summaries
also expose `runtime_status`, which is derived from live actor sessions. On
daemon startup, enabled local actors in groups whose desired state is running
are restored before the daemon publishes its IPC address.

Actor-bound chat messages and system notifications use one bounded FIFO worker
per actor. A worker seeds the runtime with its CCCC system prompt once per
session, preserves message order, uses bracketed paste when the terminal enables
it, and applies the actor's configured submit mode. Successful delivery returns
to the daemon's serialized state path before advancing the inbox cursor.

Daemon connections are read concurrently with a size limit and timeout. State
operations remain serialized behind the dispatch lock, so a slow or malformed
client cannot block the listener or introduce concurrent group writers.
Daemon shutdown stops every local runtime session before releasing the shared
lock. The combined `cccc` process also closes Web after daemon loss. Rust daemon
reuse requires matching implementation, package version, and compatibility ID;
legacy or stale daemons are replaced through graceful shutdown.

## Migration completion gate

- Rust owns CLI, daemon, kernel, MCP, Web API, runners, and integrations.
- The existing Web UI builds unchanged against the Rust HTTP/WebSocket surface.
- Linux, macOS, and Windows builds and platform smoke tests pass.
- The final runtime and Docker image contain no Python backend.
- Existing `~/.cccc` data remains available after switching implementations.
