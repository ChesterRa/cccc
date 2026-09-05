# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/), and versions follow SemVer/PEP 440.

## [Unreleased]

### Added

- **Claude Code now provides the same visible, precisely observed session to Group Actors and Voice Analyst.** CCCC observes authoritative turns and tool results from Claude Agent View and attaches the native writable Claude TUI to that exact session; ordinary stop/start resumes the validated provider conversation.

### Changed
- **Claude Code uses one strict managed-session path instead of separate Hook and `claude -p` implementations.** Runtime Profiles retain supported model, effort, tools, plugins, settings, and private environment while CCCC owns session topology, MCP identity, YOLO policy, cancellation, and resume. Unsupported wrappers, prompt tails, transport flags, and legacy receipts fail explicitly instead of falling back to a divergent session.
- **Actors no longer expose PTY versus Headless as a configuration choice.** CCCC derives the Runtime surface automatically: CLI Actors retain their native writable terminal, while Codex, Claude Code, Grok Build, and OpenCode pair it with structured lifecycle observation on the same session. Incoming Actor messages are handed to that native terminal without CCCC waiting for provider idleness or choosing queue-versus-steer semantics.

### Fixed
- **Forced launcher exit terminates owned process trees independently of normal shutdown.** PTY, managed providers, DeepSeek and Web-owned daemon launches register OS resources at spawn; the second interrupt or normal-shutdown deadline terminates those resources without protocol/session locks or another cleanup timeout. Unix exit polling revokes ownership before reaping to avoid reused PID/PGID targets.
- **Group action menus no longer activate mouse sorting.** Menu mouse-down events stay inside the trigger while row dragging remains available.
- **Long terminal history no longer loses newer lines when older pages are loaded.** Scrolled-off lines have a separate bounded history buffer with explicit truncation reporting.
- **Windows managed processes join their Job before executing.** Standard launches remain suspended through Job assignment, preserving detached launch flags and cleaning up failed starts.
- **Failed managed-process stops retain retryable ownership.** Child handles remain owned until confirmed exit and reaping, including signal and wait failures.
- **Sidebar group reordering updates immediately.** The rendered list now subscribes to order changes, preventing drag snap-back and subsequent moves based on stale row positions.
- **Claude resume recovers stale transcript paths after worktree moves.** Missing published paths are resolved to a unique retained transcript for the same session only during initial recovery; history offsets and active-file identity checks remain intact, and missing-history errors identify the path and session.
- **Daemon takeover validates the actual CLI subcommand before targeting a PID.** Global options no longer cause one-shot commands to be mistaken for daemon hosts.
- **Terminal history preserves ANSI state across backward pages and remains keyboard accessible.** Rendering preserves inferred frames before repainting within a fixed cumulative range. Short pages require explicit loading, errors pause automatic loading, and the nested dialog traps focus and restores its opener.
- **Managed Claude sessions retain host proxy and CA settings.** HTTP/HTTPS/ALL/NO proxy aliases and custom CA paths survive the cleared launcher environment and are included in the private settings used for session respawn. Explicit network overrides, including empty proxy values, take precedence.
- **Managed Claude and Codex sessions recover before their first completed turn.** Claude can locate its exact validated transcript before Agent View publishes `linkScanPath`, preserving the resume read boundary. New Codex threads materialize metadata and pass a same-thread resume check before CCCC exposes the native terminal; startup does not inject a model prompt.
- **Managed terminal startup no longer accepts truncated first messages.** Actor and Voice Analyst native input waits for the TUI's input mode before writing, without injecting startup work or waiting for the current model turn to finish.
- **Claude sessions stop and resume reliably across empty starts and observer failures.** Verified empty sessions retain their conversation ID without requiring nonexistent history; observer failures use confirmed background-task shutdown, and normal closure releases event readers. Transcript replacement checks now use file handles supported by stable Rust on Windows.
- **Native Windows Actors now start and receive messages reliably.** Runtime launch resolves executables in the configured PATHEXT order without restoring excluded extensions or selecting extensionless npm shell shims, routes `.cmd`/`.bat` through the Windows command processor, preserves backslashes in `actor --command`, and uses the last stable `portable-pty` ConPTY behavior. A Windows-only regression test delivers UTF-8 text through an npm-style batch shim and verifies that the Actor stays alive.
- **Claude managed-session settings no longer cross the Windows command-line quoting boundary as inline JSON.** CCCC merges the effective settings and Runtime Profile environment into a permission-protected JSON file under `CCCC_HOME` and passes only its path to Agent View, preventing invalid-JSON launch loops without exposing private values in process arguments.
- **Windows Web startup and non-ASCII CLI output recover from platform defaults.** A reserved-port `WSAEACCES` (`10013`) retry uses an OS-assigned port and reports the effective address; later commands reuse that binding only while its recorded process and signed readiness identity remain live. The native CLI uses UTF-8 console code pages for its lifetime and restores the previous values on exit.

## [0.4.37] — 2026-09-01

### Added
- **Experimental Codex Voice brings one global spoken control surface to CCCC Web.** Realtime Voice handles low-latency conversation while one persistent, resumable Voice Analyst Codex thread can inspect repositories, query CCCC state, use tools, delegate work to Actors, and return results through the same conversation. The console includes audio controls, device and speaking-voice selection, and the genuine Analyst TUI without creating another Actor.

### Changed
- **Agent messages are concise without losing their reply target.** Runtime delivery keeps the full actionable event ID, removes repeated default mode and parent metadata, and gives one correct reply-tool reminder per batch. Successful MCP message operations again return the perspective-reset context, while policy failures retain structured recovery details.
- **Direct localhost Web use is passwordless without creating a hidden administrator token.** Exact loopback browser origins receive an in-memory local administrator principal; LAN, Reach, public URL, and reverse-proxy exposure cannot be enabled until an explicit Admin Access Token exists. Authenticated browser sessions use a rolling 30-day HttpOnly cookie and discard the temporary bearer after verification.

### Fixed
- **Group Bridge v2 now proves the complete live handshake and pins both peers.** A fresh client nonce and server-signed ready transcript prevent challenge/ready replay, while the native client persists `min_session_protocol=2` and refuses later v1 fallback. Each approved, unclaimed record from before the claim-window upgrade receives one persisted ten-minute compatibility window when that record is first accessed instead of becoming permanently unclaimable.
- **Large Web messages no longer fail with Rust HTTP 413 responses.** Same-group and remote Group Bridge bodies above 64 KiB become UTF-8 text attachments; local cross-group text stays inline under the bounded daemon IPC contract.
- **Codex Voice turn ownership and recovery fail closed instead of misrouting work.** Voice delegations are matched to their exact Codex turn, Actor results are accepted only from the assigned recipient, stale Analyst repository bindings are replaced before reuse, and unreplayable lifecycle gaps invalidate the session rather than leaving it busy or speaking the wrong result.

## [0.4.36] — 2026-08-30

### Added
- **The native daemon now owns the public `events_stream` control path.** SDK clients retain capability discovery, bounded resume, routing, heartbeat, and slow-reader behavior without a Python daemon owner.
- **The native NotebookLM adapter supports explicit URL, YouTube, Drive, text, and project-scoped file ingestion against the upstream v0.8.1 protocol baseline.**

### Changed
- **CCCC now ships one native Rust product implementation.** The public `cccc` command owns the CLI, daemon, MCP server, and Web UI; 0.4.35 homes remain readable through explicit migration compatibility.
- **The website installer and pip platform wheels distribute the same native executable.** Generic source builds are rejected, and unsupported pip platforms fail resolution instead of falling back to an older Python-only release.
- **The default Docker image now builds and runs the same native product.** The separate Rust Dockerfile/Compose variant is retired while the existing data-volume contract remains intact.
- **Browser projection, runtime recovery, background Group Bridge retry, shared agent resources, and Voice Secretary ASR readiness now have one native owner.** Retired Python attach, sidecar, and lifecycle paths no longer compete with the shipped product.
- **NotebookLM source changes are explicit.** Automatic work/memory mirroring is retired, while legacy 0.4.35 sync metadata remains readable for upgrade status.

### Fixed
- **Pip and website installations cannot silently inherit or overwrite each other's update authority.** Their ownership markers are mutually exclusive, and switching channels requires uninstalling the current owner first.
- **NotebookLM uncertain creates and stale retries no longer become duplicate or misdirected writes.** Unconfirmed provider outcomes remain unresolved, and retry verifies the Group's current notebook binding before mutating remote state.
- **Supported 0.4.35 durable state remains available without a Python runtime.** Frozen upgrade coverage includes core state, identities and integrations, product state, and retired shadows that must not be resurrected.

### Removed
- **The Python product implementation and engine-selection layer are retired.** The Python daemon, Web server, launcher, importable `cccc` package, `cccc python` / `cccc rust` selectors, public `ccccd` alias, source distribution, and universal wheel are no longer shipped.
- **Python-only NotebookLM mirroring and legacy IM lifecycle controls are retired.** Explicit NotebookLM ingestion replaces hidden sync writes, while Web and the native CLI replace the old IM `/context`, `/launch`, and `/quit` commands.

## [0.4.35] — 2026-08-28

### Added
- **Messaging now has three explicit delivery modes: Send, Send + Reply, and Mail.** Immediate runtime handoff, reply obligations, and non-interrupting Inbox delivery are separate durable facts; Mail has its own consuming cursor and bounded, content-free reminder.
- **DeepSeek Harness is a first-class managed headless runtime.** CCCC owns the tested ACP package composition under `CCCC_HOME`, isolates sessions by actor, and projects structured turns through the same durable runtime contract as other headless providers.
- **CCCC Account and Reach add optional managed remote access.** The Web UI can link an installation through device authorization, while Linux and macOS users can explicitly publish Web through a supervised, checksum-pinned Cloudflare tunnel without uploading local ledgers or repositories.
- **CCCC Self-Evolution is now a built-in, default-enabled Skill.** New and existing Groups receive the packaged capability once, while explicit disablement remains durable across restarts and upgrades.
- **The Web workspace adds paged task/context views, directory creation in the project picker, and a full-screen mobile Presentation surface.**

### Changed
- **Python remains the stable default and Rust remains an experimental implementation of the same product.** Both engines use the same version, Web UI, daemon contract, and `CCCC_HOME`, with stronger parity around delivery, Reach, capabilities, runtime restoration, and shared state.
- **Context and Task Board reads are paged and version-consistent.** Large Groups no longer require one oversized snapshot, and stale responses cannot overwrite the active Group view.
- **Account and Reach setup now use a first-class Account surface and a shorter guided Web flow.** Local CCCC remains fully usable without an account.
- **Per-push CI now focuses on source correctness instead of repeating release verification.** Python tests use two balanced shards, Linux Rust checks share one workspace, and slow native installers plus intermediate Python compatibility run nightly or on demand while release workflows retain exact artifact gates.
- **Web Model automated tests and their visible browser prompt fixtures were removed.** Product implementation remains available, while the default Rust/frontend test run no longer opens local `Send` fixture pages in Chrome.

