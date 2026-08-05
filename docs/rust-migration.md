# Rust Backend Migration

The `rust` branch replaces the Python backend while keeping the React/TypeScript
frontend and its external product contracts stable.

## Rust package distribution

The installable crates.io package is `cccc`; its executable is also `cccc`.
The initial registry bootstrap release is installed with:

```bash
cargo install cccc
```

Existing crates.io installations can be upgraded in place:

```bash
cccc update
cccc update --check
```

The Rust updater runs `cargo install cccc --force --locked`. It requires Cargo
to remain available and stops an older running CCCC daemon only after the new
binary has installed successfully.

Implementation crates are published under the `cccc-pair-*` namespace so the
public package name stays simple while Rust module imports remain unchanged.
Normal product releases continue to use the workspace version; the `0.0.x`
releases only reserve and validate the new crates.io distribution path.

Rust release packaging runs `scripts/prepare_rust_web_assets.mjs` before Cargo.
The generated `crates/cccc-web/assets/web-dist/` directory is intentionally
ignored by Git, but `cccc-pair-web` explicitly includes it in the crate archive.
This keeps hashed frontend bundles out of source commits while preserving a
Node.js-free install from crates.io.

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
Ledger reads also normalize the pre-v1 `chat.ack` envelope (`type`, `event_id`,
and `agent`) into the current versioned event contract. Other unrecognized or
malformed historical lines are reported with their source location and skipped,
so one legacy record cannot make an entire group unavailable.

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

The Rust MCP server uses the same progressive tool surface as Python.
`tools/list` is derived from caller role and `capability_state`, includes
enabled built-in packs and Python-compatible external MCP runtime artifacts,
and forwards dynamic tool calls through `capability_tool_call`. A shared parity
test guards the static Python and Rust tool-name catalogs.

Group Bridge compatibility includes daemon-level `remote_send`,
`remote_delivery_status`, and `group_bridge_receive_remote_send` operations in
addition to the Web and MCP routes. Remote delivery requires an explicit
recipient, validates the active registration or trust route, records idempotent
receipts, and falls back to the remote Group Bridge MCP endpoint when needed.
The Rust daemon also owns Python-compatible signed outbound WebSocket sessions:
it scans active trusts, maintains heartbeats, reconnects with bounded
exponential backoff, projects connection health onto each trust, and prefers
the live route for message delivery before HTTP/MCP fallback.

The Rust NotebookLM adapter owns notebook sources, Studio artifact
create/list/download operations, and incremental work/memory synchronization.
Sync hashes local text files, replaces changed remote sources, removes deleted
sources, and persists convergence state in the group-space document.

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

`runner=headless` never creates a PTY. Codex and Claude use daemon-managed local
provider sessions: Codex app-server JSON-RPC and Claude bidirectional
stream-json. Their messages are pushed through bounded actor delivery workers,
and actor health comes from the real provider process. Web Model and custom
external headless actors retain the pull contract: the executor obtains an
ordered batch with `cccc_runtime_wait_next_turn` and commits its exact contiguous
event prefix with `cccc_runtime_complete_turn`. The legacy
`web_model_runtime_*` daemon operation names remain accepted for compatibility.

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
