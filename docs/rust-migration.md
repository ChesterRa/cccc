# Rust-only Product and Python 0.4.35 Migration

CCCC 0.4.36 has one shipped implementation: Rust. The Python daemon, Web
server, launcher, engine selector, and fallback are retired. Python remains in
the repository only for small build, release, and repository-contract tools;
normal CCCC use does not import or start it.

Version 0.4.35 is the final dual-engine release and the supported migration
boundary. Rust keeps the durable state formats required to adopt a sanitized,
frozen 0.4.35 home, but it does not preserve implementation switching or promise
compatibility with arbitrary older/private Python state.

## Install and update

Install with the website script (recommended):

```bash
# macOS / Linux
curl -fsSL https://chesterra.github.io/cccc/install.sh | sh

# Windows CMD or PowerShell
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; Invoke-RestMethod 'https://chesterra.github.io/cccc/install.ps1' | Invoke-Expression"
```

Or install the same native executable through pip:

```bash
python -m pip install -U "cccc-pair>=0.4.36"
```

PyPI publishes four native platform wheels: Linux x86-64, Intel/Apple Silicon
macOS, and Windows x86-64. Each wheel contains only the native `cccc`
executable, its package-manager ownership marker, license, and package metadata.
The minimum-version constraint keeps
pip from silently selecting a historical Python-only release on an unsupported
platform. There is no 0.4.36 source distribution, portable wheel, importable
CCCC Python package, or fallback implementation.

The GitHub Pages scripts pin the product version represented by the current
documentation build, select the current platform archive, validate `SHA256SUMS`,
and install into a user-owned directory. Rust self-updates require the complete
`.cccc-standalone` ownership marker with the exact value `standalone-v1`, then
identify their exact current executable to the installer and enter the existing
transactional backup and rollback flow. Native pip wheels own the same marker
path with the value `pip-v1`; this replaces stale standalone ownership when pip
uses the same scripts directory and is removed with the wheel on uninstall.
Version output and executable naming are never treated as proof of ownership:
every markerless command, symlink, or foreign ownership marker is protected by
default. Package-manager installs must be updated through their package manager.
Remove a conflicting command, choose another `CCCC_INSTALL_DIR`, or set
`CCCC_ALLOW_REPLACE_EXISTING=1` only when replacement is intentional. It never
overrides `pip-v1`: uninstall the pip package before switching that directory to
the website installer. The hosted
scripts are rendered with the current documentation release; callers can
override that concrete pin through `CCCC_VERSION`.

For a website-installer distribution, inspect or apply updates with:

```bash
cccc update
cccc update --check
```

Website-script installations update through the GitHub Pages installer and
support `--channel stable|rc`; the updater resolves and pins a concrete release
version before invoking the installer. Pip installations stay owned by pip and
must be updated explicitly with
`python -m pip install -U "cccc-pair>=0.4.36"`. Both channels install the same
Rust implementation and expose the same public `cccc` command.

The 0.4.35 selectors `cccc python` and `cccc rust` are retired. A legacy
`CCCC_HOME/implementation.json` is ignored: it is neither a product preference
nor an authority for runtime startup in 0.4.36. `cccc status` reports the one
installed product, daemon state, groups, and detected agent runtimes without
implementation availability rows.

Inside a pip installation, `cccc update` refuses replacement and prints the pip
command. This keeps Windows and virtual-environment files under their package
manager instead of attempting to infer an interpreter or overwrite a running
executable. Standalone ownership is proven only by the complete marker beside
that exact executable.

### Uninstall without removing user data

Before uninstalling, run `cccc home` to record the active data directory and
`cccc daemon stop` if CCCC is running. A pip installation is owned by pip:

```bash
python -m pip uninstall cccc-pair
```

For a default standalone installation on macOS or Linux, remove only the two
files owned by the installer, and only after verifying the complete ownership
marker:

```bash
test "$(cat "$HOME/.local/bin/.cccc-standalone" 2>/dev/null)" = "standalone-v1" && \
  rm "$HOME/.local/bin/cccc" "$HOME/.local/bin/.cccc-standalone"
```

