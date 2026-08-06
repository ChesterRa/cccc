# Rust Implementation and Product Distribution

The stable CCCC product distribution remains `cccc-pair` on PyPI. It keeps Python
as the initial default and, on supported platforms, bundles a private Rust
executable with the same product version behind the public `cccc` launcher. This
lets users evaluate and gradually adopt Rust without giving up Python fallback or
creating a second command on `PATH`.

The repository also publishes an experimental standalone Rust preview for
supported platforms. It shares the product version, data contracts, and public
command, but contains no Python fallback or implementation selector and is not
yet the recommended replacement for the complete pip product.

Prereleases use one canonical product identity and tag such as `v0.4.34-rc2`.
The Python manifest represents that identity as PEP 440 `0.4.34rc2`, while the
Cargo workspace uses SemVer `0.4.34-rc2`; release validation normalizes those
ecosystem-specific spellings before comparing them.

## Install and update

Install the stable product distribution from PyPI:

```bash
python -m pip install -U cccc-pair
```

Supported platform wheels include both implementations. Other platforms receive
the universal Python wheel and report Rust as unavailable.

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

In either distribution, use:

```bash
cccc update
cccc update --check
```

Standalone Rust previews update through the same GitHub Pages installer. Pip
installations update the complete `cccc-pair` product, including the private Rust
payload on supported platforms.

The public Python launcher owns implementation selection inside a pip install:

```bash
cccc status            # selected, running, and available implementations
cccc rust              # persist Rust and launch
cccc python             # persist Python and launch
cccc rust doctor        # persist Rust, then run one command
```

Selection is stored atomically in `CCCC_HOME/implementation.json`; no file means
Python, preserving existing installations. Before selecting Rust, the launcher
requires an executable payload whose normalized SemVer exactly matches the
installed Python product version. A selector stops the active Web process and
daemon before persisting the new implementation. Missing, corrupt, or mismatched
payloads fail explicitly and never fall back silently.

Inside a pip installation, `cccc update` always upgrades `cccc-pair` through pip.
This keeps the launcher, Python implementation, Rust payload, Web assets, and
contracts on one version. The launcher stops the active Web/daemon pair before
replacement so Windows can replace the native executable safely. The private
Rust binary cannot overwrite its containing wheel independently. The standalone
preview contains Rust only, so `cccc python` and implementation switching are
intentionally unavailable there.

The release publishes one source distribution, one universal Python fallback
wheel, and native wheels for Linux x86-64, Intel macOS, Apple Silicon macOS, and
Windows x86-64. Linux is rebuilt at the manylinux 2.28 baseline and repaired with
`auditwheel`; macOS and Windows dependencies are checked and repaired with
`delocate` and `delvewheel`. Each platform job installs its completed wheel,
switches both ways, and verifies that Rust `setup` records the stable public
launcher rather than the private payload path. Unsupported platforms receive the
universal wheel and report Rust as unavailable.

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

The installed `cccc` launcher selects the implementation and owns replacement of
the active process pair. Python and Rust daemons still must not write the shared
home concurrently. The legacy `ccccd` executable is retained only as a launcher-
backed compatibility alias, so it follows the same selection instead of forcing
the Python daemon.

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
runtime artifact.

`cccc space auth status|start|cancel|disconnect` uses the local Rust Web API for
NotebookLM authentication. IM start requests sent directly to the daemon are
delegated to the Web-owned integration worker, preserving one lifecycle owner.
`cccc doctor` reports daemon identity/version, PTY support, browser discovery,
and Linux display helpers so installation failures are visible from the CLI.

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
- The universal fallback and all four native wheels build and pass package metadata, platform-tag, dependency-repair, and release-set checks. Installed Python/Rust implementation smoke jobs are intentionally retired from the release path to keep publication bounded.
- Python, Cargo, the lockfile, and the Git tag resolve to one release identity.
- The native binary runs without a Python backend dependency.
- The cross-language persisted-state tests pass in their dedicated interop job.
- PyPI publication happens once, only after the complete artifact set is collected.
- Existing `~/.cccc` data remains available after switching implementations.