### Fixed
- **Delivery and recovery no longer conflate runtime handoff, Inbox read state, or replies.** Claims are settled durably, Mail never wakes an actor, reply requests can be cancelled, remote Group Bridge sends preserve their source identity, and interrupted work remains recoverable without silently duplicating committed turns.
- **DeepSeek failures now have bounded, durable recovery behavior.** Missing credentials and context overflow require an explicit restart, large histories use indexed recovery, and daemon restarts cannot turn permanent provider failures into retry loops.
- **Standalone Rust self-updates now adopt their exact markerless executable safely.** The CLI passes its canonical current path into the transactional installer, while every other markerless command—including legacy launchers and version-shaped foreign programs—requires explicit replacement and remains protected by default.
- **Remote-control boundaries now fail closed across Python and Rust.** Unauthenticated daemon IPC rejects every non-loopback TCP bind; Reach admin links use short-lived, one-time, origin-bound exchanges instead of long-lived tokens; Reach verifies the exact local CCCC Web instance before opening a tunnel; and cookie-authenticated writes require an exact allowed Origin or same-origin Referer.
- **The Vite development proxy now preserves the browser-facing Host for terminal WebSockets.** Rust Web Origin validation no longer rejects legitimate `127.0.0.1:5555` runtime-inspector connections and leaves the xterm surface blank.
- **Stale Vite dependency chunks no longer collapse the Web composer during development.** Dynamic-import failures trigger one bounded page reload and then degrade only the Voice Secretary launcher, while base-relative manifest and icon paths avoid duplicate `/ui/ui/` requests.
- **Provider exits and user-directed delivery now share one lifecycle contract across Python and Rust.** A natural provider exit records durable stop evidence without disabling the actor or Group; Send and Request Reply reactivate targeted recipients, manual delivery resumes a paused or stopped Group only after reserving new work, and Mail remains non-waking.
- **Cross-Group Voice Secretary work and mobile Presentation controls stay in their visible scope.** A pending prompt refinement remains bound to its originating Group while navigation continues, and the full-screen mobile Presentation dialog keeps keyboard focus inside the active surface.
- **Chat history, runtime activity, branding, and voice capture survive more browser edge cases.** Filtered views retain their scroll anchors, empty histories can page older events, installed PWA icons follow custom branding, and microphone resampling preserves buffered audio across input-rate changes.
- **Windows combined-launcher cleanup now owns the exact process object instead of trusting a reusable PID.** Graceful shutdown is fenced to that daemon identity and receives the full lifecycle deadline before bounded fallback cleanup, so normal dispatch contention cannot trigger an early kill and descriptor handoff cannot stop a replacement daemon.
- **Python release retries now verify immutable artifact hashes from one package-index snapshot.** Matching files remain idempotent, while a same-name rebuild fails instead of mixing distributions from different builds under one version.

## [0.4.35-rc1] — 2026-08-20

### Added
- **DeepSeek Harness is now a first-class managed headless runtime.** CCCC installs the pinned ACP composition under `CCCC_HOME`, isolates provider sessions per actor, projects structured turn events, and keeps failed or interrupted delivery retryable without duplicating committed work.
- **Optional membership and Reach are available as an explicit preview on Linux and macOS.** `cccc login`, `cccc logout`, and `cccc reach` bind a machine account, supervise a checksum-pinned `cloudflared`, and expose separately labeled Web and ChatGPT connector URLs without uploading the local ledger or repository.
- **Windows project browsing now includes available drive roots**, allowing attached scopes to be selected outside the current drive from the Web UI.

### Changed
- **Runtime restoration and message delivery use stronger lifecycle boundaries.** Group restore is serialized with lifecycle mutations, large legacy headless histories migrate through bounded indexes, and delivery completion advances only across a contiguous committed prefix.
- **Web task coordination and chat following were split into focused components.** Task Board controls, cards, columns, virtual-message anchoring, and send-follow behavior now avoid stale scroll requests and oversized container components.
- **The combined Rust Web/daemon launcher owns only the Windows daemon process it created.** Startup waits through legacy-daemon handoff, shutdown leaves a replacement owner untouched, and failed Web startup cleans up the detached daemon before returning.

### Fixed
- **Windows standalone updates and daemon recovery no longer fail on expected stopped-state diagnostics.** PowerShell 5.1 now checks daemon commands by process exit code without promoting native stderr into a terminating installer error, empty restart diagnostics are null-safe, and a verified replacement is retained when only runtime restart fails. The Rust daemon publishes IPC before restoring actors in a detached worker, serializes each Group restore with lifecycle mutations, and keeps slow or failed recovery diagnosable without blocking daemon readiness; stale unlocked daemon files continue to be reclaimed through the operating-system lock.
- **DeepSeek delivery, recovery, timeout, and write-failure paths now preserve one durable outcome.** Turn and operation identities are bounded, pending work survives restarts, and a provider failure cannot silently advance the inbox cursor.
- **Daemon process and ledger locks now fail safely across restart races.** Stale unlocked files are reclaimed, owned process trees are bounded, and competing daemon owners are not terminated during cleanup.

### Tests
- Added deterministic DeepSeek ACP, durability, recovery, timeout, projection, and setup coverage across Python and Rust.
- Added membership, Reach, cloudflared supervision, cross-engine state, Windows updater, daemon handoff, process-tree, and combined Web startup-failure coverage.

## [0.4.34] — 2026-08-16

### Added
- **Supported native wheels now bundle an experimental Rust implementation behind the normal `cccc` launcher.** Python remains the stable default, `cccc rust` and `cccc python` switch the persisted implementation explicitly, and an optional Rust-only standalone preview is available for native deployment evaluation.
- **ChatGPT Web Model actors now offer an experimental GPT Pro delivery mode.** The per-actor setting attaches one deterministic blank PNG to each batch for accounts where that ChatGPT behavior exposes the CCCC connector; Standard text-only delivery remains the default, and CCCC never selects the model.
- **Cline is now a first-class PTY runtime** with discovery, actor defaults, MCP setup and repair, diagnostics, Web metadata, documentation, and Python/Rust coverage.

### Changed
- **Python and Rust now share one product version, public command, Web frontend, daemon contract, and durable `CCCC_HOME` authority.** Engine switching validates the bundled payload, replaces the active process pair, and never silently falls back.
- **ChatGPT and NotebookLM use a unified projected-browser foundation.** Saved ChatGPT targets, new-chat binding, Page/Browser viewing, persistent profiles, pointer scrolling, Xvfb isolation, and the vendored NotebookLM 0.8.0 auth/provider boundary were aligned across implementations.
- **The Web composer defaults a new preference to Need Reply and remembers later user choices.** Delivery receipts, capability availability, implementation banners, mobile controls, chat anchors, and terminal readiness are projected more consistently.

### Fixed
- **Cross-engine state no longer splits across browser connectors, Group Bridge trust, IM identifiers, Voice Secretary sessions, active Group selection, automation, capabilities, and runtime recovery.** Canonical shared stores and generation boundaries prevent stale state from being restored after an engine switch or actor recreation.
- **Messaging and unread state follow ledger order and durable source identity.** Sends, replies, files, tracked work, cross-Group receipts, acknowledgements, cursor advances, and deferred retries avoid duplicate, skipped, or permanently stranded work.
- **ChatGPT browser delivery is now single-flight, target-aware, and at-most-once after an ambiguous submit.** A failed batch cannot poison later delivery, bootstrap state survives reconciliation, and Python/Rust expose the same visible delivery evidence.
- **Voice Secretary recording scope, document mutation, partial ASR failure, transcript cleanup, lease renewal, and speaker-analysis admission are bounded and consistent.** Switching Groups during recording no longer redirects background output, while manual controls continue to use the current Group.
- **Runtime startup, hooks, resume, terminal replay, daemon handoff, shutdown, installers, and remote Web access received a broad reliability and security pass.** Hook-unavailable Codex launches retain CCCC MCP, native terminal reconnects preserve output order, scoped tokens cannot observe other Groups, and installers refuse unowned command replacement.

## [0.4.34-rc4] — 2026-08-15

### Changed
- **CI now isolates process-lifecycle coverage and post-merge native verification.** The load-sensitive daemon/Web shutdown suite runs serially outside the parallel Rust workspace tests, while native distribution and Windows installer checks run after merge behind stable aggregate gates.
- **GitHub Actions updates no longer rewrite the pinned Rust compiler version.** Dependabot continues maintaining normal Actions dependencies but ignores `dtolnay/rust-toolchain`, whose action ref intentionally encodes the workspace's Rust 1.88 toolchain.

### Fixed
- **Web startup banners now reflect the actual listener scope.** Loopback-only Python and Rust launches no longer advertise an unreachable LAN address; a detected `Network` URL is shown only for wildcard listeners that accept remote connections.
- **Native capability installs now refresh slash-command catalogs reliably across clients.** Rust commits enablement and visibility together, emits one durable change event per semantic install, preserves the last known-good Web catalog across request races, and catches up during SSE reconnects and polling fallback.
- **Claude headless resume failures can no longer restore stale session metadata.** Provider exits after the startup grace period are serialized with session recording, invalidated before retry, and reported through the existing resume-failure event path.
- **Voice Secretary recordings keep their original Group scope while navigating.** Switching Groups during an active recording no longer redirects document checkpoints, final transcripts, Ask/Prompt work, or direct-composer text into the newly visible Group; the immutable recording scope continues targeting the original Group and document, with composer text routed to that Group's preserved draft.
- **Silent Voice Secretary streams no longer grow the native ASR heap without bound.** Rust now resets sherpa-onnx at every detected endpoint even when the hypothesis is empty or unchanged, releasing accumulated streaming features while preserving final transcript events.
- **Long Voice Secretary recordings no longer block later stops or permanently lose speaker labels.** Short WebSocket recordings still receive immediate final ASR; when speaker analysis is available, persistent recordings over 30 seconds, or recordings stopped while native inference is occupied, complete stop promptly and retain their durable live transcript while speaker analysis waits behind the active job instead of returning `worker_busy`. Final ASR paths that cannot defer reuse one SenseVoice recognizer across bounded 30-second ranges.
- **Message delivery preferences and lifecycle state stay aligned across runtime paths.** Delivery recovery no longer drifts from the actor's persisted mode or leaves stale lifecycle projections behind.
- **The embedded CLI reacts to daemon loss without polling.** Combined process shutdown follows the daemon exit signal directly, reducing delayed or inconsistent teardown behavior.
- **Capability discovery exposes autoload candidates consistently.** The daemon includes the candidate state expected by clients, and the Web capability picker now has complete English, Chinese, and Japanese labels.

## [0.4.34-rc3] — 2026-08-14

### Changed
- **Distribution guidance now keeps PyPI as the stable, recommended product path.** The standalone Rust binary is presented consistently as an experimental Rust-only preview without Python fallback or implementation switching, including its GitHub Release metadata.
- **Release publication now gates complete Python and standalone artifact sets without duplicating normal CI.** PyPI receives one source distribution, one portable wheel, and four version-matched native Rust wheels after parallel payload checks; standalone binaries execute on their build hosts and final Linux and Windows installer candidates are verified in parallel. Full source and cross-language interoperability suites remain in normal CI only.
- **Group Bridge pairing invitations are copied as soon as they are generated.** The complete JSON remains visible for manual copying when clipboard access is unavailable.
- **Web startup banners now name the active implementation.** A bare `cccc` follows the persisted Python/Rust selection, and the terminal makes that choice visible instead of looking like it silently ignored the initial Python default.
- **ChatGPT Web Model actors now have one shared per-actor delivery preference across Python and Rust.** Stable `Standard` delivery remains text-only and default; the explicitly experimental `GPT Pro` mode attaches one deterministic blank PNG before submission for accounts where that ChatGPT-side behavior exposes third-party MCP. The preference applies from the next accepted turn, survives daemon restarts and engine switches, and never selects a ChatGPT model.

