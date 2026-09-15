# Mattermost Feature Map

This map compares Mattermost with the user-facing capabilities of CCCC's native IM connectors, originally inspected at `2a38ad78700a4a188b45f515434b74cc315b91ec` on September 7, 2026. It does not claim support for every platform SDK API or every Web/TUI capability.

The [specification](mattermost-im.md) defines current behavior. The [acceptance record](mattermost-im-acceptance.md) separates historical live checks from current integration tests. F01–F34 and T01–T20 retain their original identifiers for traceability; the [original review history](https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/docs/specs/mattermost-im-features.md) remains available.

All directly supported or equivalent capabilities below are included. A platform difference must be explicit; it is not grounds for silently omitting the underlying capability. Guide text alone is not implementation evidence.

## Functional mapping

| ID | Capability and implementation references | Mattermost mapping and limits | Acceptance |
|---|---|---|---|
| F01 | Configuration, validation, startup and status; [C1], [C2], [C3], [C4] | Site URL and Bot Token; REST identity plus WS handshake; inline URL and management errors | T02, T03 |
| F02 | Group lifecycle and enabled restore; [C1], [C5] | Group workers, generation/revision guards, stale-result rejection and installed-worker cleanup; stop does not stop Actors | T04; lifecycle regressions |
| F03 | Channels, DMs and threads; [C6], [C7], [C8] | `channel_id` plus `root_id`; private channels and group DMs require actual Bot membership | T06 |
| F04 | Explicit channel addressing and implicit DM input; [C6], [C8], [C9] | Actual Bot username; prefix commands with @Bot to avoid native slash interception | T07, T08 |
| F05 | Default and explicit recipients; [C10] | Reuse daemon addressing; mentions inside the body do not add recipients | T07 |
| F06 | @all and @peers; [C10] | CCCC aliases, without requiring corresponding Mattermost accounts | T07 |
| F07 | Subscribe aliases and 10-minute pairing keys; [C9] | Request access to the exact chat/thread and show the target Group | T05, T08 |
| F08 | Pending approvals, bind, reject and revoke; [C2], [C3], [C4] | Existing Web/CLI authorization workflow and storage | T05 |
| F09 | Unsubscribe aliases; [C9] | Remove the target subscription without deleting the Group | T05, T08 |
| F10 | Per-target pause/resume; [C9], [C10] | Control that chat/thread, without cancelling the model or replaying a backlog | T08, T09 |
| F11 | Help and status; [C9] | Existing Group/Actor counts and subscription status, without model calls | T08, T09 |
| F12 | Verbose off by default; [C9], [C10] | Per-target flag; no argument enables it; on/off, true/false and 1/0 supported | T08–T10 |
| F13 | Public notification filtering; [C1] | Only eligible public, non-Actor-directed system output; no private terminal content | T10 |
| F14 | Sender titles; [C11] | Markdown sender label, falling back to Actor ID; no Bot per Actor | T10 |
| F15 | Progressive streams and throttling; [C12], [C13], [C14] | Create and patch per-target posts; no inference from TUI activity | T11 |
| F16 | Final fallback and Unicode chunks; [C12], [C13], [C15] | Suppress matching final text only after every terminal chunk succeeds; partial failure retains full fallback | T11, T12 |
| F17 | Inbound files and Blob metadata; [C16], [C17] | Authenticated download, ownership/size checks, stage all files before saving; clean temporary files on failure | T13, T14; staging regressions |
| F18 | Outbound files; [C12], [C13], [C18] | Prepare files from this Group, upload, then post file IDs with channel/thread routing | T13, T14 |
| F19 | Source and reply correlation; [C10], [C19] | Real user/post/thread IDs, stable client ID and corresponding CCCC event; no nickname identity | T06, T15 |
| F20 | Submission and event deduplication; [C10], [C20] | Stable daemon request identity plus bounded source-post cache; control decisions and attempted failure notices are remembered | T15; replay regressions |
| F21 | Loop prevention; [C1], [C6], [C8] | Filter human/IM inbound from outbound, and self/Bot/system/unrelated input | T10, T15 |
| F22 | Processing reactions; [C19], [C21] | Modify only this Bot's reactions; correlate completion; cleanup is not cancellation | T16 |
| F23 | Runtime ledger lag recovery; [C1] | Reuse shared outbound catch-up without another SSE/SDK route | T17 |
| F24 | Connection failures and recovery; [C6], [C8] | Native connection ID/sequence replay, cache-loss warning, heartbeat deadlines, terminal auth rejection and bounded retries | T03, T17 |
| F25 | Logs and latest error; [C1], [C3], [C4] | Group log plus stderr, redaction and existing status/CLI access | T03, T14, T17 |
| F26 | Persistent IM state and shared locks; [C22] | Existing config, authorized, pending and subscribers shape; no separate database | T04, T05 |
| F27 | Credential aliases, environment references and file policy; [C2], [C22] | Existing normalization plus site URL; draft/status guards and management ordering, including Weixin transitions | T02, T14; management regressions |
| F28 | Multiple targets per Group; [C10], [C12] | Separate authorization/pause/verbose/stream handles; shared Group context and eligible output | T09, T11, T18 |
| F29 | Image/audio/video/file transport; [C17], [C23], [C24] | Native file IDs; preserve inbound MIME and outgoing names/bytes; server classifies outgoing MIME; no transcription | T13 |
| F30 | Attachment-only and mixed input; [C10], [C12], [C17] | Authorized DMs support files alone; channels still require explicit addressing | T13 |
| F31 | Markdown, links, code and authors; [C11], [C12], [C13] | Mattermost text formatting; no extra card protocol to imitate another platform | T10, T12 |
| F32 | Existing CLI operations; [C4], [C22], [C25] | Config/set/unset/start/stop/status/logs/pending/authorized/bind/reject/revoke; no deprecated backlog or context commands | T04, T17 |
| F33 | Existing Web settings; [C2], [C26] | Platform form, drafts, validation, status, approvals, themes and locale strings | T02 |
| F34 | HTTP/WS network configuration; [C6], [C27] | Reuse existing client's TLS/proxy policy for both transports; no dedicated proxy service | T03 |

