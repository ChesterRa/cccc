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

Ports communicate with the daemon through the versioned IPC contract. Ledger
writes remain daemon-owned. Group documents and global settings use shared
cross-process transaction locks so daemon operations and Web-owned integration
lifecycle updates cannot overwrite each other.

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
The Rust preamble follows the Python contract: cold-start and resumed sessions
are told to call `cccc_bootstrap`, which returns group, inbox, recovery, and
context state. Ordinary chat deliveries do not duplicate the full context JSON.
`CCCC_HOME/groups/<group_id>/prompts/CCCC_PREAMBLE.md` replaces the default
Startup body when present, matching the Python override behavior.
Each delivered chat batch also ends with Python's MCP reply reminder; batched
messages receive one reminder for the whole batch rather than one per message.

`runner=headless` is a structured MCP lifecycle and never creates a PTY. The
runtime pulls an ordered batch with `cccc_runtime_wait_next_turn`, receives the
raw events, coalesced text, and session system prompt, then commits the exact
contiguous event prefix with `cccc_runtime_complete_turn`. The legacy
`web_model_runtime_*` daemon operation names remain accepted for compatibility.
PTY delivery workers explicitly exclude structured actors, preventing duplicate
terminal and cursor-based delivery.

Cursor, Kilo, and Antigravity PTY sessions receive an idempotent MCP setup
contract before the normal preamble. It first checks for `cccc_bootstrap` and
only installs the `cccc mcp` stdio server when unavailable. Custom PTY runtimes
receive the same identity, group, bootstrap, and reply protocol preamble as
built-in runtimes. Voice Secretary system notifications include the complete
`input_envelope` or action-request envelope in the delivered payload instead of
only the generic notification title.

Delivery completion advances the inbox only across a fully delivered contiguous
prefix. Resolution scans the ledger index from the actor cursor, so batches over
the former 1000-event read window neither leave stale unread entries nor skip an
undelivered event.

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