For a custom `CCCC_INSTALL_DIR`, apply the same exact-marker check to that
directory. Do not remove the directory itself: it may contain commands owned by
other tools. The Unix installer may add the general-purpose `~/.local/bin`
directory to the shell PATH; leaving that entry is intentional because other
user commands can depend on it.

On Windows, the default standalone directory is
`%LOCALAPPDATA%\CCCC\bin`. Stop CCCC, verify that
`.cccc-standalone` contains only `standalone-v1`, then remove only `cccc.exe`
and `.cccc-standalone`. Remove that exact directory from the User PATH through
Windows Environment Variables if the standalone installer added it and the
directory is no longer used.

Uninstall does not remove `CCCC_HOME`. Groups, ledgers, credentials, browser
profiles, and settings therefore survive reinstall. Back up and erase that
recorded directory separately only when permanent data deletion is intended.

The released 0.4.35 line uses the six-file dual-engine PyPI set described above.
For the approved 0.4.36 Rust-only consolidation, the canonical release workflow
has already been reduced to one build per supported platform. It wraps the exact
same self-contained executable bytes in one standalone archive and one native
wheel, producing four archives and four wheels with no source distribution or
portable fallback wheel. The compatibility wheel installs `cccc` through the
wheel scripts scheme and contains no importable CCCC Python package or Python
runtime dependencies. Its installed smoke covers 0.4.35-layout cleanup, PATH
ownership, version, MCP discovery, daemon lifecycle, Web health, package-manager
update refusal, reinstall, and uninstall without deleting `CCCC_HOME`.

The standalone Linux x86-64 artifact is built against the same manylinux 2.28
ABI baseline as the native wheel and statically carries the OpenSSL used by its
native-TLS dependency; it therefore requires glibc 2.28 or newer but not a
distribution OpenSSL package. A pre-package check rejects newer GLIBC, GLIBCXX,
or CXXABI references and non-baseline shared libraries. Both macOS artifacts
declare macOS 11.0 as their minimum deployment target and may link only Apple
system libraries. Windows x86-64 is built and verified on the pinned Windows
Server 2022 runner with the static MSVC runtime. These are artifact boundaries,
not a promise that every optional external browser, microphone, GPU, or provider
integration is available on every host.

Cargo remains a workspace development tool and the crates stay non-publishable.
Every pushed `v*` product tag runs the one canonical release workflow. It
publishes only after the four archive/wheel pairs have identical executable
hashes, the complete checksum manifest passes, and all four final installer
candidates succeed. PyPI receives only the four wheels; GitHub Releases receives
the wheels, archives, checksums, and versioned installers. Prerelease tags are
marked as such in both channels. The documentation site pins its hosted
installers to the newest stable published release that has the complete asset
set, so a prerelease or prepared but unpublished version cannot replace the
default installer. Operators can rerun
the workflow deliberately with
`gh workflow run release.yml --ref v<version>`.

## Data compatibility

Rust uses `CCCC_HOME` and defaults to `~/.cccc`. It reads the supported durable
state written by the final dual-engine release, 0.4.35.

The registry, group documents, ledgers, and actor contracts are shared. On first
Rust startup, CCCC validates the existing layout and adds a `.cccc-rust-v1`
compatibility marker without moving or deleting existing files. A non-empty
directory that is not already a CCCC home is still rejected.
The active Working Group is stored once in `CCCC_HOME/active.json` using
`{"v":1,"active_group_id":"...","updated_at":"..."}`. Rust also accepts the
former preview's `group_id` key and atomically normalizes it without losing the
selected group; new writes never recreate the legacy shape.
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
Unrecognized or malformed historical ledger lines are reported with their
source location and skipped, so one invalid record cannot make an entire group
unavailable. Rust preserves the 0.4.35 `message_mode`, `mail.read`,
reply/cancellation, and `runtime.delivery` contracts and does not recreate the
retired generic acknowledgement events.

ChatGPT Web Model connector bindings use
`CCCC_HOME/web_model_connectors.yaml`. Rust accepts the historical Python map
and preview array shapes. If the former Rust
`settings.yaml:web_model_connectors` section exists, it is merged into the
canonical file under the shared locks and removed only after the canonical
write succeeds.

A 0.4.35 daemon must be stopped before starting 0.4.36; two daemons must never
write the same home concurrently. Legacy selector metadata may remain as inert
migration input. The public `ccccd` executable is retired, while state filenames
such as `ccccd.addr.json` remain unchanged for compatibility.

