# Mattermost

This guide applies to CCCC builds that include Mattermost in the IM Bridge platform list. See the [acceptance record](../../specs/mattermost-im-acceptance.md) for verification evidence and limitations.

The connector links one CCCC Group to Mattermost using a Bot Token, REST and WebSocket. CCCC initiates HTTPS/WSS connections from its own host. You do not need to install CCCC on the Mattermost server or expose a public callback endpoint.

## Access boundaries

::: warning Chats in the same Group share context
Use a dedicated Bot for each Group. Do not reuse the same Bot identity across Groups or running CCCC instances, even with different tokens. CCCC does not detect or prevent that reuse. If multiple Groups authorize the same chat through that Bot, they may process messages more than once and mix replies.

A Group can authorize multiple channels, direct messages and threads. They share the Group's context and subscription output. A direct message is **not** a separate private Agent session. Approving a chat grants its participants access to the Group; it is not individual user authorization.
:::

## 1. Prepare a Bot

1. Create a dedicated Bot account on your Mattermost site and obtain its access token. Use a Bot Token, not a personal or administrator token.
2. Add the Bot to the required teams and channels. Invite it separately to private channels.
3. Allow it to read joined channels, create and edit its own posts, upload and read files, and add or remove its own reactions. System administrator access is unnecessary.
4. Verify HTTPS and WebSocket connectivity from the CCCC host. Any reverse proxy must support WebSocket upgrades.

See Mattermost's [Bot account guide](https://developers.mattermost.com/integrate/reference/bot-accounts/) and [API documentation](https://api.mattermost.com/). Bot membership in group direct messages depends on the site's version and permissions; do not bypass restrictions by making the Bot an administrator.

## 2. Configure CCCC Web

Open the target Group's **IM Bridge** settings:

1. Select **Mattermost**.
2. Enter the site URL, such as `https://mattermost.example.com`. Installation subpaths such as `https://example.com/chat` are supported. Do not append `/api/v4`, query parameters or credentials.
3. Enter the Bot Token or an environment variable name available to the running CCCC process, such as `MATTERMOST_BOT_TOKEN`. Prefer an environment variable reference to keep the token out of shared configurations and screenshots.
4. Save, then start the connector. Start also saves the current form before connecting; a failed save prevents startup. Invalid addresses disable Save and Start. Management errors appear in the form so you can correct them and retry.
5. Confirm the status is **Running**. Startup verifies the Bot identity and WebSocket `hello`, not just whether a token field is filled in.

Switching platforms within the same Group preserves the unsaved Mattermost draft. Switching Groups clears that draft; save first if you want to keep it. Saved configuration is unaffected.

You can edit the form while saving, starting, stopping or removing the connector. When the operation finishes, status and available buttons update while your newer edits remain. Save those edits separately to apply them.

An explicit authentication rejection during reconnect, such as HTTP 401/403, stops the connector with an error. Correct the token or permissions, then start it again. Temporary network, rate-limit and server failures reconnect automatically.

Use HTTPS in production. HTTP is suitable only for a trusted local or isolated test network: credentials and messages would be unencrypted. Certificate verification stays enabled, and authenticated requests do not follow redirects. Enter the final site URL directly.

## 3. Authorize a chat

Replace `cccc_bot` with the Bot's actual **username**, not its display name. In a channel or direct message, enter:

```text
@cccc_bot /subscribe
```

Review the Group and target in CCCC's **Pending Requests**, then approve the request. You can also bind with the pairing key, which expires after 10 minutes. Channels and threads are authorized separately; approving a channel does not authorize every thread.

::: tip Slash commands in Mattermost
Mattermost intercepts input beginning with `/`. Prefix CCCC commands with `@cccc_bot` in channels and direct messages. You do not need to register custom Mattermost slash commands named `/send` or `/subscribe`.
:::

## 4. Send messages and control subscriptions

| Input | Action |
|---|---|
| `@cccc_bot Hello` | Send to the default foreman |
| `@cccc_bot /send @reviewer Review this material` | Send to the specified Actor |
| `@cccc_bot /send @all Please respond` | Broadcast to all Actors |
| `@cccc_bot /send @peers Add your feedback` | Send to members other than the foreman |
| `@cccc_bot /status`, `@cccc_bot /help` | Show status or help without calling a model |
| `@cccc_bot /pause`, `@cccc_bot /resume` | Pause or resume this chat target's subscription |
| `@cccc_bot /verbose on`, `@cccc_bot /verbose off` | Toggle more detailed public exchanges; private events remain private |
| `@cccc_bot /unsubscribe` | Unsubscribe; `/unsub` is also supported |

Authorized direct messages accept ordinary text without the Bot prefix. Questions and files in channels require explicit addressing. Mentioning another Actor in the body does not add a recipient. `/sub` aliases `/subscribe`; `/verbose` without an argument enables it, and also accepts `true/false` and `1/0`.

Channel subscription output appears in the main timeline. Separately approved thread subscriptions retain their original thread. Eligible Group output follows the subscription rules, rather than going exclusively to the person who first asked a question.