## Platform-specific equivalents

| ID | Source-platform mechanism | Mattermost equivalent |
|---|---|---|
| N01 | Weixin QR login, verification and account cleanup; [C28] | Bot Token plus explicit chat pairing; no personal-account QR login (F01/F07/F08/F27) |
| N02 | DingTalk AI cards and WeCom callback-bound streams; [C14], [C29] | Progressive post edits without card templates or callback windows (F15/F16) |
| N03 | WeCom encrypted media and WS chunk uploads; [C24], [C29] | Authenticated native file REST endpoints, without another encryption protocol (F17/F18/F29) |

## Boundaries to verify

- Dedicated Bot identity per Group is a deployment requirement, not enforced cross-Group/instance uniqueness.
- Channel and thread authorization remain distinct. DMs share Group context with its other approved subscriptions.
- Server permissions determine private-channel and group-DM support. Do not elevate the Bot to work around restrictions.
- Real-client command checks must use the @Bot prefix; a slash command swallowed by the client is not successful CCCC delivery.
- Outgoing file MIME is classified by Mattermost. A `refs` entry is not an uploaded attachment.
- Full read receipts, Actor TUI state, meeting control, custom slash-command registration and interactive control consoles are outside the native IM contract.
- Inbound server-cache replay and outbound ledger catch-up do not promise cross-restart replay, a durable outbox or exactly-once delivery.
- Media transport does not implement OCR, document parsing, webpage extraction or speech transcription.
- The legacy-only management-continuation concern remains explicitly deferred in the [specification](mattermost-im.md#deliberate-scope-boundary); Mattermost test results do not certify it as fixed.

## Implementation references

[C1]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime.rs
[C2]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/routes/im.rs
[C3]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-daemon/src/ops/im.rs
[C4]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-cli/src/args/integrations.rs
[C5]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/worker.rs
[C6]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/slack.rs
[C7]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/feishu.rs
[C8]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/discord.rs
[C9]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/commands.rs
[C10]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/state.rs
[C11]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/outbound_message.rs
[C12]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/slack_outbound.rs
[C13]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/discord_outbound.rs
[C14]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/dingtalk_streaming.rs
[C15]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/outbound_chunks.rs
[C16]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/inbound_attachments.rs
[C17]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/telegram_inbound.rs
[C18]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/outbound_attachment.rs
[C19]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/processing_reactions.rs
[C20]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/discord_dedup.rs
[C21]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/discord_reactions.rs
[C22]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-core/src/im_state.rs
[C23]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/weixin_inbound.rs
[C24]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/wecom_message.rs
[C25]: ../guide/im-bridge/index.md
[C26]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/web/src/components/modals/settings/imBridgeConfig.ts
[C27]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/discord_gateway_proxy.rs
[C28]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/weixin_login.rs
[C29]: https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/crates/cccc-web/src/im_runtime/wecom_client.rs

## Platform references

The original API comparison was checked on September 7, 2026. These sources explain feasibility; they do not prove acceptance on a particular deployment.

[M1]: https://docs.mattermost.com/api/reference/create-post
[M2]: https://raw.githubusercontent.com/mattermost/mattermost/v11.9.0/api/v4/source/channels.yaml
[M3]: https://developers.mattermost.com/integrate/slash-commands/
[M4]: https://docs.mattermost.com/api/reference/patch-post
[M5]: https://docs.mattermost.com/api/reference/get-file
[M6]: https://docs.mattermost.com/api/reference/upload-file
[M7]: https://raw.githubusercontent.com/mattermost/mattermost/v11.9.0/api/v4/source/reactions.yaml
[M8]: https://docs.mattermost.com/api/reference/connect-web-socket
[M9]: https://github.com/mattermost/mattermost/blob/v11.9.0/server/channels/app/platform/web_conn.go