## Dependency boundaries

```text
cccc-contracts <- cccc-core <- cccc-daemon
cccc-contracts <- cccc-client <- cccc-cli
```

Ports communicate with the daemon through the versioned IPC contract. Ledger
writes remain daemon-owned. Group documents and global settings use shared
cross-process transaction locks so daemon operations and Web-owned integration
lifecycle updates cannot overwrite each other.

The Rust daemon implements the optional daemon IPC `events_stream` upgrade and
reports `events_stream=true` in `ping.capabilities`. The stream follows the
bounded, best-effort resume and heartbeat contract; clients must still tolerate
disconnects or gaps and reconcile through Inbox/ledger reads. The native daemon
owns the NDJSON stream directly.

Projected-browser process ownership is internal to native Web. The retired
Python daemon attach streams are not advertised: all six `*_browser_attach` and
`*_browser_vnc_attach` daemon capabilities are `false`. External daemon clients
must consult the exact operation-named capability instead of opening a duplex
stream as a feature probe. Browser processes and frame streams are ephemeral.
Durable Presentation content, NotebookLM credentials, Web Model targets and
delivery state, and browser profiles remain under `CCCC_HOME` so each surface
can be reopened after a component restart.

The native MCP server retains the progressive tool surface from 0.4.35.
`tools/list` is derived from caller role and `capability_state`, includes
enabled built-in packs and 0.4.35-compatible external MCP runtime artifacts,
and forwards dynamic tool calls through `capability_tool_call`. A frozen
contract test guards the static 0.4.35-to-native tool catalog. Enabling an external
capability now performs the 0.4.35-compatible package preflight and installation
for npm, PyPI, OCI, command, and remote HTTP MCP records before persisting the
runtime artifact. Static tools and their complete input schemas come from one
packaged JSON contract. `cccc_code_exec` and
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
Native CCCC uses the stable top-level `im` configuration and the existing
pending, authorized-chat, and subscriber JSON files as the durable IM state.
Web, daemon, and bridge processes serialize those classes with the same
per-group IM lock and refresh durable authorization before delivery. Rust
normalizes literal credentials and `*_env` references to the same stored
fields, while `im_bridge` is limited to runtime diagnostics and bounded legacy
import. Unset consumes the former durable shadow so migration cannot copy, fork,
or resurrect IM configuration and authorization state.
`cccc doctor` reports daemon identity/version, the invoked executable, PATH
resolution and duplicate `cccc` commands, PTY support, browser discovery, and
Linux display helpers so installation failures are visible from the CLI.
Linux Web Model projection requires Xvfb and fails with an actionable error when
it is absent; it no longer silently changes behavior by falling back to a
headless browser.

The native CLI accepts the 0.4.35 public spellings for `prompt --actor-id`,
`tail --lines`, `doctor --all`, `runtime list --all`, `update --channel`, and the
`space jobs` / `space auth` subcommand trees. Standalone `cccc status` succeeds
while the daemon is stopped and identifies the Rust-only installation instead of
turning an expected offline state into a command failure.

Group Bridge compatibility includes daemon-level `remote_send`,
`remote_delivery_status`, and `group_bridge_receive_remote_send` operations in
addition to the Web and MCP routes. Remote delivery requires an explicit
recipient, validates the active registration or trust route, records idempotent
receipts, and falls back to the remote Group Bridge MCP endpoint when needed.
The daemon also owns 0.4.35-compatible signed outbound WebSocket sessions:
it scans active trusts, maintains heartbeats, reconnects with bounded
exponential backoff, projects connection health onto each trust, and prefers
the live route for message delivery before HTTP/MCP fallback.

Group Bridge uses the 0.4.35-compatible identity, pairing, registration,
credential, and receipt files as its one persistence authority. Native CCCC
imports the former preview `settings.yaml:group_bridge`
section once, commits the canonical files first, then clears the legacy
section. Canonical terminal trust decisions win by registration and route, so a
Python revocation cannot be resurrected by stale Rust state. Raw bearer and
remote-send tokens remain in the secret credential file behind opaque
references, and delivery receipts share the same
`registration_id::idempotency_key` namespace.