### Fixed
- **Actor restart recovery no longer replays every unread message body into a fresh PTY.** Python and Rust inject one transient bounded unread summary, preserve the canonical cursor, and direct the actor to the inbox tools while normal live backlog delivery remains ordered.
- **Windows standalone updates no longer hang while restarting a running daemon.** The installer waits on the short-lived `daemon start` launcher itself instead of PowerShell's native process pipeline, while retaining a bounded failure timeout and rollback restart.
- **SPA fallback routes now retain the HTML response type.** Browser deep links served through `index.html` no longer inherit an `application/octet-stream` MIME type from the extensionless request path.
- **Windows Rust Web builds no longer compile Unix-only browser helpers.** Platform-specific imports and CDP port reservation are gated to the targets that use them.
- **Codex hook injection now follows the documented provider contract in both backends.** Unsupported `PostToolUseFailure` and `StopFailure` registrations were removed; non-zero tool commands continue to complete through Codex's supported `PostToolUse` event instead of relying on hooks that never fire.
- **Remote Web exposure now has one administrator-token boundary across the product.** The Web UI blocks LAN/public Save, Apply, and endpoint copying without an Admin Access Token; Python and Rust also reject remote start, apply, and listener startup, so scoped tokens or direct API calls cannot bypass the rule. The obsolete UI choice to disable remote token protection was removed, while localhost-only recovery and the explicit host-level unsafe override remain available.
- **Settings dialog footers retain their normal mobile spacing when adding the device safe area.** Bottom actions no longer lose their base padding on notched devices.
- **Standalone Windows replacement tolerates short-lived executable locks.** Installer transactions retry bounded file moves after probing or stopping the old CCCC process, avoiding transient sharing violations while preserving rollback and active-lock failures.
- **Rust ReMe writes no longer overwrite an existing memory file when it cannot be read as UTF-8.** Only a genuinely missing file is treated as empty; permission, I/O, and decoding failures now leave the original bytes untouched and return an error.
- **Standalone installer ownership markers now participate in the binary transaction.** Unix and Windows stage the exact marker before restarting a previously running daemon, restore a foreign marker when activation fails, and can safely replace a read-only marker during an explicitly authorized takeover.
- **Rust diagnostics now use the same system-browser discovery as the Web runtime.** On Windows, `cccc doctor` recognizes Chrome, Edge, and Chromium in both `Program Files` roots even when the browser is not on `PATH`; an unlocked `cccc-web.lock` containing a crashed process PID remains safely reclaimable on the next launch.
- **Standalone Windows self-updates now enable TLS 1.2 before downloading the installer.** Windows PowerShell 5.1 can update through the same hardened bootstrap path used by the documented fresh-install command.
- **Rust resume recovery now respects explicit actor and Group stops.** Resume verification is generation-guarded and serialized with lifecycle changes, while failure detection reads only the current PTY session so retained errors from an earlier session cannot reject a later valid resume.
- **Duplicate CCCC installations no longer fail silently through PATH shadowing.** Standalone installers preserve other installations but put the managed command first where they own PATH setup, report every remaining duplicate, and both Python and Rust `doctor` identify the invoked executable, active PATH command, and conflicts.
- **Rust ChatGPT Web Model delivery now matches the Python browser contract.** It selects the visible editable composer instead of ChatGPT's hidden compatibility textarea, uses only a stable composer-local Send control, never mistakes Stop for Send, and requires a user-message echo before reporting `submitted`. The actor bootstrap is injected once per bound conversation, post-click uncertainty is surfaced as an at-most-once ambiguous delivery, pre-click deferrals remain retryable, new chats bind their final conversation URL before later delivery, and Web health exposes the same readiness, evidence, cursor, and recovery states across implementations. Legacy pending new-chat deliveries are replayed only when the current page, empty transcript, and staged composer text prove an exact match; edited or otherwise uncertain drafts pause for operator review instead of risking a duplicate send.
- **Linux projected-browser surfaces now avoid WSLg display collisions.** Python and Rust allocate explicit isolated Xvfb displays from a high range, keep the local Unix transport available, skip occupied sockets even when their legacy lock file is absent, and preserve bounded Xvfb diagnostics instead of reducing every failure to a generic timeout.
- **Experimental standalone installers no longer overwrite a public command owned by another installation.** Unix and Windows require the exact standalone ownership marker unless replacement is explicitly requested, and the Rust self-updater validates the same marker contents.
- **Rust terminal history now keeps privacy and lifecycle boundaries intact.** Cross-actor reads obey each Group's transcript visibility, durable capture remains opt-in like Python, archive failures fall back to bounded memory without blocking actor startup, and a late reader from an old PTY session cannot overlap or hide the replacement session.
- **Cross-Group source messages again use the canonical origin-side audience and destination metadata.** Source ledger events target the local user and carry resolved remote recipients in `dst_to`, while destination events retain their normal `to` audience and legacy Web projections remain readable.
- **Installer and runtime setup path discovery is safer across shells and platforms.** Bash installation reuses the login profile Bash would already read instead of creating a shadowing `.bash_profile`; Rust MCP preflight honors inherited runtime config directories, resolves relative `PATH` entries from the actor working directory, supports Windows command shims, and terminates timed-out Windows command trees.

## [0.4.34-rc2] — 2026-08-06

### Added
- **Native installers are available from the documentation site.** macOS/Linux users can pipe `install.sh` to `sh`, Windows users can pipe `install.ps1` to PowerShell, and neither path requires a Rust or Python toolchain. Matching tags publish checksum-verified standalone archives to GitHub Releases; the hosted installer currently pins `v0.4.34-rc2` for release-candidate validation.
- **One `cccc-pair` installation now contains both CCCC implementations on supported platforms.** Python remains the default, while platform wheels carry a private, version-matched Rust payload behind the stable public `cccc` launcher.
- **Cline is now a first-class PTY runtime.** Runtime discovery, actor configuration, MCP installation and repair, Web metadata, defaults, diagnostics, documentation, and both Python and Rust tests cover the Cline CLI.

### Changed
- **Release publication keeps targeted final-artifact smoke coverage without a duplicate floor job.** Native wheels are installed and switched on their build platforms, standalone binaries execute on each native host, and final installer candidates run on Linux and Windows alongside the existing structure, checksum, test-suite, interoperability, and complete-release-set gates.
- **Standalone Rust installations now own their update path.** `cccc update` reuses the stable website installer, including daemon shutdown, checksum validation, rollback, and PATH-safe replacement; wheel-private Rust payloads remain owned by the Python product installer.
- **Implementation selection and updates now have one owner.** `cccc rust` and `cccc python` switch persistently without creating competing commands on `PATH`; `cccc update` replaces the complete pip product, and Rust crates are no longer independently publishable.
- **The global Web event stream is now a routing signal rather than a content channel.** It carries only event identity, type, time, and Group identity; clients continue to read complete events from the authorized per-Group ledger stream.

### Fixed
- **Browser navigation ignores stale DOMContentLoaded events from the reusable blank page.** Seeded profile startup now waits until the destination document is actually interactive while still returning before stalled subresources finish.
- **Windows Rust binaries no longer require a separately installed Visual C++ runtime.** The MSVC CRT is linked statically so both the standalone archive and the Rust payload inside the Python wheel start on a clean Windows system; release interop also invokes the Python interpreter provisioned by Actions explicitly.
- **Rust-delivered reply instructions now consistently use `cccc_message_reply`.** Required and cross-Group reply envelopes no longer contradict the generic MCP reminder by directing actors through `cccc_message_send`.
- **CI now uses the interpreter provisioned for Python/Rust interop and can prepare Rust Web assets on Windows.** The interop job no longer assumes a repository-local virtual environment, and the release helper launches `npm.cmd` through the Windows shell instead of failing with `spawnSync EINVAL`.
- **Rust actor startup now repairs stale CCCC MCP wiring before the provider session is created.** The Rust daemon matches Python's automatic-runtime catalog and `ready`/`missing`/`stale` verification flow across Claude, Cline, Copilot, Devin, Kiro, Droid, Amp, Auggie, Grok, Hermes, Kimi, and OpenCode; Codex keeps its stronger actor-scoped launch override. A removed Python launcher or dangling legacy symlink can no longer leave a newly started session without CCCC tools.
- **Scoped Web Access Tokens can no longer observe events from Groups outside their scope.** This applies equally when the supported `?token=` URL is used to bootstrap the browser session, and administrative capability blocking now requires an Admin token.
- **Python tests no longer inherit the operator's real `CCCC_HOME`.** Each test receives an isolated runtime home, preventing test daemons and ledger events from leaking into live local state.
- **Python inbox cursors and status projections now follow append-only ledger order instead of wall-clock ordering.** Events remain unread, read receipts and attention obligations remain accurate, Web Model turns commit through their delivered ledger boundary, and schema-1 timestamp-derived unread indexes rebuild safely when timestamps collide or the system clock moves backwards. Cursor advances use the existing ledger index instead of repeatedly rescanning full histories.
- **Group Bridge active state now reflects a route that can actually deliver.** Rust WebSocket sessions register generation-guarded runtime leases in the daemon, status resolution and MCP delivery share that readiness source, disconnected routes downgrade immediately, and endpoint-free routes can deliver through a connected reverse session with distinct unavailable, timeout, and failed errors.
- **Recovered actors no longer remain falsely marked as stopped in an open Web inspector.** An enabled visible actor with a stale stopped projection now reconciles against the daemon until its authoritative running state arrives, closing the refresh deadlock that previously prevented the terminal from reconnecting after daemon recovery.
- **Web PTY tabs now replay retained ANSI output in bounded chunks and resume from an exact raw byte cursor.** The first attach preserves full terminal semantics while 64 KB pages avoid a single multi-megabyte frame; reconnects deliver only the missing suffix without gaps or split UTF-8 characters. The UI also waits for the attach acknowledgement before reporting readiness, eliminating the misleading read-only state while Rust history is loading.
- **Chat tabs now preserve the exact visible message anchor across view switches.** Scroll snapshots use a versioned signed offset so the first row's virtualized top inset is restored without jumping, and bounded anchor correction absorbs delayed row measurements without fighting later user input.
- **Rust distributions now match the Python 0.4.33 group-preamble IPC contract.** `group_preamble_get`, `group_preamble_set`, and `group_preamble_reset` preserve permissions, idempotency, confirmation, and the 512 KiB UTF-8 limit when SDK clients migrate between daemon implementations.
- **Rust-native messaging now preserves the full domain contract across every high-level operation.** Tracked sends create durable linked tasks, `send_files` validates and stores active-scope files through the daemon-owned blob boundary, headless actors receive complete message envelopes, `/install` routes through the CCCC capability lifecycle, replies acknowledge handled obligations, human messages wake idle groups, and stream/delegation/attachment/scope/cross-group compatibility behavior matches the legacy daemon.
- **Cross-group and runtime recovery metadata now survives compatibility edge cases.** The Web UI resolves canonical cross-group sender ids to group labels, revoked bridge trust immediately disables credentials and live sessions, Python group saves round-trip YAML datetime values, voice delivery ignores legacy events without segment identity, and verified Chromium profiles recover safely after a local hostname change.
- **Stopping the combined Web and daemon process is now bounded and responsive with active actors**. Runtime starts close behind a shutdown gate, automation observes cooperative cancellation, actor delivery delays are interruptible, and IM/connection cleanup uses short shared deadlines.
- **Actors no longer replay already-read Python-era inbox history after upgrading to the Rust runtime**. Legacy read cursors are merged monotonically, actor-targeted notifications stay scoped to their intended actor, and obsolete `New message` control notices are folded into their source chat message.
- **Revoking the final migrated IM chat authorization now persists across refreshes** instead of immediately restoring the chat from legacy authorization files; unmatched revoke requests also surface as failures in Web settings.

## [0.4.33] — 2026-07-28

### Added
- **Mermaid diagrams render directly in completed Chat and Inbox messages.** Users can switch back to the original source, copy it, or open the rendered SVG in a large preview without leaving the conversation.
- **Group startup preambles now have a first-class daemon IPC contract.** Integrators can read, set, and reset a per-group startup preamble without writing directly into `CCCC_HOME`; the fixed CCCC identity and protocol frame remains intact.

