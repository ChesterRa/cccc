# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/), and versions follow SemVer/PEP 440.

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
