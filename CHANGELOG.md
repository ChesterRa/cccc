# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/), and versions follow SemVer/PEP 440.

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