### Changed
- **CCCC now supports Python 3.11 through 3.14.** Python 3.11 is the new minimum, Python 3.14 is the primary CI and Docker runtime, and compatibility smoke coverage protects Python 3.11, 3.12, and 3.13.
- **Mermaid rendering is scoped to message surfaces and loaded on demand.** Other Markdown views continue to show Mermaid fences as source, while theme changes refresh active diagrams without adding Mermaid to the initial Web bundle.

### Fixed
- **Invalid, oversized, and unsupported Mermaid diagrams fall back to their source cleanly**, while canceled queued jobs are skipped before expensive rendering begins so obsolete views do not delay later diagrams.
- **Repeated preamble provisioning is truly idempotent.** Setting the same content no longer rewrites the stored override, and content beyond the documented 512 KiB UTF-8 limit is rejected before it can be stored and silently truncated.

### Tests
- Expanded backend, frontend, packaging, Python compatibility, and CI-contract coverage for Mermaid messages, group preamble management, and Python 3.11–3.14 support.

## [0.4.32] — 2026-07-18

### Added
- **Peer Insight adds a structured second channel to agent-to-agent messaging.** Built-in MCP peer sends now carry operational `text` plus a provisional higher-order `insight`, with end-to-end projection through the ledger, Web UI, Group Bridge, IM, search, replies, files, and tracked delegation.
- **The Insight Loop places reflective checkpoints at collaboration boundaries.** Missing peer Insight is rejected before task, blob, wake, or ledger side effects; successful message operations return a short perspective reset; and bootstrap adds a conditional takeover cue when real unfinished work is recoverable.
- **The Web composer can recall loaded message history with Up and Down.** Recall restores text only while preserving the composer's current recipients, reply target, attachments, priority, and delivery mode.
- **Linux projected-browser readiness is visible in `cccc doctor`.** The report distinguishes the required system browser and Xvfb isolation from the optional x11vnc viewer.

### Changed
- **The default collaboration surface is leaner.** Ordinary actors receive a compact 13-tool protocol core, optional tools stay available through capability use, and general reasoning or writing doctrine has been removed from the base system prompt and help reference.
- **Heuristic nudges are off by default for newly created groups.** Generic nudge, unread, keepalive, and periodic help reminders no longer add background pressure unless configured; explicit reply-required and attention acknowledgement reminders remain available for reliability. Existing group settings are not migrated.
- **ChatGPT Web delivery can safely queue prompts while ChatGPT is already responding.** CCCC uses only a verified composer-local Send prompt control, defers without advancing the cursor when no safe control exists, and uses bounded single-flight retries instead of reporting a premature delivery failure.
- **Linux projected browsers now require CCCC-owned Xvfb isolation.** CCCC no longer falls back to the host desktop display; x11vnc remains optional because the embedded viewer can use CDP screencasting.
- **Grok Build PTY actors preserve provider sessions across restarts.** CCCC assigns an actor-specific session ID, resumes it with Grok's native flags, preserves user-owned session arguments, and exposes deliberate new-session restart behavior.
- **Release validation now uses durable quality boundaries.** Web checks run through Vite+ with Oxfmt/Oxlint, Python tests use deterministic PR shards plus a serial nightly reference run, Windows retains focused PTY smoke coverage, and package-only Web artifact contracts run after the bundle is built.

### Fixed
- **Ambiguous ChatGPT submit clicks no longer create automatic duplicate deliveries.** Confirmed pre-submit deferrals remain retryable, while dispatch-unknown clicks follow an explicit at-most-once policy.
- **Projected browser sessions no longer leak physical Chrome windows onto Linux desktops** when isolation dependencies are missing or persisted browser metadata is not trustworthy.
- **Recipient detail popovers remain usable when the pointer moves from the recipient chip into the popover**, and message history Escape handling no longer cancels an unrelated reply draft.
- **Terminal option changes no longer recreate the live xterm session**, preserving its WebSocket-bound input and resize behavior.
- **Deleted groups no longer leave stale Group Space memory bindings that interfere with synchronization.**

### Tests
- Expanded backend and frontend coverage for Peer Insight validation and projection, bootstrap takeover gating, ChatGPT browser delivery ambiguity and retry boundaries, Linux projected-browser isolation, Grok session reuse and rotation, composer history navigation, recipient popovers, quality workflow contracts, packaging, and Windows PTY smoke behavior.

## [0.4.31] — 2026-07-12

### Added
- **Actor environment variables now have a dedicated management surface** with masked configured keys, staged add/update/remove operations, batch paste, undo, and explicit clear-all handling.
- **Exited PTY sessions retain bounded terminal snapshots**, allowing terminal tail and history diagnostics to remain available after an actor stops or exits.

### Changed
- **PTY lifecycle operations are coordinated across actor, group, and global start/stop paths**, reducing duplicate sessions and start/stop races while preserving concurrency between unrelated groups.
- **Codex PTY state follows the terminal command when profiles, provider overrides, OSS mode, or local-provider flags make app-server state incomplete**. Grok Build and OpenCode default launches now include their autonomous execution flags.
- **Runtime and Group Bridge documentation is easier to navigate**, with dedicated guides, a generated release hub, a standards index, and refreshed CCCC branding assets.

### Fixed
- **Derived ledger indexes recover from corruption at every indexed read boundary**, rebuilding from the append-only ledger after explicit `SQLITE_CORRUPT` or `SQLITE_NOTADB` failures without masking lock, permission, or unrelated database errors.
- **Long Web chat histories preserve the user's visual anchor more reliably** while older messages are prepended and virtualized rows are remeasured.
- **Actor configuration keeps unfinished environment-variable drafts when switching advanced tabs**, refreshes configured keys across actor/edit sessions, and disables destructive clearing when key metadata cannot be loaded.
- **Terminal output is drained and preserved more reliably during fast exits and shutdown**, including Windows ConPTY sessions, with clearer exited-session diagnostics.
- **Agent mention suggestions prioritize concrete actors before broad recipient tokens**, making direct recipients easier to select.

### Tests
- Expanded backend and frontend coverage for PTY lifecycle concurrency, exited-session snapshots, Windows output draining, runtime-state inference, ledger-index corruption recovery, actor environment-variable drafts, Web history prepend compensation, and mention ordering.

## [0.4.30] — 2026-07-03

### Added
- **Slash skill commands can now dispatch as hidden agent turns**, letting users invoke active CCCC skills from the Web composer while keeping the agent-facing execution prompt out of the visible chat transcript.
- **Cross-group messages now carry receipt anchors for better reply continuity**, so replies from the source group can thread back to the remote message when CCCC has a recorded destination event.

### Changed
- **NotebookLM-backed workspaces and MCP setup are more robust**, with a refreshed vendored NotebookLM provider, stronger auth/session handling, better artifact/source/research support, and clearer MCP setup behavior.
- **Group Copy packaging is easier to use from Web and daemon clients**, including file-backed upload/preview flows and stricter package-size and input validation.
- **The Feishu IM bridge has been split into focused adapter modules**, making message, attachment, webhook, reaction, identity, and websocket behavior easier to test and maintain.
- **`@` in the Web composer is now treated as name completion only**. Message routing follows the explicit recipient/group controls instead of silently changing delivery just because a name was mentioned in the text.

### Fixed
- **Web chat scrolling and group switching are more stable**, including virtualized history anchoring, scroll restore, stale request handling, and actor/context refresh behavior.
- **Agent state updates return clearer confirmation after writes**, while preserving runtime-home isolation and guarding malformed hygiene metadata.
- **Cross-group send, slash skill dispatch, and remote receipt projection are idempotent across retries**, preventing duplicate hidden turns, duplicate target-group messages, and duplicate cross-group receipt anchors.

### Tests
- Added and updated coverage for NotebookLM provider scaffolding, MCP setup, Feishu adapter modules, Group Copy flows, Web chat scrolling, request routing, agent state confirmation, slash skill dispatch, cross-group reply routing, receipt hydration, and retry idempotency.

## [0.4.29] — 2026-06-24

### Added
- **Group Bridge became a first-class collaboration surface**, allowing trusted CCCC groups to exchange explicit cross-group messages, send attachments, and grant remote Messages, Read, or Full access.
- **Remote MCP tools are available for trusted groups**, including repository/context inspection for Read access and repository mutation or command execution for Full access.
- **The runtime catalog expanded** with first-class entries for Devin CLI, Kiro CLI, GitHub Copilot CLI, Antigravity CLI, Kilo Code CLI, and Cursor CLI.

### Changed
- **The Web UI treats remote groups as explicit recipients**, with clearer remote-group identifiers, improved composer recipient behavior, and a reorganized Group Bridge settings surface.
- **MCP guidance and repository search were tightened** so agents receive clearer send/reply guidance and more precise local or remote repository inspection tools.
- **Group Bridge and runtime documentation were promoted in the public docs**, including clearer Messages / Read / Full access-level explanations.

### Fixed
- **IM bridge and Web runtime behavior received a broad reliability pass**, including DingTalk media/reaction handling, bridge subprocess environment cleanup, stream-close handling, and cache invalidation after writes.
- **Cross-group send failures are surfaced as failures instead of successful sends**, making remote delivery state easier to trust.

### Tests
- Added and updated coverage for Group Bridge pairing, routing, remote delivery, remote MCP access, attachment transfer, reply relay behavior, runtime setup, repository search, IM bridge lifecycle, Web cache invalidation, and composer recipient behavior.

## [0.4.28] — 2026-06-18

### Changed
- **Local ASR maintenance is easier to understand**. Voice Secretary local ASR status now reports installed/latest sherpa-onnx versions, update availability, update errors, and artifact source details so users can tell when local speech components should be refreshed.
- **Neovate is no longer listed as a formal CCCC runtime**. The dedicated runtime, MCP setup path, Web metadata, docs, and logo asset have been removed; unsupported CLIs can still be wired through `custom`.

### Fixed
- **Local ASR model updates preserve the last verified install on failure**, so a failed download or replacement does not disable an already-ready model.
- **Web terminals suppress terminal-generated color/device query responses** that could otherwise appear as stray text in the input path with newer CLI/runtime combinations.
- **Fresh context reads now have deterministic cache-test coverage**, reducing CI flakes around stale in-flight context requests.

## [0.4.27] — 2026-06-15

### Added
- **Agents can now suggest the user's next message** by attaching `suggested_user_message` when sending or replying to the user. CCCC Web shows the suggestion as editable gray prefill text in the composer; the user can accept it with Tab or the inline suggestion button, edit it, or ignore it.
- **The suggested-message field is available through daemon messaging, Web messaging, and MCP messaging APIs**, with focused safeguards so suggestions are only surfaced for the active user-facing composer context.
- **ChatGPT Web Model delivery now includes a lightweight app-permission hint** when a browser-delivered message appears to be waiting on ChatGPT-side app approval.

### Changed
- **ChatGPT Web Model setup has been reorganized around the real setup order**: configure Web Access, create a ChatGPT Web Model actor in the target group, sign in to ChatGPT, connect the MCP app, then save an explicit delivery target.
- **ChatGPT delivery targets are now explicit and easier to reason about**. CCCC can bind to a saved conversation URL, use the current inspected ChatGPT chat when the user saves it, or start a new chat on the next delivery and bind it once ChatGPT creates the final `/c/...` URL.
- **Automatic ChatGPT page refresh recovery is disabled by default**. The legacy refresh mechanism remains available through `CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD=1` for fragile browser sessions.
- **CCCC no longer tries to automate ChatGPT app permission approval clicks**. Users should approve the CCCC app in ChatGPT, preferably with ChatGPT's "Always allow" option when they trust the local connector.
- **Legacy `## @pet` help blocks are treated as preserved legacy content**, not as an active assistant prompt surface.
- **Unsupported internal actors are skipped during group start/autostart**, preventing stale internal runtime records from being launched.