The Rust NotebookLM adapter owns notebook sources and Studio artifact
create/list/download operations. Its wire baseline now follows
`notebooklm-py` v0.8.1: the current `notebook.google.com` personal-app host,
correct artifact lifecycle values, `.m4a` audio output, and current source-add
payloads. Source refresh invokes NotebookLM's refresh RPC, and synchronous
artifact generation can wait for completion and save into the attached scope's
`space/artifacts/`.

Native Rust `resource_ingest` supports attached-scope local files,
`pasted_text`, Web URLs, YouTube, and Google Drive Docs/Slides/Sheets. It
validates file scope/format/size, URL kind, and Drive type before a provider
mutation; callers must still query `group_space_capabilities` instead of
assuming every source type exists.

Native Rust reads the canonical work-sync state and memory manifest written by
Python 0.4.35, but automatic remote work/memory mirroring is retired for
0.4.36. The old run action fails before provider-side mutation; CLI, Web, and
MCP no longer advertise it. Explicit ingestion replaces the mutation path
without retaining a hidden Python fallback or a second manifest implementation.
Artifact generation defaults to `wait=false` and `save_to_space=false`. CCCC
does not run the retired background auto-save worker; list or download the
completed remote artifact later, or explicitly request synchronous wait and
save. Native Rust downloads currently support audio, video, report/study guide,
infographic, and slide deck artifacts. Quiz, flashcard, mind-map, and data-table
generation remains available with `save_to_space=false`, but their native
download/formatting paths report `capability_unavailable` instead of writing an
incorrect file format.

## Runtime recovery and delivery

`group.running` stores the operator's desired runtime state. API group summaries
also expose `runtime_status`, which is derived from live actor sessions. On
daemon startup, enabled local actors in groups whose desired state is running
begin restoring in a detached worker after the daemon publishes its IPC
address. IPC health and diagnostics therefore remain available while an actor
runtime is slow to prepare or fails to resume; individual restore failures are
logged and do not prevent the daemon from becoming ready. Each Group is
reloaded and restored under the same mutation lock used by lifecycle requests,
so a concurrent stop or pause cannot be overwritten by a stale startup snapshot.

Actor-bound chat messages and system notifications use one bounded FIFO worker
per actor. A worker seeds the runtime with its CCCC system prompt once per
session, preserves message order, uses bracketed paste when the terminal enables
it, and applies the actor's configured submit mode. Successful delivery returns
to the daemon's serialized state path by appending `runtime.delivery`; it never
advances the separate Mail cursor.
The native preamble retains the 0.4.35 contract: cold-start and resumed sessions
are told to call `cccc_bootstrap`, which returns one bounded semantic packet:
session orientation, recovery state, an actionable inbox preview, context
hygiene, the memory-recall gate, and named routes for colder detail. It does not
dump the raw group or context trees, and ordinary chat deliveries do not
duplicate the full context JSON.
`CCCC_HOME/groups/<group_id>/prompts/CCCC_PREAMBLE.md` replaces the default
Startup body when present. Each delivered chat batch also ends with the MCP
reply reminder; batched
messages receive one reminder for the whole batch rather than one per message.

Before starting an automatically managed PTY actor, CCCC applies the retained
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
and actor health comes from the real provider process. Web Model and the
programmatically configured custom external-headless path retain the pull contract:
the executor obtains an ordered direct-delivery batch with
`cccc_runtime_wait_next_turn` and closes that exact active turn with
`cccc_runtime_complete_turn`. Runtime completion does not advance the Inbox read
cursor; only `cccc_inbox_read` consumes Inbox contents.

ChatGPT Web Model delivery uses one browser transaction boundary. It selects a
visible editable composer, confines Send discovery to that
composer, and treats Stop or a disabled Send control as a retryable pre-submit
deferral. Only a matching user-message echo is reported as `submitted`; weaker
post-click signals remain explicitly `ambiguous` and follow the shared
at-most-once policy, so CCCC does not click the same prompt again automatically.
The first delivery to a newly bound conversation also carries the daemon's
actor system prompt and Web transport contract; that bootstrap is recorded only
after the matching browser message appears, and is not repeated on later turns
unless the target or prompt revision changes.
The Web health projection exposes whether the cursor was committed and the
recommended recovery action. A newly created chat is bound to its final
conversation URL before a later batch can be delivered into it.

