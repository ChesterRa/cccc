# ChatGPT Web Model Runtime

For the separate Grok Bot runtime, see [Grok Bot Web Model](/guide/grok-web-model-runtime). Its Actor routing uses per-call credentials rather than ChatGPT conversation pairing.

The `web_model` runtime lets a ChatGPT web chat participate in a CCCC group through browser delivery plus a remote MCP connector. In ChatGPT sessions that expose the CCCC MCP connector, **GPT-5.x** can act as a first-class local development actor: it can receive routed CCCC messages, call CCCC MCP tools, edit the active workspace, run scoped commands, inspect git output, and report back through the same coordination layer as Codex or Claude Code. When the selected GPT-5.x chat exposes the CCCC MCP connector, ChatGPT web capacity can become additional local-development agent capacity and reduce pressure on native Codex usage for work that fits the ChatGPT Web path.

MCP availability is determined by the selected ChatGPT model and account, not by CCCC. Use a ChatGPT session that can actually see the CCCC connector for local development. CCCC also offers an experimental **GPT Pro** delivery mode for accounts where attaching an image makes the connector available to a GPT Pro chat. This is an observed ChatGPT behavior rather than a supported model-selection API, so it can stop working when ChatGPT changes. CCCC never switches the ChatGPT model for you.

There are two delivery transports behind the same actor identity:

1. **Browser delivery**: CCCC claims a pending Send or Send + Reply batch and injects it into a bound ChatGPT web chat through the Web-owned shared browser with an Actor-specific Page target. It waits while ChatGPT is responding and preserves unrelated unsent drafts. It never treats the visible Stop control or a similarly named control elsewhere on the page as a send target. A confirmed injection records `runtime.delivery=accepted`; an indeterminate post-click result records `ambiguous`, and neither is automatically submitted again. A definite pre-submit failure records `failed` and remains eligible for a later delivery attempt.
2. **Remote-MCP pull**: ChatGPT calls `cccc_runtime_wait_next_turn` through MCP. Returning a turn records that the pull transport accepted it; `cccc_runtime_complete_turn` closes that exact active runtime turn after processing.

Browser confirmation requires the current batch's complete marker in a user
message. A successful click, older messages reappearing, or an assistant quoting
the prompt does not count as delivery. Until a receipt is observed, the message
remains unverified and later messages wait without overwriting the draft.

In both modes, transport delivery, Inbox reading, and runtime completion are separate
facts. Neither browser submission nor `cccc_runtime_complete_turn` advances the Inbox
Mail cursor; only `cccc_inbox_read` consumes Mail. The model does not make a
completion call for a browser-injected batch because the browser adapter closes that
turn. If the completion response is lost, reconciliation retries only the completion
identity and never replays the browser prompt. A mismatched active turn is rejected
instead of overwriting newer runtime state.

If a message shows **Delivery unverified**, use **Check and resume** beside that
message to inspect the Actor's ChatGPT conversation without searching through
settings. If you send the staged message manually, CCCC can recognize its exact
batch marker in that same conversation, update the delivery status and continue
with later queued work after ChatGPT finishes. If confirmation is still unavailable,
send or clear the draft, wait for the response to finish, then choose **Continue
after checking**. This only releases later work: it does not resend the uncertain
batch, reset pairing or clear conversation history.

Mental model: the ChatGPT Web Model actor is a normal CCCC agent whose model surface happens to be ChatGPT Web. It reuses the same `cccc_bootstrap`, `cccc_help`, messaging, coordination, capability, memory, and repository tool paths as Codex/Claude actors. Browser delivery and remote-MCP pull are transport adapters, not a separate help system.

Connector model: one authenticated CCCC connector serves multiple verified ChatGPT conversations. All Actors share one dedicated browser profile and login; each owns a persistent separate window and Page target. No active-tab scheduler chooses the delivery recipient. Rotating the global credential invalidates the old URL while preserving pairings. Revoking and recreating the connector requires new pairings.

In global Web Model settings, **Open login window** reveals the login window and restores the ChatGPT entry page if that window is blank. It preserves an ongoing sign-in flow. **Check login** only inspects readiness; it does not reload the page. **Close login window** closes only that window; Actor windows, queued work and the saved login remain intact. It does not require stopping Actors. Signing out of ChatGPT still affects all Actor windows sharing the login.

Actor windows become viewable as soon as navigation starts, including while the website is loading or awaiting verification. Delivery checks the target, composer and draft state separately before claiming work; opening a window does not mean the website is ready to receive a task.

The login window opens without waiting for the site's document to finish loading, so slow pages, network errors and verification remain visible and the window stays closable. Browser availability is separate from login and delivery readiness; automatic Actor delivery still checks the page before submitting.

MCP tool model: ChatGPT registers a remote MCP schema up front, so the ChatGPT Web Model connector advertises a fixed built-in schema instead of extending that schema with newly discovered capability tools. Explicitly disabling CCCC code mode removes its two code-mode tools; daemon restart timing does not collapse the remaining Web Model schema to the smaller ordinary-actor fallback. Calls are still authorized with the connector-bound actor identity. A Web Model actor cannot bypass that surface by naming an unadvertised tool directly. A group foreman can reach an enabled built-in capability-pack tool through `cccc_capability_use`; a peer cannot use that route to acquire foreman management authority.