### Removed
- **PET and WebPet have been removed** from the daemon, MCP tool surface, Web UI, settings, release workflow, and generated Web assets. Voice Secretary remains the supported built-in assistant.
- **PET-specific local review, task proposal, reminder, context refresh, WebPet animation, and PET settings surfaces are no longer active product paths**.

### Fixed
- **Starting a new ChatGPT chat on next delivery now reports and persists the final bound conversation more reliably** after the first browser-delivered prompt.
- **ChatGPT target setup no longer treats diagnostic browser history as a saved delivery target**. A `last_tab_url` is informational only until the user saves a target.
- **Suggested next-message prompts no longer reappear after a newer user reply**, even when the chat view is filtered or showing a jump-to window.
- **Suggested next-message prompts are hidden when the composer is routed to another group**, preventing an agent-proposed reply for one group from being sent to another.

### Tests
- Added and updated coverage for PET removal boundaries, legacy help parsing, ChatGPT target drafting, browser recovery behavior, app-permission hints, new-chat binding, suggested-message contracts, and composer suggestion freshness/routing.

## [0.4.26] — 2026-06-10

### Added
- **First-class local memory daemon operations** expose search, read, write, profile, and health checks on top of CCCC's local ReMe-backed memory store.
- **Read-only terminal viewing** supports viewer-mode attaches without granting or stealing terminal write control.

### Changed
- **Web terminal sessions now preserve raw PTY output across attach and reconnect**, including byte-cursor replay, better viewer/control separation, and safer control takeover behavior.
- **Group sends no longer get blocked just because the Web UI currently sees no running actor**, allowing stopped groups to wake through the server-side send path instead of failing early in the browser.
- **Terminal transcript rendering is more accurate for wide CJK/fullwidth characters and alternate-screen TUIs**, making terminal tails and diagnostics easier to read.

### Fixed
- **Read-only exhibit terminal access can no longer take over a live PTY writer**, so public viewers cannot deny control to an active operator.
- **Changing Web theme or terminal scrollback no longer recreates the live xterm instance**, preventing terminal input/resize from silently detaching until reconnect.
- **Windows ConPTY installs now avoid the newly regressed pywinpty 3.0.4 release** while keeping the previously verified 3.0.3 path available.
- **Ledger index catch-up is serialized to avoid SQLite writer-lock failures** during concurrent ledger tail/search and append-index activity.
- **Headless Codex and Claude session shutdown now waits briefly for worker threads**, reducing stale runtime threads after stop/restart flows.
- **Reply uploads now reject invalid default recipients with a clear validation error** instead of creating a misleading send result.
- **Task reference chips have stronger dark-mode contrast**, and Web chat follow mode avoids forcing the bottom while the user is browsing detached history.
- **Foreman actors can remove peer actors through the expected management path**, matching the documented permission model.

### Tests
- Added and updated coverage for terminal attach modes, reconnect cursors, raw PTY replay helpers, read-only Web terminal access, runtime thread cleanup, local memory operations, ledger index locking, group send lifecycle projection, reply upload validation, terminal transcript rendering, and related Web typecheck paths.

## [0.4.25] — 2026-06-04

### Changed
- **The Web UI background no longer runs continuous decorative blob animations while idle**, reducing baseline GPU work and improving responsiveness on older desktops and integrated GPUs.

### Tests
- Validated the Web typecheck and production build after the background rendering change.

## [0.4.24] — 2026-06-04

### Added
- **Grok Build runtime support** is available across runtime detection, actor creation/editing, CLI setup, daemon runtime metadata, MCP setup checks, Web runtime selectors, runtime logos, and tests.
- **Reset group** is available from Web and CLI for creating a clean replacement group without manually deleting and recreating the group.

### Changed
- **MCP stdio framing now follows the client's inbound framing**, improving compatibility with plain newline-JSON MCP clients while keeping Content-Length framing for clients that use it.
- **Reset group preserves high-effort group configuration** such as actors, project scopes, stored secrets, and automation rules/settings, while intentionally leaving message history, memory, context, runtime sessions, and provider state behind.
- **The Web reset/delete group controls use a consistent tooltip style**, with reset guidance moved out of the modal footer into contextual hover help.

### Fixed
- **`cccc attach .` and `cccc group use` now resolve CLI paths before crossing the daemon boundary**, so running them from a project console no longer depends on the daemon's current working directory.
- **Web attach requests reject relative paths with a clear error**, avoiding accidental interpretation relative to `CCCC_HOME` or the daemon process directory.

### Tests
- Added and updated coverage for Grok runtime defaults and MCP setup, MCP stdio framing, reset group behavior and permissions, CLI active-group handling after reset, and attach path resolution through CLI and Web routes.

## [0.4.23] — 2026-06-01

### Changed
- **Web chat history now uses a single chronological search contract** for initial tails and older-history pagination, so the latest message window and follow-up history loads connect in normal conversation order.
- **NotebookLM work notebook sync is explicit by default**: plain `context_sync` no longer uploads repeated internal context snapshots into the work notebook.
- **Group Space source and artifact lists can use cached remote snapshots**, reducing routine NotebookLM provider calls while keeping explicit fresh reads available.
- **Automation nudge scans are throttled per group**, lowering repeated obligation-scan load without changing configured nudge windows.
- **Daemon logging is quieter for HTTP and NotebookLM provider internals**, making debug logs easier to read.

### Fixed
- **Ledger history recovery is more robust** when the sqlite ledger index becomes stale or incomplete while the append-only ledger still contains the messages.
- **Full event-id lookups can fall back to the ledger** after an index miss instead of treating the event as absent.
- **Windows ConPTY sessions now drain final output from fast-exiting commands**, fixing cases where quick commands appeared to emit only terminal control sequences.
- **Copy Groups zip downloads now use Unicode-safe download headers** with an ASCII fallback and `filename*` value.
- **Stale Group Space jobs left running by an earlier daemon** are reconciled into a failed state with a clear stale-job reason.

### Tests
- Added coverage for ledger index repair, chat history ordering and pagination, Copy Groups Unicode download headers, NotebookLM/Group Space cached lists and stale job reconciliation, automation nudge throttling, and Windows ConPTY fast-exit output draining.

## [0.4.22] — 2026-05-29

### Added
- **Accessible actor profile listing** lets non-admin users see global profiles and their own user-scoped profiles through one server-enforced view, making profile selection simpler without exposing other users' private profiles.
- **Image attachment hover previews** in the chat composer show a bounded preview before sending supported image files.
- **Persistent DingTalk conversation routing** keeps recent reply webhook and 1:1/group routing metadata across bridge restarts.

### Changed
- **Delivered runtime messages are now marked read by default** after successful PTY delivery, keeping actor unread state aligned with what has already been handed to the runtime. Existing explicit `auto_mark_on_delivery=false` settings are still respected.
- **Codex command normalization now sees the daemon environment**, so daemon/Docker-level `OPENAI_BASE_URL` and related environment values can affect Codex launch config without being repeated per actor. Actor-specific environment still takes precedence.
- **Hermes PTY actors now receive safer terminal defaults** for the TUI launch path while preserving explicit user overrides.

### Fixed
- **Codex app-server authentication failures** are detected from 401 stderr output and recorded as non-resumable runtime session state instead of leaving stale resume metadata looking usable.
- **DingTalk 1:1 reply fallback** now uses DingTalk's one-to-one API even when the conversation id looks like a group-style `cid`, reducing failed replies after webhook expiry.

### Tests
- Added coverage for Codex auth-failure session state, DingTalk conversation persistence and routing, Codex daemon environment command normalization, Hermes PTY environment defaults, delivery auto-read defaults and explicit opt-out behavior, actor profile accessible views, and composer image preview positioning.

## [0.4.21] — 2026-05-25

### Added
- **OpenCode runtime support** is now available across actor creation/editing, runtime profiles, CLI runtime choices, setup diagnostics, runtime detection, Web runtime metadata, and Web runtime avatars.
- **OpenCode MCP runtime injection** configures CCCC's MCP server through OpenCode runtime environment at actor launch time, avoiding global OpenCode config edits.
- **New Session control for Claude and Codex actors** in the Web actor detail panel starts a fresh runtime session with the same actor settings after confirmation.

### Changed
- **Codex Docker endpoint configuration** now maps `OPENAI_BASE_URL` to Codex CLI's `openai_base_url` runtime config when Codex actors start, covering both PTY and app-server backed launch paths.
- **Runtime selection in actor configuration** is searchable, making the full supported runtime list easier to discover.
- **Docker docs and `.env` examples** now document `OPENAI_BASE_URL` as CCCC's compatibility entry for Codex custom endpoints.

### Fixed
- Fixed Docker/Compose Codex deployments only honoring `OPENAI_API_KEY` while ignoring custom `OPENAI_BASE_URL` endpoints.
- Fixed the operator path for stale or unwanted Claude/Codex resume metadata by adding an explicit Web action that clears CCCC's saved session metadata and starts fresh without deleting provider-side history.

### Tests
- Added coverage for OpenCode runtime/MCP setup, Codex base URL config injection, Web/daemon New Session behavior, unsupported runtime rejection, and related Windows diagnostics.

## [0.4.20] — 2026-05-22

### Added
- **Voice Secretary recording diagnostics** now report clearer stop reasons for browser and local ASR paths, including microphone track endings, muted tracks, local ASR websocket failures, backend errors, and audio-context interruptions.
- **Voice Secretary document refresh fallback** now detects external document changes while the Doc panel is open by polling document metadata and loading full content only when the active document revision changes.
- **Message request lanes and chat diagnostics** add better isolation and evidence for chat send/reply flows, improving durability around concurrent message requests and short-circuit failures.
- **Actor configuration modal unification** replaces the separate Add Actor modal with a shared add/edit surface, reducing UI drift between actor creation and editing.

### Changed
- **Local ASR and Browser ASR document-mode stop behavior** is now more consistent: stopping Local ASR can trigger the same final document-refinement path as Browser ASR without requiring a separate manual instruction.
- **Voice Secretary activity and transcript surfaces** are quieter and denser, focusing Activity on live/request/reply feedback while keeping document transcripts in the document workspace.
- **Voice Secretary document polling** now uses low-noise metadata-change detection instead of frequent full-content refreshes.
- **Web UI surfaces** were polished across modals, search, message bubbles, runtime details, sidebar items, typography, and markdown/code-block rendering.
- **Slash command filtering** now prioritizes command names over noisy short description matches.
- **Vite development proxying** now honors `CCCC_WEB_HOST` and `CCCC_WEB_PORT`, making local Web development less dependent on the default `127.0.0.1:8848`.

### Fixed
- Fixed Local ASR normal websocket closes being reported as unexpected recording failures after a successful document-mode stop/save.
- Fixed Voice Secretary final transcript spacing around ASCII punctuation and improved streaming/offline Sherpa worker shutdown and JSONL handling.
- Fixed no-op draft handling so empty or meaningless voice input is less likely to pollute composer output.
- Fixed composer send gating while group actors are still hydrating.
- Fixed several daemon messaging and ledger reliability issues, including lane isolation, reply idempotency, reverse lookup, compact-threshold behavior, and diagnostics for short-circuit message requests.
- Fixed MCP blob/file MIME guessing so Markdown attachments are reported as `text/markdown`.
- Fixed test isolation around remote access environment variables, IM startup process spawning, and pre-commit timeout handling.

