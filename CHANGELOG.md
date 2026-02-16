# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/), and versions follow SemVer/PEP 440.

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
- **Token-based authentication gate** (`CCCC_WEB_TOKEN`) and non-root Docker user support.
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