### Viewing or changing a working conversation

A connected Actor shows its verified conversation and status. **View conversation**
opens the viewer on demand; hiding it does not close the browser or stop the Actor.
Normal Actor stop/start and CCCC restarts reuse the binding without another setup
message. The runtime panel's **ChatGPT conversation** button opens this section
of the Actor settings directly. **Refresh ChatGPT page** reloads that page; it does
not restart the shared browser.

To change the working conversation, select **Change working conversation**. You
can edit the URL while the Actor runs. Previewing or connecting asks to pause only
this Actor, so no separate Stop action is required. Preview a new chat or an
existing URL, enable the CCCC
connector in that chat, then select **Use this conversation**. CCCC sends the setup
message and verifies the reply automatically. Previewing alone does not switch the
binding. Failed or cancelled verification preserves the previous binding and
pending work. After successful replacement, **Start Actor** is available in the
same section; verification does not start it for you. These operations are independent of the
Actor editor's Save button. Disconnecting is a separate maintenance action and is
not required to replace a conversation.

**Shared login & connector** opens instance settings while retaining the Actor's
unsaved draft. Creating a connector credential only creates its address: it does
not prove that ChatGPT has added or used it. Shared settings separately show the
last recorded connector request and the number of verified Actor bindings.
Credential replacement and revocation are under **Manage connector credential**.

### Conversation pairing and diagnostics

The connector exposes `cccc_pair({"code":"…"})` and `cccc_connector_status({})` before pairing through ChatGPT's normal connector tool invocation. These connector-level tools are not available inside **CCCC's `cccc_code_exec`**; this restriction does not prescribe how ChatGPT internally dispatches connector calls. On normal startup, an enabled Actor without a binding or prior attempt automatically connects its conversation. CCCC sends one setup message, receives a unique receipt through MCP, and verifies its assistant-side echo after that exact message in the same owned window before saving the stable conversation URL. Only then does the existing delivery queue send the pending task. No preliminary pairing click, copied code or second CCCC confirmation is needed; any ChatGPT approval remains user-controlled.

The pairing code is generated by the selected CCCC instance and sent back only to that instance's connector. `cccc_pair` records a pending link; it does not read workspace files or execute tasks. It is declared as a state-changing tool, while `cccc_connector_status` is read-only. MCP acceptance alone grants no workspace access: after browser receipt verification, subsequent calls use the linked Actor's configured permissions. Codes expire after ten minutes and can be accepted by only one host conversation. If ChatGPT blocks or rejects a call, retain its original tool error for diagnosis. A host safety rejection does not establish that connector calls require a different invocation mode, and CCCC does not automatically replay a failed setup.