### Tests
- Expanded coverage for Voice Secretary ASR, document finalization, service runtime behavior, chat diagnostics, message lanes, ledger indexing/segmentation, MCP attachment MIME handling, Web UI modal/composer behavior, slash command filtering, and Voice Secretary document polling support.

## [0.4.19] — 2026-05-19

### Fixed
- **PTY runtime resume recovery** now falls back to a fresh actor when a saved provider resume fails, while preserving durable fresh-session metadata for Claude and Gemini where explicit provider session IDs are supported.
- **Codex app-server backed PTY relaunches** now record fresh remote TUI thread ids after stale resume metadata falls back to a new thread, preventing repeated relaunch failures on the same invalid thread.
- **Windows actor restart behavior** now handles Codex command shims, app-server process-tree termination, and PTY terminal capability queries more reliably.
- **MCP runtime context recovery on Windows** can recover CCCC runtime identity from parent and ancestor process environments when the immediate MCP process environment is incomplete.

### Changed
- **Codex remote TUI startup and shutdown** now distinguish provider thread failures from observer disconnects, use bounded websocket shutdown, and avoid marking resume metadata failed when only the remote TUI surface exits.

### Tests
- Expanded coverage for Windows PTY query replies, Windows MCP context recovery, Codex app-server thread resume/fallback, remote TUI shutdown semantics, and PTY resume failure recovery across Claude, Gemini, and Codex.

## [0.4.18] — 2026-05-18

### Added
- **Hermes runtime support** across CLI, daemon IPC, actor startup, MCP setup, runtime selectors, Web runtime display, and tests. Hermes uses the selected user Hermes home/profile, with explicit `HERMES_HOME` respected as a normal user override.
- **Hermes runtime diagnostics and setup commands**, including status, prepare, and MCP test operations that validate the Hermes CLI, auth/config state, launch readiness, and actor-scoped CCCC MCP environment placeholders.
- **Daemon-owned Voice Secretary recording leases** so browser recording is guarded across tabs, browsers, and devices with TTL-based recovery.
- **View-aware Voice Secretary state and document content APIs** so the Web workspace can load compact status snapshots or document content explicitly instead of over-fetching full assistant state.
- **Codex app thread resume/start support** for app-server backed sessions, giving Codex runtime sessions a clearer provider thread lifecycle.

### Changed
- **Voice Secretary ASR and document flow** now emits final ASR text, merges CJK transcript chunks more naturally, and uses tighter notification cursors so fresh input is delivered without stale or duplicate nudges.
- **Codex and runtime session restart behavior** is more conservative around observer disconnects, bootstrap-control failures, explicit fresh-session requests, and stale provider session metadata.
- **Web chat synchronization** now uses a shared SSE connection registry, better request freshness guards, and composer-state recovery after failed sends.
- **Runtime avatars** now treat provider logos as branded assets on a stable light logo plate; ChatGPT Web Model reuses the Codex logo and Hermes has a dedicated logo asset.
- **Voice Secretary Web surfaces** have a quieter activity stream, clearer document loading, and less transient process noise.

### Fixed
- Fixed projected browser launch compatibility on macOS.
- Fixed Codex PTY actors being stopped accidentally when an observer-side connection closes.
- Fixed Codex restart/session edge cases that could leave app-server state, PTY state, or saved session metadata out of sync.
- Fixed unnecessary slash-command refresh work after formal event updates.
- Fixed IM sender identity and mention propagation paths, with additional WeCom adapter hardening.

## [0.4.17] — 2026-05-14

### Added
- **Codex PTY app-server state source** for interactive Codex actors. Codex can now keep a PTY surface for the user while CCCC derives runtime activity from Codex app-server events instead of relying only on terminal text heuristics.
- **Runtime state source controls** across daemon, CLI, Web API, and actor contracts, currently scoped to Codex PTY actors that opt into app-server-backed state.
- **WeCom media bridging improvements**, including outbound file/media upload through the WeCom AI Bot WebSocket chunk protocol and active outbound sends when no callback reply handle is available.

### Changed
- **Codex PTY lifecycle handling** was tightened so remote TUI exit stops the backing app-server session, disabled actors project as stopped even if stale runtime state remains, and app-server-backed PTY actors avoid duplicate bootstrap queuing.
- **PTY activity detection** was split into reusable terminal-state helpers with better Claude prompt/working detection and safer Codex prompt-versus-working ordering.
- **Runtime dock and actor list state** now treat app-server-backed Codex PTY actors like structured runtime sessions for activity rings while preserving their PTY runner identity for terminal access.
- **Chat scrolling behavior** now clears stale scroll affordances more reliably when the user is already at the bottom or jumps to a specific message.

### Fixed
- Fixed Codex PTY state false-idle cases where an older prompt line could override a newer visible `Working (...)` banner.
- Fixed app-server-backed Codex PTY tests so CI does not depend on the runner having the Codex CLI installed.
- Fixed disabled actor projections that could still appear active when stale headless/app-server state existed.
- Fixed runtime avatar rendering for Claude by using an inline Claude logo where the packaged runtime logo path is not appropriate.

## [0.4.16] — 2026-05-13

### Added
- **Durable runtime session resume state** for supported provider runtimes. CCCC now stores provider session metadata under each group so actors can relaunch into the same Claude, Codex, or Gemini session when the runtime supports explicit resume.
- **PTY runtime resume support** for Claude, Codex, and Gemini with provider-specific session capture: generated explicit session IDs for Claude/Gemini, Codex `/status` session detection, stale-resume fallback, and safeguards against PTY/headless session mix-ups.
- **Headless runtime session metadata** for Claude and Codex app sessions, including provider session/thread recording and guarded native-resume handling where supported.

### Changed
- **Actor terminal connection handling** was extracted into a reusable hook, reducing AgentTab complexity and making PTY attach, reconnect, terminal signals, and runtime transitions easier to maintain.
- **Runtime state synchronization in Web** now uses one unified actor snapshot path for ordinary actors and built-in runtime actors, while `actor.activity` updates merge into both stores. Runtime dock state is less likely to appear stale until a manual page refresh.
- **Composer and Voice Secretary UI behavior** were tightened around cross-group recipients, mobile/voice layout, live transcript preview behavior, and attachment/action plumbing.
- **MCP install and runtime startup checks** were hardened so already-installed runtime MCP configurations are handled more predictably during actor startup.

### Fixed
- Fixed runtime dock desynchronization where an actor could be running in the daemon but still appear stopped in the Web UI until refresh.
- Fixed noisy terminal attach loops when switching between PTY and headless actors, including repeated `terminal attach is only available for PTY actors` messages.
- Fixed stale or rejected runtime resume metadata so failed provider resume attempts fall back to fresh starts and do not poison subsequent launches.
- Fixed Copy Groups export so runtime session metadata remains excluded from copied group packages.
- Fixed built-in assistant startup/profile synchronization so PET and Voice Secretary use their own configuration instead of inheriting foreman runtime details.

## [0.4.15] — 2026-05-10

### Added
- **Copy Groups** export/import for durable group duplication, migration, and backup. Copy packages include group ledger, actors, memory, attachments, automation, and durable settings while excluding workspace files, live credentials, browser sessions, caches, and runtime state.
- **Capability Center lifecycle management** for discovering, importing, enabling, hiding, removing, and inspecting skills, MCP toolpacks, and capability packs from one Web surface.
- **Slash command support for capability workflows**, including `/install` routing through the CCCC capability registry and capability-backed slash command discovery in chat.
- **VNC-backed projected browser viewing** for CCCC-owned Xvfb browser sessions, with noVNC integration and CDP screencast fallback when VNC is unavailable.

### Changed
- **Group blueprints/templates were replaced by Copy Groups** as the supported group copy/migration path.
- **ChatGPT Web Model delivery** now distinguishes submitted, ambiguous, pending, and failed delivery states more explicitly, avoids automatically resending previously failed batches, and exposes clearer user-message delivery status.
- **Projected browser surfaces** now separate visual viewing from CDP automation more cleanly. VNC is used only for CCCC-owned displays, while inherited host desktops stay on the safer CDP fallback path.
- **Capability overview and source management** were optimized with lighter overview requests, kind counts, source-instance deletion, SkillsMP deduplication, and fewer expensive aggregation calls from list-only UI surfaces.
- **Web composer and settings behavior** were tightened around slash commands, group selection, recipient persistence, markdown code block rendering, and settings initialization.

### Fixed
- Fixed capability mutation APIs so a failed `capability.changed` notification no longer makes an already persisted enable/visibility change appear to fail.
- Fixed install/reinstall flows where a capability could remain hidden from slash surfaces after being installed again.
- Fixed GitHub capability source instance grouping so root-level skills from different refs remain distinct.
- Fixed Web readiness timeouts on slower machines by making the Web child readiness window less brittle.
- Fixed projected browser VNC safety so CCCC does not expose an inherited local desktop display through noVNC.
- Fixed Markdown code block rendering that could wrap CCCC's code block UI inside redundant nested `<pre><code>` containers.

## [0.4.14] — 2026-05-07

### Added
- **Session-scoped No-MCP advisory groundwork** for future web agents that cannot attach a CCCC MCP connector, with read-only project context, bounded resource access, and idempotent advisory-message return.
- **Localized ChatGPT Web Model settings** in English, Simplified Chinese, and Japanese, under a provider-scoped Web Models i18n structure that leaves room for future web-model providers.

### Changed
- **ChatGPT Web Model setup documentation** now follows the current ChatGPT Apps/Connectors flow, including public HTTPS tunnel guidance, the fields to fill, the `No Auth` MCP configuration, and a note that exact ChatGPT menu names may vary by plan and workspace.
- **GPT-5.x / GPT-5.x Pro messaging** now clearly positions MCP-capable GPT-5.x ChatGPT sessions as the supported local-development path, while documenting GPT-5.x Pro as advisory-only because it cannot reliably access CCCC MCP or local resources.
- **ChatGPT Web Model browser status handling** now uses cheaper cached status for routine reads and reserves live browser inspection for explicit actions such as opening the surface or binding the current ChatGPT tab.
- **Composer recipients** are preserved per group for normal sends and restored after reply flows, reducing repeated recipient selection without leaking recipients across groups.

### Fixed
- Fixed ChatGPT Web Model setup/open/bind flows that could show stale or inactive browser state after opening the global setup surface or binding the current tab.
- Fixed Web Model settings copy and i18n structure that treated all future Web Model providers as if they were ChatGPT-specific.
- Fixed release and setup documentation that still implied a No-MCP fallback could make GPT-5.x Pro a reliable local runtime.

## [0.4.13] — 2026-05-04

### Added
- **ChatGPT Web Model runtime** for GPT-5.x ChatGPT web sessions, including connector setup, browser delivery, target-chat binding, and runtime panel visibility.
- **Remote CCCC MCP local-development tools** for ChatGPT Web Model actors: repo reads/edits, Codex-style patching, shell/exec, git, and file attachment send/read paths.
- **`cccc_code_exec` code mode** for higher-throughput Web Model work loops, with nested MCP tool orchestration, tool discovery helpers, common work loops, and actor-scoped cells.
- **Voice Secretary local ASR stack** with Sherpa ONNX streaming/final recognition, speaker diarization, model manifest handling, and local cache controls.
- **ChatGPT Web Model health snapshot** exposing derived browser, target, delivery, and recommended next-action state to Web UI and API callers.