The daemon also owns a per-actor delivery preference. `standard` remains the
default text-only path.
`image_compat` is an explicitly experimental ChatGPT transport workaround: CCCC
materializes one deterministic 32x32 blank PNG in its runtime cache, attaches it
through the browser file input before Send, and treats attachment plus prompt
submission as one transaction. The setting persists across daemon restarts,
applies from the next accepted turn, and does not select or
change the model in ChatGPT.

Rust can conservatively recover pre-migration `wmd_*` new-chat deliveries whose
cursor was already committed before the conversation URL was bound. It rebuilds
the current prompt (including the actor bootstrap) and clicks Send only when the
same page has no user message or active response and its staged composer exactly
matches the recorded legacy prompt. User-edited or otherwise unverifiable drafts
remain paused, preserving the at-most-once boundary.

Cursor, Kilo, and Antigravity PTY sessions receive an idempotent MCP setup
contract before the normal preamble. It first checks for `cccc_bootstrap` and
only installs the `cccc mcp` stdio server when unavailable. Custom PTY runtimes
receive the same identity, group, bootstrap, and reply protocol preamble as
built-in runtimes. Voice Secretary system notifications include the complete
`input_envelope` or action-request envelope in the delivered payload instead of
only the generic notification title.

Voice Secretary operation discovery and its cross-client recording lease use
the canonical daemon boundary. Native CCCC stores the short-lived lease in the
0.4.35-compatible locked CCCC_HOME document, and Web
delegates acquire, heartbeat, and release to the daemon while validating the
token again for the audio stream. Lifecycle, durable health, sessions, pending
prompt drafts/requests, and ask requests now share the 0.4.35-compatible
`state/assistants.json` authority. Native CCCC imports its former group-embedded
workflow state once and leaves only `enabled`/`config` in `group.yaml`. Live
process health is deliberately recomputed rather than persisted. Native Rust
document/input projections and model installation remain implementation-owned
details; model installation is Web-owned and advertised as
unavailable through the Rust daemon.

Runtime delivery and Mail reading are separate facts. Send and Send + Reply use
`runtime.delivery` and never advance the Mail cursor. `cccc_inbox_read` consumes
only the next Mail batch in Mail append order; explicit message history remains
non-consuming.

Daemon connections are read concurrently with a size limit and timeout. State
operations remain serialized behind the dispatch lock, so a slow or malformed
client cannot block the listener or introduce concurrent group writers.
Daemon shutdown stops every local runtime session before releasing the shared
lock. The combined `cccc` process also closes Web after daemon loss. Daemon
reuse requires matching product version and compatibility ID;
legacy or stale daemons are replaced through graceful shutdown.

## Unified release gate

A release is publishable only when all of these remain true:

- Rust owns its CLI, daemon, kernel, MCP, Web API, runners, and integrations.
- The existing Web UI builds unchanged against the Rust HTTP/WebSocket surface.
- Four native wheels and four standalone archives wrap byte-identical native
  executables for their platform and pass metadata, size, payload, ABI, and
  checksum checks. There is no source distribution or portable wheel.
- CI installs the native wheels and runs their declared CLI, MCP, daemon, Web,
  update, and uninstall journeys. It also runs offline status, daemon lifecycle,
  MCP initialization, and a real `cccc_code_exec` cell against the built binary.
  Final-installer verification repeats the complete Unix
  flow on Linux and both macOS architectures; Windows verifies installed offline
  status, MCP startup, daemon lifecycle, and executable release after shutdown.
- Wheel metadata, Cargo, the lockfile, and the Git tag resolve to one release identity.
- The native binary runs without a Python backend dependency.
- Frozen 0.4.35 homes pass the native migration suite without Python on `PATH`.
- PyPI and GitHub publication happen only after the complete four-platform
  release set passes one canonical workflow.
- Supported 0.4.35 `~/.cccc` data remains available after upgrading.

Credentialed live-provider canaries (NotebookLM/Google browser auth and external
IM vendors) remain environment-owned release checks: source and installed-binary
gates cannot synthesize those third-party accounts. Their absence is reported as
a live-validation blocker, never treated as proof that the provider path passed.
