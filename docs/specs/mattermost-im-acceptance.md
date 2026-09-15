# Mattermost Acceptance Record

This record accompanies the [specification](mattermost-im.md) and [feature map](mattermost-im-features.md). Evidence is scoped to the tested version and environment. Local tests, controlled protocol fixtures, real-server checks and user acceptance are distinct; none alone proves a release ready on every platform.

The connector arrived through [PR #103](https://github.com/ChesterRa/cccc/pull/103). Its [original acceptance record at ea00093b](https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/docs/specs/mattermost-im-acceptance.md) preserves the complete development chronology, initial failures, reruns and contributor-reported Linux/Windows checks. Private server details, credentials and raw session evidence do not belong in this repository.

## September 15 integration findings

Two independent probes of the merged code found issues not caught by the earlier green checks:

- **Status hidden by a dirty draft:** editing the URL or token during a management request invalidated the whole subsequent status/configuration refresh. The running connector could be presented as stopped, with no Stop button. The existing delayed-operation test matrix was strengthened to assert authoritative status as well as draft preservation: six cases failed before the correction.
- **Socket fixture accepted too early:** the daemon HTTP fixture made its listener nonblocking before accepting a just-connected client. Twenty isolated runs reproduced `WouldBlock`. The fixture now accepts while blocking and makes only the accepted stream nonblocking, retaining coverage of the production reader's mode normalization and fragmented HTTP handling. Twenty repeat checks then passed.

The UI fix separates current status ownership from draft hydration. It adds no polling or persistent state. The socket correction changes the fixture, not the production transport. An isolated browser verified start, stop, remove and save while editing, including mobile layout; no real Mattermost server or model was called.

A subsequent language audit found 306 source lines containing Chinese outside locale files. Comments, diagnostics and ordinary test descriptions are standardized to English, along with the new public documents. Fourteen deliberate multilingual body/filename fixture lines remain to exercise Unicode transport. Existing locale files are unchanged.

Verification of the complete source correction passed on Linux: Web static/type checks, all 1,632 Web tests and production build; Rust formatting, strict workspace/all-target Clippy and full workspace/all-target tests. The tooling suite's 120 tests and Ruff passed before the language-only source cleanup; no tooling implementation changed afterward. Isolated browser checks cover actual buttons and draft preservation, not live Bot delivery. Native Windows/macOS and real-server acceptance were not rerun.

The native 0.4.40 Linux package also built successfully. The archive's executable matches the release binary, and the prepared embedded Web assets match the current Web build. This is build evidence, not deployment or live-service acceptance.

The documentation build initially detected source-relative links that resolve in the repository but fail on the published site. Those now use pinned GitHub URLs, and adjacent Markdown references are separated correctly. All 29 pinned source targets and the new cross-document anchors were checked. The failed build remains part of the integration evidence; dead-link checks were not disabled.

## Follow-up attachment integrity correction

A subsequent review reproduced complete HTTP responses whose body length disagreed with file metadata. Staging now compares the actual byte count with a known metadata size before returning the upload. A mismatch drops the temporary upload and prevents message submission. Unknown metadata sizes remain supported.

The existing multi-file failure regression now covers metadata larger and smaller than the downloaded body, including cleanup and absence of ledger submission. All 62 offline Mattermost tests and strict Web-crate Clippy passed; three live tests remained ignored. The accompanying workspace and Presentation corrections passed all 1,638 Web tests, static checks and isolated Chrome checks. The Linux package was rebuilt and its binary and prepared assets verified. These focused checks do not replace the earlier full-workspace evidence or establish live-server or native Windows/macOS acceptance.

## Acceptance matrix

Identifiers are retained from the original feature comparison. The historical evidence column summarizes the contributor's earlier records; it does not claim that those live scenarios were rerun during integration.

| ID | Required behavior | Historical evidence and limits |
|---|---|---|
| T01 | Native source organization, helpers, visibility, errors, tests and dependencies | Source review against Slack, Telegram and Discord; necessary shared changes documented |
| T02 | Web/CLI configuration, references, drafts, validation, status, themes, narrow layout and locales | Automated and real-browser checks; user confirmation separately in T20 |
| T03 | Real startup validation; explicit auth/TLS/proxy errors; redaction; recovery | Real HTTPS/WSS and invalid-token checks; controlled WS auth, proxy/NO_PROXY and TLS failures |
| T04 | Start/stop/status/config/unset/logs and enabled restore; no Actor stop or old-ledger replay | Live configuration/lifecycle checks; nonempty logs and follow-mode checks |
| T05 | Pairing, approval, rejection, expiry, revocation and unsubscribe; no unauthorized file download | Real Web/CLI pairing; 600-second expiry and aliases; fixture checks for unauthorized files and exact threads |
| T06 | Public/private channels, DMs, permitted group DMs and threads; correct routing and authorization | Five real target types reported; channels/threads authorized separately; shared Group context stated |
| T07 | Default/specific Actors and aliases; no extra recipients from body mentions | Live addressing and unknown-target checks; controlled unrelated/Bot/duplicate filtering |
| T08 | Commands and aliases through a real Mattermost client | @Bot-prefixed subscribe, unsubscribe, send, pause/resume, help/status and all verbose forms; bare slash interception documented |
| T09 | Independent per-target subscriptions and shared Group output | Real multi-target pause/resume, verbose and status checks; no claim of private DM sessions |
| T10 | Public visibility, sender fallback, Markdown, links and code; no loops | Shared privacy/filter assertions and real formatting checks; private terminal content excluded |
| T11 | Per-target progressive output; complete-final dedupe; fallback after failures | Real main-timeline/thread editing; controlled failed creation/editing, long previews and fallback |
| T12 | Unicode-safe long text and code, fully reconstructible | Real multilingual/emoji chunks and a 22,158-character code/link body; controlled fallback cases |
| T13 | Two-way text/files/images/PDF/audio/video and attachment-only messages | Real transport of synthetic TXT/PNG/PDF/WAV/WebM files with hash checks; not content-understanding evidence |
| T14 | Size, ownership, length, path, redirect and upload failures | Real oversize rejection; controlled unknown length, ownership, redirects, paths, cross-Group Blobs and HTTP 403 |
| T15 | Stable source IDs, dedupe, Bot/system/edit filtering; no wait for model completion | Source correlation and shared client-ID assertions; controlled repeated/edited/Bot posts |
| T16 | Own-Bot processing reactions, correlated completion/failure and cleanup | Real reactions; controlled event association, expiry and HTTP 403 without losing message text |
| T17 | Ledger lag, disconnects, rate limits, auth failures and restart boundaries | Shared lag recovery and real restart boundaries; controlled WS/429/401/403 and lost POST responses without duplicate creation |
| T18 | Multi-target Group behavior and separate Bots for separate Groups | Real multiple targets; two simulated Bots for Group isolation, not a second real-Bot deployment |
| T19 | Existing Rust/Web/build/package paths, with evidence for each mapped feature | Version-specific workspace, frontend and package records; no separate connector package |
| T20 | User configures through Web and tests addressing, files, threads, streams and commands | Contributor recorded explicit user acceptance on September 8, 2026; not fresh acceptance of this integration |

## Historical findings and later corrections

These summaries preserve the useful evidence without treating every review round as a separate current specification. Exact versions, commands and original failure logs are indexed in the [archived record](https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/docs/specs/mattermost-im-acceptance.md).

- **File references:** an Actor returned `refs` instead of using file delivery. No file reached Mattermost in that attempt. It remains an Actor tool-use failure; the connector was not changed to convert references into attachments.
- **LOG01 → LOG03/LOG05:** an oversize file produced status and user feedback but no readable Group log. The combined CLI path lacked a tracing subscriber. Native Group logging plus stderr corrected that gap; a new oversize upload verified the real log path, and follow mode later showed new records once. Rotation, redaction and write-failure checks remain controlled tests, not real disk-failure evidence.
- **FILE02 / NET02:** controlled 403 metadata/body failures left no Blob. A server that received a complete POST and lost its response did not receive an automatic duplicate create.
- **V01:** a real human-origin request missed during a short disconnect was recovered and produced an Actor reply. Revoking access before recovery prevented a later replayed request from reaching the Actor. These were finite Linux live scenarios within the server cache window, not Windows evidence or guarantees after restart/cache expiry.
- **Reviews 1–6:** address validation, bare-mention filtering, draft preservation, identity serialization, runtime error ownership, native connection recovery, unknown-submit feedback, lookup authorization and heartbeat write deadlines were corrected. Platform fixtures and live protocol checks are recorded separately.
- **Review 7:** staged multi-file download/validation and cancellation cleanup were checked without deleting shared Blobs. Saving files and submitting a ledger event still do not form one atomic transaction.
- **Reviews 8–10:** cross-platform startup result ownership, repeated lookup/control/failure feedback, delayed stop/config replacement, upload metadata and safe recovery cursor retention received regressions.
- **Reviews 11–13:** pre-allocation startup/restore windows, equal-config request revisions, long final streams and platform-visit ownership were checked at actual entry points.
- **Reviews 14–15:** initial hydration, edits during management, daemon IPC delegation and browser request ordering were checked. These earlier draft assertions did not catch the status-refresh omission found during integration.
- **Review 16:** failure after worker installation now removes and stops only that generation. Stop/remove errors remain visible, and Weixin-specific management flows participate in Mattermost ordering.
- **Review 17:** the existing Group sweep now removes revisions for deleted Groups that never had workers. Legacy-only management continuations were explicitly deferred; see the [scope boundary](mattermost-im.md#deliberate-scope-boundary).

Historical full checks also recorded intermittent terminal WebSocket shutdown and voice test-server startup failures. Unchanged focused and full reruns passed, but those records did not establish a root cause. They must not be rewritten as an initially clean run or as proven platform fixes.

## Regression entry points

Use isolated `CCCC_HOME` directories, synthetic credentials and controlled platform fixtures. Do not use a developer's running services for these checks.

| Area | Entry points and assertions |
|---|---|
| Core state and CLI | `im_state` normalization, URL validation, credential aliases and CLI argument translation |
| Daemon management | `ops::im::tests`: real HTTP delegation, fragmented reads, unavailable/rejected Web and stale ownership |
| Runtime lifecycle | `im_runtime`, `routes::im`: generation/revision handoff, failed final commit, restore and deleted-Group cleanup |
| Inbound | `mattermost`, `mattermost_inbound`, `inbound_attachments`: exact authorization, identity failures, control replay, staged files and safe submission outcomes |
| Outbound | Mattermost protocol fixtures: actual post edits, chunk reconstruction, attachment metadata, per-target dedupe and reaction failures |
| Network | Controlled REST/WS: TLS/proxy parity, sequence replay, missing hello, gaps, cache loss, auth rejection, heartbeat write timeout and queue backpressure |
| Web | `SettingsModal.mattermost.test.tsx`, `IMBridgeTab.revoke.test.tsx`, IM API/config tests: status and drafts, failures, Group/platform visits, unmounts and request ordering |
| Real browser | Delay actual request boundaries; check status/buttons, preserved input, narrow layout, keyboard/focus and visible errors |
| Release preparation | Web check/test/build, Rust fmt/Clippy/workspace tests, tooling and package build |

Three live Mattermost tests remain explicitly ignored by default. They require an authorized test site/channel and credential file, leave synthetic posts, and are not part of ordinary offline regression. Enabling them is not implied by running the repository gate.

## Completion criteria

A passing mock proves the exercised behavior, not the entire live integration. Real API acceptance requires a dedicated Bot, exact approved targets and separate authorization to send messages. Claims about native Windows/macOS behavior require those platforms.

Before publishing, tie CI and acceptance to the actual candidate commit. Do not carry earlier test counts forward as current evidence, report a build as deployed, or label an excluded legacy issue fixed.
