# Rust Implementation and Product Distribution

The maintained Python compatibility distribution remains `cccc-pair` on PyPI.
The repository separately publishes an experimental standalone Rust preview for
supported platforms. Both share the product version, data contracts, and public
command, while the standalone artifact contains no Python fallback or
implementation selector. Replacement readiness is enforced by source-level
parity tests plus binary and manual installed-artifact smoke checks; promotion
from preview remains a release decision, not an implementation fallback.

Prereleases use one canonical product identity and tag such as `v0.4.34-rc2`.
The Python manifest represents that identity as PEP 440 `0.4.34rc2`, while the
Cargo workspace uses SemVer `0.4.34-rc2`; release validation normalizes those
ecosystem-specific spellings before comparing them.

## Install and update

Install the stable product distribution from PyPI:

```bash
python -m pip install -U cccc-pair
```

PyPI publishes a portable Python wheel and source distribution. The current
Python package does not bundle a private Rust executable.

To explicitly evaluate the experimental standalone Rust preview without a Rust
or Python toolchain:

```bash
# macOS / Linux
curl -fsSL https://chesterra.github.io/cccc/install.sh | sh

# Windows PowerShell
irm https://chesterra.github.io/cccc/install.ps1 | iex
```

The GitHub Pages scripts pin the product version represented by the current
documentation build, select the current platform archive, validate `SHA256SUMS`,
and install into a user-owned directory. They refuse to overwrite a public
`cccc` executable not carrying the standalone ownership marker; uninstall that
command deliberately, choose another
`CCCC_INSTALL_DIR`, or set `CCCC_ALLOW_REPLACE_EXISTING=1` only when replacement
is intentional. The initial experimental preview is `0.4.34-rc2`; callers can
override the documentation pin through `CCCC_VERSION`.

In either distribution, inspect or apply updates with:

```bash
cccc update
cccc update --check
```

Standalone Rust previews update through the GitHub Pages installer and support
`--channel stable|rc`; the updater resolves and pins a concrete release version
before invoking the installer. Pip installations update `cccc-pair` through pip.
Legacy Python installations that already contain a private Rust payload retain
their implementation-selector compatibility, but the portable wheel does not
create that payload. The standalone preview is always Rust-only.

The Python release publishes one source distribution and one portable wheel.
The standalone workflow builds native archives for Linux x86-64, Intel macOS,
Apple Silicon macOS, and Windows x86-64, reusing one Web bundle. Final native
installation smoke checks are manual rather than separate release jobs.

Cargo remains a workspace development tool and the crates stay non-publishable.
The experimental standalone workflow builds and verifies all four supported
archives on manual runs. A matching pushed tag additionally publishes those
preview archives, checksums, and versioned installers to GitHub Releases with an
explicit experimental notice; prerelease tags are marked as such in GitHub
Releases.

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

Python and Rust daemons must not write the shared home concurrently. Legacy
bundled installations retain their selector metadata, while a standalone
installation always launches Rust. The legacy `ccccd` executable remains only a
launcher-backed compatibility alias.

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
test guards the static Python and Rust tool-name catalogs. Enabling an external
capability now performs the Python-compatible package preflight and installation
for npm, PyPI, OCI, command, and remote HTTP MCP records before persisting the
runtime artifact. Static tools and their complete input schemas now come from one
packaged JSON contract consumed by both implementations. `cccc_code_exec` and
`cccc_code_wait` use the same isolated JavaScript runtime, actor ownership,
yield/wait/terminate lifecycle, output bounds, and per-actor cell store. Node.js
must be available on the host when code mode is used; it is an execution-engine
dependency, not a Python backend dependency.

ReMe operations now preserve the Python context-compaction boundary (including
split turns), structured memory metadata, daily-flush signal budgets, semantic
deduplication, source filtering, search controls, idempotency, supersession, and
the single daily shadow write for durable memory entries.

`cccc space auth status|start|cancel|disconnect` uses the local Rust Web API for
NotebookLM authentication. IM start requests sent directly to the daemon are
delegated to the Web-owned integration worker, preserving one lifecycle owner.
`cccc doctor` reports daemon identity/version, PTY support, browser discovery,
and Linux display helpers so installation failures are visible from the CLI.
Linux Web Model projection requires Xvfb and fails with an actionable error when
it is absent; it no longer silently changes behavior by falling back to a
headless browser.

The Rust CLI accepts the Python public spellings for `prompt --actor-id`,
`tail --lines`, `doctor --all`, `runtime list --all`, `update --channel`, and the
`space jobs` / `space auth` subcommand trees. Standalone `cccc status` succeeds
while the daemon is stopped and identifies the Rust-only installation instead of
turning an expected offline state into a command failure.

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

Before starting an automatically managed PTY actor, Rust now applies Python's
runtime MCP readiness contract. CLI-backed and configuration-backed runtimes
are classified as `ready`, `missing`, or `stale`; missing or safely replaceable
entries are installed, then verified before the provider process is created.
This covers Claude, Cline, Copilot, Devin, Kiro, Droid, Amp, Auggie, Grok,
Hermes, Kimi, and OpenCode. Codex continues to receive its actor-scoped command
line override, while OpenCode receives an inline launch configuration.
More-specific stale entries
that CCCC does not own are reported rather than overwritten. This prevents an
old Python launcher path or dangling symlink from freezing a newly created
provider session without CCCC tools.

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

## Unified release gate

A release is publishable only when all of these remain true:

- Rust owns its CLI, daemon, kernel, MCP, Web API, runners, and integrations.
- The existing Web UI builds unchanged against the Rust HTTP/WebSocket surface.
- The portable Python wheel and source distribution pass package metadata and
  install checks; all four standalone native archives and their checksum set are
  complete.
- CI runs offline status, daemon lifecycle, MCP initialization, and a real
  `cccc_code_exec` cell against the built Rust binary. Manual final-installer
  verification repeats the complete Unix flow; Windows verifies installed
  offline status, MCP startup, daemon lifecycle, and executable release after
  shutdown.
- Python, Cargo, the lockfile, and the Git tag resolve to one release identity.
- The native binary runs without a Python backend dependency.
- The cross-language persisted-state tests pass in their dedicated interop job.
- PyPI publication happens once after the portable Python artifacts pass their
  gates; standalone native assets are published by their dedicated workflow.
- Existing `~/.cccc` data remains available after switching implementations.

Credentialed live-provider canaries (NotebookLM/Google browser auth and external
IM vendors) remain environment-owned release checks: source and installed-binary
gates cannot synthesize those third-party accounts. Their absence is reported as
a live-validation blocker, never treated as proof that the provider path passed.