### Changed
- **ChatGPT browser delivery** now uses the daemon-managed projected browser session by default, records stronger submission evidence, avoids duplicate pending new-chat delivery, and keeps setup/runtime surfaces on one shared ChatGPT browser profile.
- **Web Model setup** was simplified around one ChatGPT Web Model actor per CCCC instance, a single MCP URL, and clearer ChatGPT account/target-chat controls.
- **MCP help and toolspecs** now better explain common Web Model loops, attachments, turn completion, and when to use `cccc_code_exec` versus direct tools.
- **Voice Secretary Ask/Document/Prompt flows** were tightened after the ASR work so live transcript previews, document transcripts, Ask replies, and composer draft submissions stay separated.
- **README and docs** now describe GPT-5.x ChatGPT Web Model as the full local-development path, while GPT-5.x Pro is documented as review/advice-only because current ChatGPT Pro sessions cannot use third-party MCP with full local access.

### Fixed
- Fixed ChatGPT prompt submission cases where inserted text could be mistaken for successful delivery without proof that ChatGPT actually accepted the prompt.
- Fixed pending new-chat binding so already-submitted events are not resent while waiting for ChatGPT to expose the `/c/...` conversation URL.
- Fixed stale Web Model auto-confirm watchers and tightened connector/runtime gating for local-power MCP tools.
- Fixed internal actors, including Voice Secretary, from accidentally being counted as or converted into ChatGPT Web Model actors.
- Fixed Docker Node.js installation so image builds include `npm`, and suppressed noisy managed-node deprecation warnings.
- Fixed Voice Secretary ASR model/status cache edge cases and transcript display regressions.

## [0.4.12] — 2026-04-23

### Added
- **Voice Secretary workspace** with repository-backed markdown documents, Document/Ask/Prompt modes, request history, transcript feedback, document archive/download actions, and dedicated assistant reporting paths.
- **Built-in assistant controls** in the chat composer for PET and Voice Secretary, plus a dedicated assistant settings surface with larger prompt editors.
- **Daemon-owned tracked delegation** with task/message linkage, idempotency hardening, and task chip projection in chat messages.
- **`cccc update` command** with install-source detection.

### Changed
- **Voice Secretary input handling** now routes stable transcript/request data through dedicated assistant surfaces instead of noisy chat-style JSON notifications.
- **Headless delivery and inbound rendering** were tightened for Codex and Claude, including clearer sender/recipient context and safer reply routing.
- **Runtime Dock state projection** was polished so PTY/headless actors and built-in assistants expose clearer idle, active, and stopped states.
- **Web workspace and settings UI** received broad composer, modal, assistant, capability, automation, copy, and markdown rendering polish.
- **Collaboration follow-up settings** were renamed and regrouped around operator-facing behavior instead of internal automation terminology.

### Fixed
- Fixed partial-failure retry handling for tracked delegation so retries do not duplicate tasks after message-send failures.
- Fixed Voice Secretary prompt-refine, Ask reply, idle-review, and document routing edge cases.
- Fixed headless reply recipient routing for ambiguous sender/source contexts.
- Fixed user message bubble background regression after text-color unification.
- Fixed actor secret placeholder examples for Codex and Claude Code so the UI no longer suggests ineffective OpenAI environment variables for Codex.
- Fixed WeCom response URL fallback behavior for streaming and media replies.

## [0.4.11] — 2026-04-11

### Added
- **Weixin bridge now uses the Python `wechatbot-sdk` integration** instead of the previous packaged Node.js sidecar, reducing bundled bridge assets and keeping Weixin login, inbound, media, and outbound behavior inside the Python adapter stack.
- **Weixin subscription guidance in Web settings**: after Weixin login, the IM Bridge panel now explicitly explains the `/subscribe` and Pending Requests flow, including separate guidance for unconfigured, stopped, already-bound, and ready-to-subscribe states.
- **Runtime Dock ring tone coverage** extracted into a dedicated helper so PTY and headless actors share clearer `stopped`, `ready`, `queued`, `active`, and `attention` state mapping.
- **Mention suggestion labels in chat composer** now show actor display labels with secondary IDs where useful, and the mention preview can be closed with Escape.

### Changed
- **Weixin packaging was simplified** by removing the Node sidecar packages and packaged `.mjs` resources; `wechatbot-sdk>=0.2.0` is now the Python dependency for the Weixin path.
- **Headless streaming reconciliation** was tightened so pending placeholders, canonical reply sessions, stream-id promotion, and terminal reply phases are less likely to reset or regress after final replies.
- **Runtime state projection** now writes stopped actor entries to the ledger when actors disappear from runtime snapshots, helping the Web clear stale working halos and live indicators.
- **Actor edit modal synchronization** was cleaned up so profile-backed actors and custom actors open with settings that better match their current stored configuration.
- **Web group runtime updates** now use SSE/runtime projections more consistently, reducing sidebar state drift after refreshes or lifecycle changes.

### Fixed
- Fixed CLI daemon fallback behavior so daemon rejections are not incorrectly treated as permission to fall back to local mutations.
- Fixed Weixin outbound/context-token handling so cached SDK context can be rehydrated into the running bot and outbound readiness survives bridge restarts more reliably.
- Fixed Weixin IM configuration canonicalization for empty and legacy account fields, with additional route and adapter coverage.
- Fixed missing visibility for `actor.activity` ledger append failures by logging append errors instead of silently swallowing them.
- Fixed EventKind documentation parity gaps by adding internal contract coverage.
- Fixed a bare Chinese placeholder in the Weixin settings panel by moving it into the English, Chinese, and Japanese locale files.

## [0.4.10] — 2026-04-10

### Added
- **Claude headless runtime support** alongside the generalized headless streaming pipeline, enabling structured headless sessions beyond Codex.
- **Runtime Dock and live trace surfaces in Web**: richer headless previews, compact activity timelines, grouped runtime inspectors, and better runtime state projection across chat and actor views.
- **Headless runtime plumbing and test coverage**: cache/projection helpers, broader coverage for headless events, runtime startup, Web actor routes, and Windows PTY behavior.
- **Weixin sidecar support refreshed** with expanded IM bridge adapter-level validation.

### Changed
- **Headless delivery architecture** generalized from Codex-specific to a shared headless model used consistently across daemon, Web, and MCP surfaces.
- **Web chat and runtime UX** significantly refined: message reconciliation, activity persistence, runtime previews, composer behavior, per-message identity rendering, and group runtime controls.
- **Workspace and presentation flows** streamlined through enhanced browser/presentation handling and more resilient scope attachment behavior.
- **CI and release verification** strengthened with longer timeout coverage, `pytest-timeout` in smoke paths, and tighter release workflow checks.

### Fixed
- Fixed multiple **headless reply and streaming lifecycle regressions**, including fallback flow errors, message identity drift, canonical-reply reconciliation, and startup-state mismatches.
- Fixed **missed headless injection for automation-generated `system.notify` events** — automation-triggered notifications now reach running headless agents instead of only landing in the inbox.
- Fixed **PTY teardown hardening**, unread/index parity edge cases, and additional delivery flow stabilization.
- Fixed **Web chat rendering regressions**: lost avatars, unstable activity bubbles, stale runtime preview state, and composer alignment under text scaling.
- Fixed **group start/pause button UI not updating** — `setGroupDoc` now syncs `runtime_status.lifecycle_state` immediately, and `refreshGroups` patches `runtime_status` and `running` from server meta so stale local state no longer overrides the authoritative status.
- Fixed **Windows PTY wake-path locking** problems and related test instability.
- Fixed **IM integration reliability**: DingTalk mention handling, Weixin sidecar SDK pinning, and adapter behavior.

## [0.4.9] — 2026-04-05

### Added
- **WeChat (Weixin) IM bridge**: Node.js sidecar, CLI login/logout, QR-code auth in Web UI, and daemon routes for bridge lifecycle, following the same bind-key authorization model as other adapters.
- **Text-size accessibility control**: three-tier scale selector (90% / 100% / 125%) persisted per-browser. System-theme icon changed to a display icon; mobile menu now cycles light → dark → system.
- **Async result contract**: formal `async_result` IPC signaling (accepted/completed/queued) across daemon actor operations.
- **Assistive-jobs layer**: explicit pet review and profile job kinds requiring verified completion before marking done.

### Changed
- **Actor launch pipeline unified**: add/update/lifecycle/runtime operations now share one resolution path with consistent async-result semantics; group start/stop reliably awaits per-actor results.
- **Pet runtime tracks group settings**: desktop-pet enablement syncs with group-settings changes.
- **Web Pet task advisor is local-first**: local evidence evaluation before surfacing proposals, reducing speculative noise.
- **Presentation viewer** gained inline web-preview support, topic-aware slide navigation, and split-layout mode for simultaneous conversation and viewing.
- **Group sidebar** extracted into a standalone component with optimized chunk splitting.
- **MCP dynamic capability tools** reflect real-time actor state.

### Fixed
- Fixed idle-standup suppression and silence-activity filters to stay quiet when there is genuinely nothing to act on.
- Fixed runtime visibility controls so peer and pet tabs show/hide based on actor composition.
- Fixed context sync and group-space writeback to distinguish accepted vs. completed status.
- Fixed automation snippet catalog separation so built-in overrides are distinct from user-authored rules.

## [0.4.8] — 2026-03-30

### Added
- **Windows-friendly env snippet support** in the Web secret editor: `set KEY=VALUE` and `$env:KEY="VALUE"` forms accepted alongside Unix-style entries.
- **Terminal-derived working state** exposed in the actor list for richer runtime visibility.
- **Modularized Web API services** for a cleaner frontend integration layer.

### Changed
- **Web Pet** substantially reworked: review scheduling, reminder generation, decision handling, and task proposals are more reliable and less noisy.
- **Actor startup** gained stronger runtime preflight checks and clearer daemon transport diagnostics.
- **Peer-created MCP tasks** now default to self-assignment instead of unassigned.
- **Ledger and unread-index paths** made faster with reduced overhead.

### Fixed
- Fixed projected browser session reliability for embedded views and NotebookLM/Google auth flows.
- Fixed task status and update flows being fragile under rapid MCP task operations.
- Fixed Web-to-daemon messaging semantics and context/chat UI behavior after the Web API modularization.

## [0.4.7] — 2026-03-23

### Added
- **Presentation workspace**: slot-based presentation content managed through daemon, Web, and MCP, with a dedicated Chat Presentation rail and viewer flow.
- **Browser-backed presentation views**: interactive viewer lifecycle with refresh, fullscreen, replacement, and URL entry.
- **Presentation references in chat**: messages can point to a specific Presentation view with snapshot and compare support.
- **Web branding controls**: product name and logo asset configuration from the Web settings surface.

### Changed
- **Task state handling** in Web UI is more structured; task/context workflow logic is tighter.
- **MCP task update compatibility** improved so status changes are less fragile.
- **Default `cccc` entry path** now respects top-level `--host` / `--port` overrides throughout supervised Web startup and restart.
- **Kimi runtime defaults** updated to match the current preferred path.

### Fixed
- Fixed group/context/unread refresh behavior for better state coherence post-mutation.
- Fixed general message, panel, and console usability issues across the Web surface.

## [0.4.6] — 2026-03-19

### Added
- **WeCom IM bridge**: dedicated adapter, Web-side bridge settings, authentication/readiness behavior, and operator docs including a dedicated WeCom setup guide.
- **Built-in role presets**: first-wave roster (planner, implementer, reviewer, debugger, explorer) with a faster preset-application UI for common actor role starting points.

### Changed
- **Web context and actor route caching** made more deliberate with proper invalidation after writes, reducing stale readback after actor/context updates.
- **Prompt and help surface** tightened so startup guidance stays lean; richer guidance lives in the help/preset layers.
- **Web readiness checks** now tolerate `OSError` and `HTTPException` instead of surfacing brittle failure behavior.