## 5. Progressive output and files

- When CCCC publishes `chat.stream`, the Bot creates and updates the same post. Otherwise, it sends the final reply. Terminal activity is not automatically converted into chat output.
- Duplicate final text is omitted only when the entire streamed final body was delivered successfully and matches the final message. Long final bodies reuse the initial post for the first chunk and send the rest in order. An edit or chunk failure keeps the full final-message fallback; already delivered portions may appear twice after partial failure.
- Long messages split safely at Unicode character boundaries, with a default limit of 16,383 characters per post. Site or proxy limits may be lower.
- Images, ordinary files, PDFs, audio and video travel through the Group's Blob storage. Attachment-only messages are supported. File transport does not imply OCR, PDF parsing or speech transcription.
- Each file is limited to 10 MiB, or the Group's `files.max_mb` if lower. `files.enabled=false` disables file forwarding. File failures are reported rather than presented as successful Agent delivery.
- All files in a source post are downloaded and validated before saving. A download or validation failure prevents partial submission and cleans up staged files. Existing shared Blobs are preserved. Saving files and appending a message are not one transaction; disk errors or unknown submission outcomes cannot guarantee rollback.
- The Bot adds processing reactions and updates them on a correlated response or failure. Expired-reaction cleanup does not cancel the Agent's work.

### File references are not attachments

Filenames, local paths and `refs` in message text are not automatically uploaded. Like other native IM connectors, Mattermost sends only the message's `attachments`.

To deliver a file, an Agent should call `cccc_file(action="send", ...)` and check the result. The file must be in the current working directory's scope. For an incoming `state/blobs/...` attachment, read or resolve it through the file tools, then copy the file into the working directory before sending it back. A failed tool call must not be replaced by a plain-text claim that the file was sent.

When diagnosing delivery, inspect both CCCC `attachments` and the Mattermost post's `file_ids`. Text saying that a file was sent is not delivery evidence.

## 6. CLI and operations

```sh
cccc im set mattermost --group g_example \
  --mattermost-url https://mattermost.example.com \
  --bot-token-env MATTERMOST_BOT_TOKEN
cccc im start --group g_example
cccc im status --group g_example
cccc im pending --group g_example
cccc im bind --group g_example --key KEY_FROM_CHAT
cccc im authorized --group g_example
cccc im logs --group g_example -f
cccc im stop --group g_example
```

The existing `config`, `unset`, `reject` and `revoke` operations also apply; consult `cccc im --help` for arguments. Stopping the connector does not stop Actors. Network workers run in the CCCC Web process, without a separate connector service.

### Errors and logs

`im logs` requires developer mode in the global observability settings. Otherwise it returns `developer_mode_required`; error recording continues. Mattermost writes errors to the Group's `state/im_bridge.log` and process stderr, including in combined CLI/Web and Docker deployments.

Records contain the time, Group, operation and a redacted error, excluding chat bodies and file contents. Errors are capped at 4,096 characters. The log rotates to `im_bridge.log.1` before exceeding 1 MiB and retains one rotated file. File-write failures are reported on stderr without stopping message handling. `last_error` is the latest status, not log history; clearing it does not delete logs.

Daemon errors can include user input. The connector records fixed submission error categories and source post IDs instead of raw daemon text. An explicit recipient rejection asks you to correct the `/send` target. An unknown outcome asks you to check CCCC before resending, without automatically submitting again or adding a failure reaction. It only removes the processing reaction. The inbound worker remembers that source post; a manually resent message has a new ID and is not protected by that deduplication.

If channel or sender lookup fails, a short notice is attempted only for an exactly authorized, subscribed, unpaused target that meets the addressing rules. Unauthorized targets, this Bot and known other Bots receive no notice. An unknown channel type is not treated as a direct message. Lookup failures do not download files or invoke a model. A failed notice is logged without repeated attempts.

### Disconnects and recovery

Temporary disconnections reconnect with Mattermost's native connection ID and event sequence to recover events still in the server cache. Replayed events are checked against current authorization, pause and deduplication rules. A server restart, expired cache or different cluster node can prevent recovery; logs and status then warn that events may be missing.

Runtime ledger-consumer lag uses the shared outbound catch-up mechanism. Process restarts and deliberate stops start from a new boundary without replaying old history. There is no persistent outbox or exactly-once guarantee. Post creation is not blindly retried after an ambiguous network failure.

WebSocket reception and attachment processing run separately. The inbound queue holds up to 128 events in order. A full queue pauses reads, including unread control frames, while local heartbeats continue. Reads resume when capacity returns; that backpressure time does not count as remote inactivity. Sustained overload may still disconnect the server, after which recovery depends on its cache. Stopping cancels both workers.

Runtime Ping/Pong writes have a 5-second deadline; write failures or timeouts use a fixed 5-second reconnect delay. The initial handshake has an overall 15-second timeout. These are implementation bounds, not evidence of a measured production network failure.

Changing the site or Bot identity clears old approvals, pending requests and subscriptions. Rotating a token for the same Bot preserves them. Identity-check failures never fall back to a personal account or broader permissions.
