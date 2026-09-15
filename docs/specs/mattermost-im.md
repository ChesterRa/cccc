# Native Mattermost IM Connector Specification

This specification describes the Mattermost connector contributed through [PR #103](https://github.com/ChesterRa/cccc/pull/103), including the September 15, 2026 integration corrections. Implementation and test evidence are separate: see the [feature map](mattermost-im-features.md), [acceptance record](mattermost-im-acceptance.md), [user guide](../guide/im-bridge/mattermost.md) and [architecture decision](../adr/0001-native-mattermost-im.md).

The original comparison baseline was `2a38ad78700a4a188b45f515434b74cc315b91ec`. The [pre-integration specification at ea00093b](https://github.com/ChesterRa/cccc/blob/ea00093b07a21d947982a252af42f2755bc9ac0f/docs/specs/mattermost-im.md) preserves the detailed development and review history. Historical passing results do not establish verification of later changes.

## Scope and implementation

Mattermost is a native IM platform, built and released with CCCC. It reuses Group configuration, chat authorization, subscriptions, daemon dispatch, ledger output, Blob storage and worker lifecycle. No separate service, crate, SDK, plugin system, installer or dependency is introduced.

The implementation follows the existing Slack/Telegram layout:

- `crates/cccc-web/src/im_runtime/mattermost.rs`: REST/WS transport, authentication, lifecycle, recovery and protocol fixtures.
- `mattermost_inbound.rs`: source filtering, commands, authorization, file staging and daemon submission.
- `mattermost_outbound.rs`: target posts, progressive edits, final fallback and uploads.
- Shared registration changes remain in IM state, runtime dispatch, Web/daemon routes and CLI argument translation.
- Web uses the existing SettingsModal, IMBridgeTab, SelectCombobox, controlled forms, design tokens and locale namespaces.

Existing helpers remain authoritative for credentials, chat targets, command decisions, recipient semantics, public-output filtering, text formatting, chunks, files and processing reactions. Internal helpers retain local visibility and the existing error types. Network input waits for daemon submission, not for the Actor to finish.

Source comments, diagnostics, ordinary test descriptions and public documentation use English. Localized Web copy belongs in the existing English, Chinese and Japanese locale files. Multilingual fixture data remains where it verifies Unicode transport or filenames.

## Configuration and identity

Configuration adds `platform=mattermost`, `mattermost_url` and the existing `bot_token` / `bot_token_env` fields. CLI uses `--mattermost-url`. Preserve existing aliases and file-policy normalization.

The URL is a site root, optionally with an installation subpath. Reject embedded credentials, query strings, fragments and API endpoint URLs. Backend normalization is authoritative; frontend validation provides immediate feedback. Production uses HTTPS; HTTP is limited to trusted local or isolated testing. Authentication does not follow redirects or disable certificate validation. REST and WebSocket share reqwest's TLS and proxy policy.

Startup resolves credentials once, verifies a Bot account and validates the initial WebSocket handshake before reporting Running. Failed identity checks do not fall back to personal accounts or broader permissions.

Use one dedicated Bot identity per Group. Preventing reuse across Groups or running instances is a deployment responsibility; the connector has no global Bot registry or lock.

Changing the site or Bot identity clears pending requests, approvals and subscriptions. Rotating a token for the same Bot preserves them. Serialize identity comparison and update using the existing Group file lock. Persist authorization removal before recording the new identity, outside the IM update callback; a failed intermediate write must not leave new identity metadata paired with old grants. Stale startups cannot change a newer configuration's identity or authorization.

## Lifecycle and management ownership

Web owns the network workers and their runtime state. Operations involving Mattermost use the existing lifecycle and configuration locks:

1. Read configuration and its revision together. Under the same configuration lock, compare both before allocating a startup generation. Equal configuration values alone do not identify the same request.
2. Saving, stopping, removing or switching platforms invalidates prior startup requests. Old manual and restore requests cannot allocate a worker after that invalidation.
3. Installing, removing and closing workers respects the generation under the existing lifecycle lock. A delayed stop or failed startup must not close a newer worker.
4. If the final state commit fails after worker installation, remove and close only that installed generation. Return the original error; this does not roll back messages already sent.
5. Runtime error writes and clears check both generation and configuration under the config lock. Stale workers may write redacted diagnostics but cannot overwrite current status.
6. The existing `stop_missing(active_groups)` sweep also retires configuration revisions for deleted Groups, even if they never had a worker. Keep active revisions and existing stop counts. A failed Group listing is not an empty listing.

Daemon protocol `im_set` / `im_unset` operations involving Mattermost, including switches into or out of it, delegate to Web's existing management endpoint. Check platform ownership under the configuration lock before allowing an older direct-write path. Delegation failure must not fall back to editing state locally. Mattermost start/stop delegation failures likewise must not overwrite Web-owned state. Ordinary CLI management already uses Web; it is not the same entry point as daemon IPC.

Two-platform paths that never involve Mattermost retain their existing configuration, storage-read failure and concurrency behavior. The connector does not refactor every IM lifecycle into a new framework.

### Web status and drafts

Group, modal visit and platform identify the owner of a Mattermost asynchronous action. Closing, switching Groups or leaving and revisiting a platform invalidates old continuations, including their errors, follow-up requests and busy-state cleanup.

Status refresh and configuration hydration have separate guards. A current operation's authoritative status must update even if the user edited the URL or token while it was pending. Only stale configuration hydration and direct draft clearing are suppressed by the edit sequence. Check edits again after fetching configuration. This supersedes the earlier behavior that discarded the whole refresh after an edit.

Save failure preserves the Mattermost draft and shows an error; Start proceeds only after its save succeeds. Stop/remove failures also remain visible and release busy state. Switching platforms in one Group preserves the Mattermost draft; switching Groups clears it.

Management flows involving Mattermost are ordered per Group within the browser, across component unmounts. Save-before-start is one flow. Weixin login, verification, logout and post-login startup use the same queue where they intersect Mattermost. Queued actions whose visit expired do not send new requests. Failures release the queue; other Groups remain independent. This is not a cross-client transaction or proof that an ambiguous network request was cancelled.

### Deliberate scope boundary

The [review17 comment](https://github.com/ChesterRa/cccc/pull/103#discussion_r4006724800) identified a possible pre-existing ownership gap in management continuations that never pass through Mattermost: a delayed operation in Group A can refresh A's fields after navigation to B or modal closure. The contributor's scope decision explicitly deferred that legacy-only issue. Static inspection of the old SettingsModal supports the concern; it does not prove a write to the wrong Group.

This integration does not claim to fix that legacy-only path. Mattermost ownership, draft, status and ordering protections remain required. Deleted-Group revision cleanup is implemented and is not covered by the deferral.

## Authorization and message semantics

Supported targets are public/private channels, direct messages, group direct messages where the server permits Bot membership, and threads. Missing membership is an error, not a reason to elevate Bot privileges.

Authorization applies to an exact chat target in a Group, not individual participant RBAC. Channel targets have an empty thread ID and publish to the main timeline; thread targets preserve `root_id`. Channel approval does not automatically approve its threads. Multiple approved targets share Group context and eligible subscription output; DMs are not private isolated sessions.

The existing `/subscribe` / `/sub`, pairing key, approval/rejection, revocation, unsubscribe, pause/resume, verbose, help and status semantics apply. Pairing keys expire after 10 minutes. These commands do not add model calls or a new permission system. This specification does not authorize changing any live deployment's access scope.

Mattermost intercepts leading slash commands. Document `@<Bot username> /command` as the reliable text-command form; use the actual username, not the display title. No custom slash-command registration or public callback is required.

Authorized DMs can send ordinary text. Channel questions and files require addressing the Bot; recognized CCCC commands that arrive as ordinary posts also count as explicit requests. Ignore a bare mention with no text or files. Strip only the leading Bot mention. Default foreman, explicit Actor IDs, `@all` and `@peers` retain daemon semantics; mentions in the body do not add recipients.

Use real `user_id`, `channel_id`, `root_id`, `post_id` and `file_ids`, preserving native source metadata and stable client IDs. Ignore self/Bot/system posts, edits and unrelated chatter. Authorization and pause checks precede file downloads or model submission.

## Inbound failures and files

The bounded source-post cache records successful dispatch, applied control decisions and attempted failure feedback. Replaying a source post must not repeat authorization changes, file downloads or failure notices. A new post ID can retry. This is an in-memory limit, not persistent exactly-once delivery.

Shared daemon errors may contain user input. Convert them to safe categories at the submission boundary. An explicit recipient rejection gives a corrective message; unknown outcomes instruct the user to check CCCC before resending. Do not log raw daemon text or automatically resubmit ambiguous requests. Remove the processing reaction without marking an unknown outcome as a definite failure.

Channel/sender lookup failure feedback is limited to the exact currently authorized, subscribed, unpaused and correctly addressed target. Do not infer an unknown channel type as a DM or notify unauthorized/self/known-Bot sources. Record the feedback attempt even if sending fails.

Files use native authenticated endpoints and Group Blob storage. Verify file-to-post ownership, metadata, filename, size and download length; never trust arbitrary remote URLs. When metadata supplies a size, the staged byte count must match it, even if the HTTP response has a valid Content-Length. Stage all attachments before saving any. On download/validation failure or cancellation, clean up temporary files without deleting shared final Blobs. A file-save or ledger-submit failure is not a cross-file atomic rollback.

Enforce the shared 10 MiB file limit and a lower Group `files.max_mb` before Blob writes; `files.enabled=false` disables forwarding. Images, files, PDFs, audio/video and attachment-only input are transport capabilities, not OCR/transcription promises. Incoming MIME comes from Mattermost. Outgoing upload preserves names and bytes; Mattermost classifies the resulting MIME.

Only `attachments` are uploaded. Plain text and `refs` are not converted into files. Agent file delivery uses `cccc_file(action="send", ...)` and must respect its working-directory scope.

## Outbound, streaming and reactions

Reuse the common ledger consumer, subscription filters and sender-title fallback. Only eligible public output is forwarded; do not expose private notifications, Actor-directed system events or terminal internals.

Create and patch Mattermost posts for `chat.stream` start/update/end, with per-target handles and throttled updates. No stream event means final-message delivery, not terminal-state inference.

Split long text safely by Unicode characters. Reuse the first stream post for the first final chunk, then send remaining chunks. Suppress final text only after the complete matching terminal body has succeeded for that target. If creation, editing or any final chunk fails, retain the full final fallback. Partial success can duplicate already displayed portions; it is not an atomic transaction. Preserve attachments even when matching text is suppressed.

Use native file upload plus `file_ids`, keeping channel/thread routing. Reaction failures must not suppress message text. Correlate processing reactions with the accepted event ID before processing a completion; modify only this Bot's reactions. Expiry cleanup is not Actor cancellation.

## Recovery and observability

REST retries are limited to explicit rate-limit rejections. Do not repeat post creation after an unknown transport outcome. Authentication rejection stops the worker; transient network/server errors reconnect.

WebSocket recovery uses the native `connection_id` and next event sequence. All events advance the cursor, not only `posted`. Ignore duplicates; a gap triggers recovery. Initial connections wait for `hello`, but same-ID recovery can immediately replay events or remain idle without a new hello. Do not discard the first replayed event. A new connection ID reports possible cache loss through logs and status; ordinary reconnect success does not erase that gap warning.

Reception and inbound work use separate tasks and a queue of 128 events. Local backpressure pauses reads but is not remote inactivity. Runtime Ping/Pong writes have a 5-second deadline and reconnect uses a fixed 5-second delay. Initial handshake timeout is 15 seconds. Sustained overload can still exceed server recovery capacity.

Ledger lag catch-up is outbound; server-cache recovery is inbound. Neither promises history replay after a process restart or deliberate stop. No REST history polling, persistent outbox, extra database or exactly-once contract is added.

Errors go to the existing Group `state/im_bridge.log` and stderr independently of tracing initialization. JSON lines include time, Group, operation and redacted error, excluding bodies, file contents and raw HTTP responses. Use the existing file lock, Unix mode 0600 for new files, a 4,096-character error cap and one rotated file at 1 MiB. Logging failures remain visible on stderr without breaking messaging. Developer mode controls log reading, not recording. Status and durable diagnostics have separate lifetimes.

## Excluded business features

Meeting orchestration, Topic lifecycles, automatic Group preparation, hardcoded owner lists, one-Bot cross-Group routing, a mandatory single chat per Group, default verbose mode, full channel inventories, persistent delivery queues, a new control console and Actor TUI cards are not native connector prerequisites.

Deployment rollback procedures and a dedicated operational alert channel remain separate deployment concerns. Their earlier mention is not evidence that the runtime implements them.

## Verification boundary

Verify shared contracts at the affected CLI, daemon, Web and runtime entry points. Fault tests must assert actual state, delivery or worker cleanup, not only request strings. Use controlled REST/WS and isolated homes for automated tests. Real-server, native-platform and user acceptance evidence must be identified separately; see the [acceptance record](mattermost-im-acceptance.md).