### Fixed
- Fixed Windows shutdown cleanup for lingering process/lifecycle edge cases.
- Fixed cache invalidation after actor/context writes to prevent stale Web UI state.
- Fixed WeCom adapter startup and config flows.

## [0.4.5] — 2026-03-18

### Added
- **Web Pet panel**: task progress, smarter hints, direct jump to chat/task, and post-stop terminal output snippet after an agent ends a session.
- **Web health endpoint** made publicly reachable for external health checks and probing.
- **Supervised Web restart/apply flow** surfaced more clearly from the main `cccc` session.

### Changed
- **Desktop pet surface removed**: Web Pet is now the primary pet surface (previous Tauri-based implementation retired).
- **Web Access panel** better aligned with real operator goals: local-only, LAN/private, and externally exposed access postures are clearer.
- **POSIX background Python startup** now preserves the active virtualenv interpreter path instead of resolving to system Python.

### Fixed
- Fixed Windows Codex MCP setup to prefer a stable absolute `cccc` entrypoint and avoid false "already installed" detection.
- Fixed supervised Web child shutdown so Ctrl+C and restart flows behave predictably.
- Fixed fail-fast MCP startup checks to avoid over-blocking unrelated lifecycle flows.
- Fixed IM bridge child-process startup to follow the same safer background-process rules as the daemon/Web stack.

## [0.4.4] — 2026-03-16

### Changed
- **Group settings**: Guidance is now the default first-open tab, matching the visible tab order.
- **Settings terminology** now more clearly separates built-in automation from user-authored rules and snippets.
- **Delivery panel** simplified to the only user-facing behavior that remains: PTY delivery auto-advance of the read cursor.
- **Actor idle alerts** default to `0` (off) for new/default/reset paths without silently changing existing stored values.

### Removed
- **`min_interval_seconds`** removed from the Web settings UI (daemon/API compatibility preserved).

## [0.4.3] — 2026-03-15

### Added
- **User-scoped actor profiles** working end-to-end across daemon, Web, and MCP paths.
- **NotebookLM runtime guidance** injected into the help layer only when the relevant capability is actually active.

### Changed
- **Guidance stack re-layered**: startup prompt is slimmer; live capability guidance is in `cccc_help`; actor role notes are canonically stored in group help `@actor` blocks instead of leaking into working-state fields.
- **Task authority model tightened**: `task.restore` follows an archived-only precondition; peers can no longer mutate unassigned tasks outside their own scope.
- **`agent_state` semantics aligned** across docs, daemon behavior, MCP tooling, and Web expectations.
- **Group Space bind/unbind status** now tracks the current binding accurately without leaking stale sync residue after rebind cycles.
- **Runtime support surface narrowed** to runtimes CCCC can set up and operate reliably; standalone Web startup follows the same local-first binding model as the main CLI.

### Fixed
- Fixed blueprint export/import round-trips so portable fields survive a full cycle without divergence.
- Fixed Windows MCP reliability: runtime-context resolution and stdio/encoding robustness.
- Fixed DingTalk sender identity, revoke behavior, `@` targeting, and streaming fallback edge cases.
- Fixed Web settings, modal overflow, context presentation, and translation coverage gaps.
- Fixed global browser surfaces to default-scope users to relevant data, reducing machine-global noise for scoped users.

## [0.4.2] — 2026-02-22

### Added
- **IM key-based chat authorization**: dynamic bind-key authentication for IM bridges, replacing static trust with per-chat cryptographic binding. Includes `/bind` command, auto-subscribe on successful bind, pending approval management, and revoke semantics.
- **`cccc_im_bind` MCP tool**: programmatic chat authorization via the MCP surface.
- **Authorized chats Web UI**: view/manage bound IM chats and pending bind approvals from Settings → IM Bridge tab.
- **Bind key UI**: generate and display bind keys for chat authorization directly from Web settings.
- **Actor profiles system**: profile linking across daemon, Web, and MCP — including profile runtime, persistent store, and a dedicated Actor Profiles settings tab.
- **Remote access control plane**: remote daemon access with hardened IM revoke semantics for secure multi-node operation.
- **Telegram typing indicators**: typing action support with configurable throttling for more natural conversational UX.
- **IM authentication IPC documentation**: new standards doc covering IM auth IPC methods.

### Changed
- **IM display names**: prefer actor titles over raw actor IDs in IM-rendered messages for better readability.
- **Modal UX refinements**: extracted modal close handlers and adjusted inbox modal height for cleaner interaction.

### Fixed
- Fixed MCP message send incorrectly collapsing `None` recipients to empty list, breaking broadcast semantics.
- Fixed `authorized_at` timestamp handling in IM Web UI.
- Fixed IM KeyManager state not reloading from disk on each inbound poll, causing stale authorization data.
- Fixed docs incorrectly requiring post-bind `/subscribe` step (now handled automatically).

## [0.4.1] — 2026-02-20

### Added
- **Actor lifecycle event coverage**: daemon streaming now emits fuller actor/group lifecycle transitions for better observability and downstream automation hooks.
- **Secret safety UX upgrade**: actor secret keys now support masked previews in Web edit flows, improving operator confidence without exposing plaintext.
- **Branding assets refresh**: project logos were added and integrated into Web/README surfaces for consistent distribution identity.

### Changed
- **Automation idle semantics** were aligned with explicit group-state behavior, reducing ambiguity in scheduled reminder execution during `idle` mode.
- **Terminal safety hardening**: resize and related terminal maintenance paths were tightened to avoid unstable behavior in mixed runtime conditions.
- **Docs and onboarding** were updated to match current `v0.4` behavior, including SDK entry points, release hub linking, and refreshed top-level README content.

### Fixed
- Fixed lifecycle edge cases where actor/group state transitions and event emission could diverge.
- Fixed multiple test-surface instability points (especially MCP/environment isolation), improving release reproducibility.

## [0.4.0] — 2026-02-16

### Added
- **Chat-native orchestration model**: operators can assign and coordinate work in a persistent Web conversation, with full delivery/read/ack/reply state tracking.
- **External IM extension of the same workflow**: Telegram, Slack, Discord, Feishu/Lark, and DingTalk bridges allow the same group control model outside the browser.
- **Prompt-configurable multi-agent workflow design**: guidance prompts and automation rules become first-class workflow controls instead of ad-hoc conventions.
- **Bi-directional orchestration capability**: CCCC schedules agents, and agents can schedule/manage CCCC workflows via MCP tools under explicit boundaries.
- **Append-only ledger truth model**: every group event is persisted in `groups/<group_id>/ledger.jsonl` for replayable, auditable operations.
- **Structured automation engine**: interval/recurring/one-time triggers with typed actions (`notify`, `group_state`, `actor_control`) for operational delegation.
- **Accountable messaging semantics**: read cursors, acknowledgement paths, and reply-required obligations for high-signal collaboration.

### Changed
- **Generation shift from v0.3**: replaced the tmux-first operating model with a daemon-first collaboration kernel and versioned contracts.
- **Control-plane unification**: Web/CLI/MCP/IM now operate on one shared state model (thin ports, daemon-owned truth).
- **Runtime state standardization**: operational state is managed under `CCCC_HOME` (default `~/.cccc/`) instead of repository-local state.
- **Operating workflow modernization**: day-to-day usage aligns around `attach / actor / group / send / mcp` over tmux-era command patterns.

### Fixed
- Reliability hardening across RC cycles for delivery flow, automation execution, reconnect/resume handling, and registry normalization.
- Stability and UX fixes in Web interactions (including mobile operation and composer/tasking flows).
- MCP/docs/CLI parity drift reduced through dedicated guardrail tests.

### Removed
- Deprecated tmux-first orchestration line from active mainline development (archived at `cccc-tmux`).

## [0.4.0rc21] — 2025-07-24

### Added
- **Web i18n framework**: integrated `react-i18next` with namespace-based locale loading (`common`, `layout`, `chat`, `modals`, `actors`, `settings`) and automatic browser language detection.
- **Chinese (zh) locale**: complete Simplified Chinese translation across all 6 namespaces (735 keys), with native-level phrasing review and unified typography (full-width `：`, Unicode `…`).
- **Japanese (ja) locale**: complete Japanese translation across all 6 namespaces (735 keys), with native-level phrasing review and unified typography (full-width `：`, Unicode `…`, full-width `？`).
- **Language switcher UI**: minimal trigger button showing only short label (`EN`/`中`/`日`), positioned at the rightmost of the header; dropdown panel with scale-in animation and left accent bar for active item; React Portal for proper positioning.
- **i18n key parity test**: automated test to verify all locale files have identical key sets across languages.

### Changed
- `LanguageSwitcher` refactored from cycle-button to professional popover dropdown.
- Language switcher moved to header rightmost position (after Settings button) with separator.
- Shared language configuration extracted to `languages.ts`.
- README overhaul with comprehensive project details, architecture, features, and quick start guide.
- Installation instructions updated to use TestPyPI for release candidates.
- Docker Claude config updated with bypass permissions flag.

### Fixed
- Chinese locale encoding unified from `\uXXXX` escape sequences to direct Unicode characters.
- Chinese translation quality: `忙碌中`→`处理中`, `义务状态`→`回复状态`, `编辑器`→`输入框`, `代码片段`→`模板`.
- Japanese colon typography: 39 instances of half-width `:` after CJK characters corrected to full-width `：`.
- `to` label in chat kept as English "To" in ZH/JA (international convention).
- Misleading "Clipboard" label corrected to "Context" in EN/ZH/JA layout.

## [0.4.0rc20] — 2026-02-13

### Added
- **Daemon modularization**: extracted monolithic `server.py` into 22+ focused ops modules with full dispatch orchestration (`request_dispatch_ops.py`), preserving identical logic with callback injection for all external dependencies.
- **MCP parity guardrails**: toolspec dispatch parity test, schema guard test, CLI reference parity test, and web automation docs parity test.
- **MCP toolspec normalization**: consistent indentation and formatting across all 1400+ lines of tool definitions.
- **Web UI component extraction**: `ModalFrame`, `SettingsNavigation`, `ContextSectionJumpBar`, `ProjectSavedNotifyModal`, `ScopeTooltip` extracted from monolithic modal files.
- **Docker deployment guide** with custom API endpoint configuration and proxy handling.
- **Access-token authentication gate** and non-root Docker user support.
- **Auto-wake disabled recipients**: agents are automatically started when they receive a message.
- **Message mode selector** in the Web UI composer; reply-required workflow with digest nudges.
- **DingTalk enhancements**: message deduplication, file sending via new API, stream mode documentation.
- **IM bridge improvements**: proxy environment variable passthrough, implicit send behavior clarification.
- **`cccc_group_set_state`** MCP tool now accepts `stopped` (mapped to `group_stop`).

### Changed
- Daemon ops modules use dependency injection (callbacks) instead of global imports for testability.
- Serve loop extracted to `serve_ops.py`; socket protocol to `socket_protocol_ops.py`.
- MCP dispatcher split by namespace; tool schemas extracted from handler code.
- Web Settings: `AutomationTab` split into focused subcomponents.
- Runtime behavior tests made runner-independent for cross-platform CI.
- Registry auto-cleans orphaned entries on group load failures.
- Release and standards docs aligned to version-agnostic examples.

### Fixed
- Orphaned PTY actor processes cleaned up on daemon restart.
- Mobile modal UX regressions (composer, runtime selectors).
- Template import now correctly handles `auto_mark_on_delivery`.
- Tooltip ref callback stability in Web UI.
- `reply_required` correctly coerced to boolean in MCP message send.

## [0.4.0rc18]

### Notes
- Release candidate baseline before the rc19/rc20 quality-convergence cycle.
- Established append-only ledger, N-actor model, MCP tool surface, Web UI console, and IM bridge architecture.