When an update changes connector tool descriptions or schemas, restart CCCC, open the existing CCCC connection in ChatGPT's Plugins settings and select **Refresh**, then test in a new conversation. Refreshing the browser page alone is not the tool-metadata refresh. See [OpenAI's metadata refresh instructions](https://developers.openai.com/plugins/deploy/connect-chatgpt#refresh-metadata).

Every business tool call resolves its Actor from the current request's host-supplied `params._meta["openai/session"]`, scoped with `openai/subject` and `openai/organization` when provided. The connector credential authenticates the caller; metadata correlates its conversation. Model arguments, initialize metadata and HTTP session headers cannot choose an Actor. Missing or invalid metadata, conflicting pairings, stopped/deleted Actors and old Actor generations fail closed. Raw host identifiers and pairing codes are not persisted; the store retains salted identity hashes and code hashes.

`cccc_connector_status` reports `routing_mode: session`, an opaque `session_fingerprint`, the pairing state, and this conversation's Group/Actor when bound. Compare A → B → refreshed A: A should keep its route, B should have a different route. Credential rotation preserves this fingerprint; recreating the connector does not. Local synthetic tests cannot prove that a particular ChatGPT account/model supplies stable host metadata. A host that does not supply it cannot use this routing mode; there is no default Actor fallback.

The host metadata fields are described in the [OpenAI host metadata reference](https://developers.openai.com/plugins/reference#_meta-fields-the-client-provides).

## Requirements

- A CCCC group with an attached workspace scope.
- An Actor with runtime `ChatGPT Web Model`.
- A public HTTPS URL that reaches `cccc web`.
- A ChatGPT account with remote MCP connector support.

ChatGPT developer mode supports remote MCP over SSE or streamable HTTP and does not connect to local MCP servers. Full local development requires the selected ChatGPT conversation to expose the CCCC connector and its write-capable tools. If the selected model cannot see the CCCC connector, that chat has no CCCC local access.

## Zero-to-ready setup

Follow this order. `Settings > Global > ChatGPT Web Model` shows the prerequisite status, handles shared sign-in and the one MCP app URL. Each Actor’s settings handles its conversation and pairing.

### 1. Start CCCC and expose Web

Start CCCC:

   ```bash
   cccc daemon start
   cccc web --port 8848
   ```

Expose Web through a public HTTPS tunnel or reverse proxy. ChatGPT runs in the cloud, so `localhost`, plain HTTP URLs, and private tailnet-only URLs cannot be used as the ChatGPT MCP server URL.

Practical options:

- **Cloudflare Tunnel**: recommended for most users. Example: `cloudflared tunnel --url http://127.0.0.1:8848`, then map the tunnel to an HTTPS hostname.
- **ngrok**: quick temporary public HTTPS URL for testing.
- **Tailscale Funnel**: public HTTPS exposure from a Tailscale node; ordinary tailnet-only Tailscale URLs are not enough for ChatGPT.
- **Caddy / Nginx / Traefik reverse proxy**: best when you already own a public host or domain.

Avoid putting an interactive login challenge in front of the MCP endpoint. The copied CCCC MCP URL already carries the instance connector credential; ChatGPT should be able to reach that URL directly over HTTPS.

In CCCC Web, open `Settings > Global > Web Access` and set the public Web URL, for example:

   ```text
   https://cccc.example.com/ui/
   ```

Protect public Web access with an Admin Access Token in the same panel. The MCP connector has its own credential; do not paste the administrator token into ChatGPT.

### 2. Sign in once and create the shared connector

Open `Settings > Global > ChatGPT Web Model`, open the login window and sign in. All Actor windows use this dedicated profile. Login from your everyday browser is separate; CCCC does not copy cookies. Signing out here signs out all ChatGPT Actors.

Create the shared connector and copy its private URL. The secret is shown only after creation or rotation. If the URL is local-only or not HTTPS, configure Web Access before continuing. Add only this one connector in ChatGPT.

In ChatGPT, open `Settings > Apps > Advanced settings > Create app`. ChatGPT menu names may vary by plan and workspace. If this exact path is not available, look for Apps or Connectors settings, enable Developer Mode if required, then create a custom MCP app/connector. Use these fields:

```text
Name: CCCC
Description: CCCC local workspace connector
MCP Server URL: paste the full CCCC MCP URL copied from Settings > ChatGPT Web Model
Authentication: No Auth
```

Check the custom MCP risk acknowledgement and click `Create`.

Open a GPT-5.x chat, select Developer mode/tools, and enable the CCCC connector. On the first CCCC tool call, ChatGPT may show an app permission approval card. Choose `Always allow` if that option is available and you trust this local CCCC connector; otherwise approve the action manually when ChatGPT asks. CCCC does not automate ChatGPT permission approvals. If CCCC was upgraded after the connector was created, refresh the app/tool list in ChatGPT settings so new tools such as `cccc_code_exec` are visible.

### 3. Start each Actor

1. Create or edit an Actor with runtime `ChatGPT Web Model` and save its configuration.
2. In its **ChatGPT conversation** section, optionally open an existing conversation URL. Otherwise the Actor opens a new chat. Each Actor has its own persistent window sharing the login from step 2.
3. Enable the same CCCC connector in the intended conversation and start the Actor normally. Complete any login or connector approval in ChatGPT itself. CCCC automatically sends one setup message and verifies its reply before delivering queued tasks. You can send Group messages while connection is pending; they stay queued.
4. Repeat for other Actors using separate conversations. No additional connector, login or pairing action is required.

Connection attempts last at most ten minutes. You may close settings while waiting, or cancel the attempt. Draft text and pending attachments are protected; before the first send, CCCC waits until the composer is available. After an attempted send, failure, cancellation, expiry or interruption requires **Retry connection**; background checks never replay it. Stopping an Actor or pausing/stopping its Group invalidates an unfinished automatic handshake, including a rapid stop/start. Established bindings survive ordinary restarts. Status GETs only observe state; they do not send or resume anything.

For a new chat, connection also waits for ChatGPT to assign its permanent conversation address. Its temporary address during the first response is part of normal startup, not a request to select or pair the conversation manually.

To replace an established conversation, choose **Change working conversation**, preview the intended conversation (confirm the pause if running), then use **Use this conversation**. An existing binding remains until the replacement is verified or explicitly unpaired. This deliberate replacement is separate from automatic first connection. An unpaired Actor's explicitly chosen URL is retained as a startup destination, never as tool authority.

Actor presets, notes, roles and capabilities remain applicable. Launch commands and private launch environment inputs are hidden for Web Model Actors and Profiles. Existing private environment values are retained, and hidden environment drafts are not applied when saving Web Model settings. Runtime Profiles reuse runtime type and default capabilities, not browser login, conversation or pairing.

Opening a URL does not silently change an existing pairing. An unrelated composer draft is never overwritten. One host session or one conversation URL cannot be assigned to two Actors. Closing an Actor window leaves the shared browser and other windows intact; reopen it explicitly to resume the surface. Closing the global login window also leaves Actor windows and their login intact.

### Upgrading from Actor-specific connectors

Old credentials are rejected rather than widened to instance authority. Configure the shared connector explicitly, update the ChatGPT app URL, and pair each Actor. Existing chats, ledgers and browser profile remain. Uncertain deliveries are retained and never replayed automatically. Re-pairing the same saved conversation preserves its pending evidence; changing away from an unresolved delivery requires review first. If an old pending new-chat delivery never obtained a stable URL, review that chat and recreate the stopped Actor for a new pairing; Group ledger history remains and the old batch is not automatically replayed.

### Optional manual check

Send a small CCCC message to the actor:

```bash
cccc send "Use CCCC MCP to read README.md and reply with one sentence." --group <group_id> --to <actor_id>
```

The message should appear in the bound ChatGPT conversation. ChatGPT should use CCCC MCP tools for the reply. If the ChatGPT app has not been seen by CCCC yet, ask ChatGPT directly:

```text
Use the CCCC connector and call cccc_bootstrap.
```

For remote-MCP pull mode, prompt the model to use CCCC explicitly:

   ```text
   Use the CCCC connector. First call cccc_runtime_wait_next_turn.
   For multi-step local development, prefer cccc_code_exec and call nested tools
   through tools.*. Direct tools remain available for simple steps: cccc_repo for
   read-only workspace inspection/search, cccc_repo_edit or cccc_apply_patch for edits,
   cccc_exec_command/cccc_write_stdin for commands/tests, cccc_git for
   status/diff/add/commit, cccc_message_send for visible replies, then
   cccc_runtime_complete_turn.
   Do not use built-in browsing or unrelated tools for CCCC work.
   ```

## Common setup blockers

- **MCP URL is localhost or HTTP**: ChatGPT cannot reach local URLs. Set a public HTTPS URL in `Settings > Global > Web Access`, then rotate/copy the MCP URL again.
- **ChatGPT cannot see the CCCC connector**: first use a ChatGPT model/account with Developer mode and the CCCC app enabled. If a GPT Pro chat exposes the connector only when an image is attached, select **GPT Pro (experimental)** in that actor's runtime panel; CCCC still cannot guarantee or control ChatGPT-side MCP availability.
- **Conversation is not paired**: refresh the ChatGPT app tool list, enable the connector in the intended conversation, and start the Actor. If a previous attempt failed or was cancelled, use **Retry connection** in its Actor settings. `cccc_connector_status` shows the current route without starting work.
- **ChatGPT says `CCCC tool has been disabled`**: first refresh the CCCC app/tool list, enable the connector for the current chat, and approve the trusted call in ChatGPT. That wording is normally ChatGPT-side permission or connector state. Treat it as a CCCC policy failure only when the CCCC connector activity panel records a concrete error such as `code_mode_disabled` or `permission_denied` for the same call.
- **ChatGPT is signed in but CCCC has not confirmed it**: open the embedded browser in `Settings > Global > ChatGPT Web Model` and use `Check status` if needed.
- **Conversation changed or tool access stopped**: stop this Actor, inspect its paired URL in Actor settings, and re-pair the intended conversation. If host metadata is missing, do not select a default Actor; report the connector diagnostic.

### ChatGPT Browser Delivery

Browser delivery is the proactive path for ChatGPT web. CCCC uses one Web-owned Chrome/Edge browser process with a shared login and separate Actor windows for settings, runtime inspection, optional manual reload, optional auto-reload recovery, and message delivery. Delivery submits CCCC message batches into the explicitly bound chat; the web model still uses the CCCC MCP connector for all visible replies and local work. Choose a GPT-5.x model/session that can see and use the CCCC connector for local execution. If the selected model cannot see MCP tools, switch to an MCP-capable GPT-5.x chat before assigning local work.

The actor runtime panel provides two durable delivery modes:

- **Standard** (default): text-only browser delivery. This is the recommended stable path.
- **GPT Pro (experimental compatibility mode)**: CCCC attaches one deterministic 32×32 blank PNG to each delivered batch before invoking Send. The image is transport-only and contributes no task context. CCCC uses the browser file input directly, never the OS clipboard, and treats attachment plus submission as one transaction. A pre-submit upload failure records a retryable failed delivery; a post-click ambiguous result is never automatically duplicated.

The setting is stored per group and actor and applies from the next accepted delivery, including after daemon restarts. It does not select Pro, change the active ChatGPT model, or guarantee that ChatGPT exposes the connector. Select the desired model in ChatGPT itself.

On native Linux, projected headed browsers require `Xvfb`. CCCC starts a private virtual display, removes inherited Wayland display markers, and forces Chrome/Edge onto X11 so the physical desktop never receives the browser window. Missing `Xvfb` is a startup error even when the host has a usable `DISPLAY`; CCCC does not silently expose the projected browser on the host desktop. Install `xvfb` with the distribution package manager, then restart the ChatGPT browser session. `cccc doctor` reports the system browser, required Xvfb isolation, and optional x11vnc viewer separately.

On macOS, the shared ChatGPT browser runs headless by default. The daemon still uses the installed system Chrome or Edge, the same persistent login profile, and the same CDP-backed **Page** projection, but it does not open or focus a separate desktop window during warmup or delivery. Sign-in and normal interaction happen through the embedded **Page** view. For temporary compatibility troubleshooting only, set `CCCC_WEB_MODEL_BROWSER_HEADLESS=0` and restart the ChatGPT browser session to restore the visible system-browser window.

The default submit timeout is 30 seconds and can be changed with `CCCC_WEB_MODEL_BROWSER_DELIVERY_TIMEOUT_SECONDS`. This is the outer delivery hard cap; slow page loads, composer waits, safe `Send prompt` discovery, and new-chat binding share that budget and may not each consume their full internal timeout. Browser startup is handled by the projected browser runtime, which requires a real system Chrome or Edge CDP-capable browser for ChatGPT. Automatic page reload recovery is disabled by default. To opt into the legacy recovery behavior for a fragile ChatGPT browser session, set `CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD=1`; the inactivity threshold is controlled by `CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD_INACTIVITY_SECONDS`. CCCC activates the verified Send control once through its normal page click handler, after rechecking the current conversation, exact draft, existing receipt and visible, enabled control in one page operation. It does not depend on moving the native mouse into an Actor window. After a submit action, only the exact current batch marker in a ChatGPT user message is direct acceptance evidence; loading more historical messages does not confirm delivery. Composer clearing, a conversation URL change, or generation controls alone remain corroborating but insufficient. Persisted ambiguous deliveries that contain direct user-message evidence are reconciled without resending, and a validated observed `/c/...` URL is bound as the conversation target. Once CCCC activates Send, an uncertain dispatch is recorded with `dispatch_unknown`; browser timing or a pre-existing running indicator cannot prove that the click did or did not dispatch, so the message is not automatically re-bundled. A pre-submit deferral is different: no submit action was attempted, so the delivery is recorded as failed and remains retryable. CCCC may stage the batch in its daemon-owned composer before discovering that no safe submit control is currently available; it only reuses staged text when its normalized content exactly matches the current batch, and it does not treat staging as submission. CCCC keeps one single-flight worker per Actor, retries pre-submit deferrals with bounded backoff, and does not append duplicate submitting events for the same batch. Each attempt snapshots the pending direct-delivery messages that exist at that time, so a newly arrived message may be coalesced into the next batch and produce a new deterministic delivery id. If the retry budget is exhausted, a failed batch remains eligible for the next normal trigger; direct-delivery events that arrived after the final snapshot produce a different delivery id and receive one fresh worker after single-flight is released. Inbox unread state is independent throughout this process.

Actor panels use **Page** view bound to their own target. Global login/maintenance additionally offers **Browser** view of the entire shared desktop on a CCCC-owned Xvfb display with `x11vnc` installed. That view can show other Actor windows and is available only in global settings. Without VNC, the native browser and CDP page view remain available. The VNC server binds to localhost and remote access passes through the authenticated CCCC WebSocket bridge. `CCCC_PROJECTED_BROWSER_VNC=0` disables it.

The shared process has one exclusive profile owner. Actor close/delete only closes its page; Web shutdown closes the owned process and display resources once. Other browser features keep their separate profile ownership. A browser crash affects all shared windows and recovery uses their explicit bindings; it never adopts an unrelated tab.

The login and delivery paths share this profile:

```text
CCCC_HOME/state/web_model_browser/_shared/chatgpt_web/chrome_profile
```

Enable browser delivery with:

```bash
export CCCC_WEB_MODEL_DELIVERY_MODE=browser
```

Browser delivery is the default for paired ChatGPT Actors. Explicit pull/off delivery preferences remain supported.

For a browser-delivered batch, the injected prompt already contains the messages. The model should not call `cccc_runtime_wait_next_turn` first for that injected batch. It should work from the injected messages, use normal CCCC MCP tools, and call `cccc_help` if the workflow is unclear.

### Prompt and Help Layering

The browser-injected prompt should stay small. Each embedded message uses the same actor-facing format as normal peers: sender, audience, full current `event_id`, an optional short parent correlation marker, and `reply_required` only when it changes the required action. It does not repeat the canonical storage mode or full parent id. `event_id` is the value to pass to `cccc_message_reply` when answering that message. The first injected batch in a paired ChatGPT conversation also carries the normal actor system prompt plus a short Web transport note; later batches do not repeat that seed. Durable collaboration rules belong in the shared `cccc_help` path, including the Web Model Transport runtime note appended for `runtime=web_model` actors.

Use this split to avoid duplicate or drifting instructions:

- Shared agent behavior: `cccc_bootstrap`, `cccc_help`, actor notes, capability state, context, memory, and messaging rules.
- Web transport behavior: do not pull a browser-injected batch again; do pull when operating in remote-MCP mode without an injected batch; visible communication must use CCCC MCP tools. Confirmed browser submission records accepted delivery, post-click uncertainty records ambiguous delivery, and neither is automatically redelivered. Definite pre-submit failure remains retryable. Runtime completion and Inbox reading remain separate operations.

## Smoke Test

Check that the remote MCP endpoint is reachable:

```bash
curl -s "$CONNECTOR_URL" \
  -H "Authorization: Bearer $SECRET" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"limit":200}}'
```

For clients that probe the streamable HTTP/SSE receive path, the connector also accepts:

```bash
curl -i "$CONNECTOR_URL?token=$SECRET"
```

The expected response is `text/event-stream` with a short readiness comment.

Expected tools include:

- `cccc_runtime_wait_next_turn`
- `cccc_runtime_complete_turn`
- `cccc_code_exec`
- `cccc_code_wait`
- `cccc_repo`
- `cccc_repo_edit`
- `cccc_apply_patch`
- `cccc_shell`
- `cccc_exec_command`
- `cccc_write_stdin`
- `cccc_git`
- `cccc_message_send`

Then send work to the actor:

```bash
cccc send "Read README.md and report back through CCCC." --group <group_id> --to <actor_id>
```

For pull mode, pull a turn:

```bash
curl -s "$CONNECTOR_URL" \
  -H "Authorization: Bearer $SECRET" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"cccc_runtime_wait_next_turn","arguments":{}}}'
```

## Reading local images

File reads and attachment delivery have separate tools: `cccc_file` is read-only
(default action: `read`; also `info` and `blob_path`). Use
`cccc_file_send(path="...", to="user", mode="send")` to deliver a file to the
human user. Agent Mail uses the existing agent-only audience rules. The old
`cccc_file(action="send")` call is rejected; it does not silently send a file.

After upgrading, restart CCCC and **Refresh** the existing CCCC connection in
ChatGPT Plugins so the tool names, schemas and permission annotations update.
Verify the refreshed catalog before testing in a new conversation. Accurate tool
metadata does not guarantee acceptance by ChatGPT's safety checks. If a call is
blocked, report the actual tool error and seek the user's next instruction;
do not automatically retry through a different tool or claim it succeeded.

Use `cccc_file` with `action="read"` and an active-workspace relative path, or the
`state/blobs/...` path of an attachment delivered to this Group:

```json
{"action":"read","rel_path":"screenshots/page.png"}
```

PNG, JPEG and WebP files are detected from their bytes and returned as native MCP
image content. **Original bytes are preserved**, including any existing metadata;
CCCC does not decode, crop, resize, recompress or reorient them. The image limit is
20 MiB. An explicit `max_bytes` may lower that limit; oversized images fail rather
than returning a partial image. Image interpretation, including orientation and
animated-frame handling, belongs to the selected client/model.

Inside code mode, image reads automatically include the image in the next
`cccc_code_exec` / `cccc_code_wait` result. JavaScript receives only file metadata;
base64 transport bytes do not consume the text-output budget or pass through the
JavaScript bridge. No `image()` helper is needed:

```js
const file = await tools.cccc_file({action: "read", rel_path: "screenshots/page.png"});
text(file);
```

Each code-mode result can include up to **four binary items totaling 20 MiB** before
base64 encoding. Read fewer/smaller files or call `await yield_control()` before
reading more. These limits fail explicitly, without silently dropping images.
Ordinary UTF-8 reads return a bounded text preview (200,000 bytes by default,
configurable with `max_bytes` up to 5 MiB) and a `truncated` flag. Use
`cccc_repo(read)` when line continuation or a file hash is needed.

After restarting the updated CCCC binary, test a known local image through the
existing connector and ask for details available only in its pixels. Check both
the tool call and answer; an image preview alone is not proof of understanding.
Refresh the connector's tool definitions if it still advertises an older schema.
No separate media connector is required.

### Native-input boundary

This bridge adapts inputs the selected ChatGPT client/model can consume through
MCP. It does not supply local PDF extraction/rendering, image editing, OCR, Office
parsers, audio transcription or video frame extraction. `region`, `page` and
`view` are not supported. Unsupported binary reads fail explicitly; PDF/PPTX
reads only forward original bytes. `info` and `blob_path` still
return ordinary metadata/path, without starting any
helper process. Neither metadata nor a local path means the model has read the
file. The existing Presentation viewer, Voice features and general local command
tools are separate capabilities and are unaffected by this boundary.

PDF and PPTX handoff have been successfully tested by a user in ChatGPT. Other
Office formats, audio and video remain unsupported by this file bridge. ChatGPT upload support does not establish MCP
support: OpenAI's
[Responses file-input documentation](https://developers.openai.com/api/docs/guides/file-inputs)
describes `input_file`, a different interface from
[ChatGPT MCP tool results](https://developers.openai.com/plugins/reference#tool-results).
Do not infer one interface's file support or size limits from the other.

Keep new formats limited to a small original-file handoff and an actual client
test. Do not add a local converter, a paid inference fallback or a browser-upload
workaround to simulate native support.

### Original PDF and PPTX handoff

`cccc_file(action="read")` returns original PDF and PPTX bytes as inline MCP
embedded resources (`type="resource"`, base64 `blob`), using MIME `application/pdf`
or `application/vnd.openxmlformats-officedocument.presentationml.presentation`.
The `cccc-file:///...pdf` or `...pptx` URI identifies the content; it is not a
public download endpoint. No new connector or document helper program is needed.

Files are identified by their contents, including extensionless Group attachments.
For PPTX, CCCC checks only ZIP format metadata to distinguish the presentation
format from other archives; it does not extract slide text, notes, images or
layout. The original file remains unchanged. The 20 MiB per-file limit applies;
documents and images share the four-item/20 MiB aggregate code-mode budget.
Binary files are never truncated. Code mode emits the native resource while
JavaScript receives metadata, just as with images.

User testing on 2026-09-20 confirmed both routes: ChatGPT read a 17-slide PPTX
through its native file reader and opened a one-page image-only PDF through its
native page reader, with no local extraction or rendering. These results establish
the tested ChatGPT route; content interpretation remains the selected
client/model's responsibility.

After `cccc_file(action="read")`, use ChatGPT's native file reader. For PDF,
read the text layer when useful, then use native page/image viewing for pages
whose visual content matters. An image-only PDF can have an empty text layer
while its page image is readable. A page-image reference alone is not a transcript;
inspect the image before describing its contents. For long files, read the pages
needed for the task and state the actual coverage.

Native PPTX text access does not establish that slide layout, embedded images or
charts are visible. If the client's native reader cannot expose requested content,
report that limitation. This bridge does not silently extract, render or convert
locally to compensate.

## Local command execution

`cccc_shell` and `cccc_exec_command` execute a program and its arguments directly.
Command strings support shell-style quoting, but `&&`, pipes, redirection, variable
expansion, and wildcards are not interpreted. The tool names remain unchanged for
existing connectors. Invoke an installed shell explicitly when needed:

```json
{"command":"sh -c 'pwd && git status --short | head -5'"}
```

On Windows, for example:

```json
{"command":"powershell.exe -NoProfile -Command 'Get-Location; Get-ChildItem'"}
```

Both tools accept a relative `cwd` inside the Group's active workspace and `env`
overrides for the child process. `cccc_exec_command` also accepts `workdir` as an
alias. This constrains the starting directory, not what an authorized local
program can subsequently access. Commands run with the CCCC host process's OS
permissions, including access outside the workspace and network access. Actor
binding authenticates which Group/Actor is calling; it does not create a
filesystem or network sandbox. Use a container, VM, or OS-level isolation when
that boundary is required. `cccc_shell` defaults to a 60-second timeout
and retains at most 200,000 bytes per output stream; `max_output_bytes` can raise
that limit to 1,000,000. Check the exit code and truncation flags before relying
on command output.

`cccc_exec_command` returns initial output and a `session_id`. `yield_time_ms`
(default 1,000; 0–30,000) waits for new output or exit without delaying other
sessions. `cccc_write_stdin` returns only output not previously consumed by that
session. Its output budget defaults to 200,000 bytes (maximum 1,000,000); a UTF-8
character may extend the budget by up to three bytes instead of being split.
Both calls include `status`, `cursor`, `has_more`, `cursor_expired`, `timed_out`
and `closed`. Continue polling until `closed=true`: an exited command can still
have unread output pages. Empty output alone does not indicate completion.

`timeout_s` is a **hard command lifetime**, default and maximum 600 seconds,
and is enforced even without another tool call. Polling does not extend it.
`terminate=true` stops the owned process tree and returns unread output; it does
not also submit `chars`. The command retains a bounded 2 MB output window;
`cursor_expired=true` explicitly reports overwritten unread history. Redirect
large logs to a workspace file when the whole log is needed. At most 64 sessions
are retained per host/Home; abandoned results expire ten minutes after the
command deadline. Commands are not persistent service hosting. Transport failure
or a cancelled wait does not prove that input was not executed; do not silently
replay input or start the same command again.

### Bounded repository inspection

`cccc_repo` accepts only its declared read-only actions; editing requires
`cccc_repo_edit` or `cccc_apply_patch`. `path` and `file_path` are aliases across
reads and edits; supplying both with different values is an error. Move accepts
`dest_path` or `to_path` (existing `new_path` callers remain supported).

- `read` honors line ranges and `max_bytes` (default 200,000; maximum 1,000,000).
  It streams the file to calculate the whole-original-file hash while retaining
  only the bounded selected range. Text retains the existing LF-separated,
  no-final-LF presentation. `truncated` means the requested range was not fully
  returned; use `next_start_line`. If `partial_last_line=true`, that line is
  incomplete: reread it with a larger budget rather than advancing past it. For
  a line exceeding the maximum budget, use a focused shell command. A full-file
  hash still requires reading the whole file, even when only one line is shown.
- `search` honors a directory or single-file `path`, `case_sensitive` (false by
  default), `regex`, `include_globs`, `exclude_globs`, `context_lines`, `limit`
  (default 200; maximum 500) and `max_bytes`. Context appears in each hit's
  `before`/`after` arrays. Result budgets count serialized hit bytes, excluding
  response metadata. Oversized files (`max_file_bytes`, default 200,000; maximum
  1,000,000), non-text files and unreadable paths are counted in `skipped_files`.
  `incomplete=true` distinguishes skipped oversized/unreadable files or limits
  from a complete search with no matches. `truncated_reason` identifies result,
  output or scan limits; narrow the path/globs or adjust the corresponding limit.
- `list` returns immediate entries; `list_dir` uses `depth` (default 2, maximum 8;
  depth 1 is immediate children). Both return sorted, filtered entries with a
  scope-relative `path`, honor `limit`/`max_bytes`, and use 1-indexed `offset` plus
  `next_offset`. Pagination assumes the directory contents and filters are
  unchanged. `scan_truncated=true` requires narrowing the path; no reliable next
  offset is offered for a partially scanned tree. Unreadable paths are reported.

Traversal excludes dot-prefixed entries unless `include_hidden=true`, and does
not descend into `.git`, `target` or `node_modules`; explicitly targeting those
directories remains possible. Globs are case-sensitive workspace-relative paths
with `/` separators on every platform: `*` stays within a component, `**` crosses
directories, and exclusions win. `.gitignore` is not implicitly applied. Listing
can show symbolic links, but traversal never follows them. Explicit internal
links remain usable after scope validation; external links are rejected.

A traversal inspects at most 10,000 entries, and one search reads at most 64 MiB
of file contents. Reaching those bounds is reported, never represented as a
complete empty result. An output budget too small for even one hit or entry
returns an actionable error. Inspection runs off the async request executor;
there is no background index, external `rg` requirement or automatic retry.

### Scoped editing

`cccc_repo(action="read")` returns a SHA-256 of the **whole original file**, even
for a line-range read. Supply it as `expected_sha256` for a subsequent exact edit.
`multi_replace` validates ordered replacements in memory and writes the file once,
so a rejected later replacement leaves the original untouched. `replace_all` is
explicit; `expected_replacements` checks the match count. Existing file modes are
preserved, including executable scripts.

Codex-style `cccc_apply_patch` accepts exact `@@ context` anchors, ordered hunks,
`*** End of File`, `*** Move to:` and additions in new directories. It preserves
existing line endings and the final-newline convention. An unanchored hunk must
match uniquely; an exact unique anchor searches forward for the first matching
block. No whitespace or punctuation guessing is performed. Move destinations
must not exist; combine edits to the same path into one section. All sections
are checked before writing, but multiple files are not one filesystem transaction:
if an OS write fails, inspect the reported completed paths before retrying.

After changing connector tool metadata, restart the updated CCCC build, refresh
the connection in ChatGPT, and verify the tools in a new conversation. Local
MCP tests alone do not establish ChatGPT's cached metadata or model performance.

`cccc_code_exec` treats JavaScript strings, comments, regular expressions and
raw template text as data. Node module loading remains unavailable: static
imports are invalid in the cell function, dynamic imports have no loader, and
`require` is not exposed. Executable failures are reported when evaluated, like
other JavaScript errors; effects of earlier nested tool calls are not rolled back.

Code-mode `yield_time_ms` limits the current wait for results, including nested
tool calls. A slow nested tool keeps running in its cell and its result is
collected by `cccc_code_wait`; a poll deadline does not cancel or replay it.
Explicit cell termination, expiry, and host shutdown cancel outstanding nested
calls. Changes already made are not rolled back.

### Unverified browser delivery and unsent drafts

If CCCC cannot verify a browser submission, later deliveries to that chat pause
so they cannot replace an unsent draft. Check the saved conversation in the
embedded browser, manually send or clear the draft, and wait for any response
to finish. In the delivery-target section, **I checked ChatGPT — resume** resumes
queued messages after checking that the saved chat is open and its composer is
empty. It does not resend or mark the unverified message as accepted. A conversation needs a verified binding before new deliveries can start. Ordinary unsent drafts detected before a new batch is claimed
are also preserved; queued delivery continues once they are sent or cleared.

## Current Boundaries

- `web_model` does not spawn a local PTY or local headless model process.
- Connector secrets are one-time visible; CCCC stores only a hash.
- Connector and per-binding activity are best-effort diagnostics. Pairing is authoritative; activity or a successful tool-list request is not proof of a ready Actor.
- Unknown or malformed tool calls return JSON-RPC protocol errors. A known tool that fails execution or policy checks returns an MCP tool result with `isError: true`; the native server includes the daemon's machine-readable `code`, `message`, and non-empty `details` in `structuredContent.error` as well as the text content.
- Only tools whose declared operation is read-only are annotated with `readOnlyHint: true`. Mixed-action and mutating tools remain unannotated so a client is not encouraged to bypass approval for a write path.
- The ChatGPT Web Model `tools/list` is intentionally stable for ChatGPT registration. Direct calls remain limited to that advertised surface; hidden built-in capability-pack tools must pass through `cccc_capability_use` and its actor-role checks.
- ChatGPT Web Model local-power tools are actor-bound. Repository/file APIs validate their paths against the active workspace (or authorized Group blobs). Shell execution and Git subprocesses start in that workspace but retain the host process's OS permissions; scope and identity binding are not an OS sandbox.
- Local `cccc_shell`, `cccc_git`, and unified-diff `cccc_apply_patch` calls own finite commands. Timeout covers input, output, and process exit; cancelling the call ends its owned process tree. This does not undo changes already made by a command. Use `cccc_exec_command` for foreground work within its declared lifetime, rather than leaving background children behind a completed shell.
- Shell results follow `max_output_bytes` (default 200,000); Git results retain at most 2,000,000 bytes per output stream and report `stdout_truncated` / `stderr_truncated`. Excess output is drained within the same command deadline instead of accumulating in memory.
- Local `cccc_exec_command` sessions belong to the CCCC host process, Home, Actor and conversation binding. Host shutdown ends those commands; closing a browser tab does not. Cleanup leaves Actor/Analyst sessions and other hosts alone.
- ChatGPT proactive delivery depends on the shared projected browser session and an active logged-in browser profile.
- New chats are created and paired explicitly in Actor settings before delivery. Browser history is diagnostic only and cannot grant routing authority.
- GPT-5.x is selected inside ChatGPT. CCCC treats ChatGPT Web Model as one browser-delivery/runtime path, not as a separate provider per model.
- GPT Pro compatibility mode is an experimental browser-transport workaround, not a supported ChatGPT model API. It may stop working when ChatGPT changes, and local access still depends on the selected chat exposing the CCCC connector.
- ChatGPT Web Model prompt/help behavior intentionally reuses the normal CCCC agent help path; only the transport note is runtime-specific.

## References

- OpenAI Apps SDK: Connect from ChatGPT: https://developers.openai.com/apps-sdk/deploy/connect-chatgpt
- OpenAI Apps SDK: Testing and tool refresh guidance: https://developers.openai.com/apps-sdk/deploy/testing
- OpenAI Help: Developer mode and MCP apps in ChatGPT: https://help.openai.com/en/articles/12584461-developer-mode-apps-and-full-mcp-connectors-in-chatgpt-beta
