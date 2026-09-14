# ChatGPT Web Model Runtime

The `web_model` runtime lets a ChatGPT web chat participate in a CCCC group through browser delivery plus a remote MCP connector. In ChatGPT sessions that expose the CCCC MCP connector, **GPT-5.x** can act as a first-class local development actor: it can receive routed CCCC messages, call CCCC MCP tools, edit the active workspace, run scoped commands, inspect git output, and report back through the same coordination layer as Codex or Claude Code. When the selected GPT-5.x chat exposes the CCCC MCP connector, ChatGPT web capacity can become additional local-development agent capacity and reduce pressure on native Codex usage for work that fits the ChatGPT Web path.

MCP availability is determined by the selected ChatGPT model and account, not by CCCC. Use a ChatGPT session that can actually see the CCCC connector for local development. CCCC also offers an experimental **GPT Pro** delivery mode for accounts where attaching an image makes the connector available to a GPT Pro chat. This is an observed ChatGPT behavior rather than a supported model-selection API, so it can stop working when ChatGPT changes. CCCC never switches the ChatGPT model for you.

There are two delivery transports behind the same actor identity:

1. **Browser delivery**: CCCC claims a pending Send or Send + Reply batch and injects it into a bound ChatGPT web chat through the shared daemon-owned projected browser session. If ChatGPT is already responding or its composer contains an unrelated draft, CCCC waits without replacing the draft or pressing Send/Stop. This normal busy state does not consume the connection-error retry budget. A confirmed injection records `runtime.delivery=accepted`; an indeterminate post-click result records `ambiguous`, and neither is automatically submitted again. A definite pre-submit failure records `failed` and remains eligible for a later delivery attempt.
2. **Remote-MCP pull**: ChatGPT calls `cccc_runtime_wait_next_turn` through MCP. Returning a turn records that the pull transport accepted it; `cccc_runtime_complete_turn` closes that exact active runtime turn after processing.

In both modes, transport delivery, Inbox reading, and runtime completion are separate
facts. Neither browser submission nor `cccc_runtime_complete_turn` advances the Inbox
Mail cursor; only `cccc_inbox_read` consumes Mail. The model does not make a
completion call for a browser-injected batch because the browser adapter closes that
turn. If the completion response is lost, reconciliation retries only the completion
identity and never replays the browser prompt. A mismatched active turn is rejected
instead of overwriting newer runtime state.

Mental model: the ChatGPT Web Model actor is a normal CCCC agent whose model surface happens to be ChatGPT Web. It reuses the same `cccc_bootstrap`, `cccc_help`, messaging, coordination, capability, memory, and repository tool paths as Codex/Claude actors. Browser delivery and remote-MCP pull are transport adapters, not a separate help system.

Connector model: each CCCC group can have one ChatGPT Web Model actor. Each actor owns its connector identity and saved target conversation. Multiple groups share one physical ChatGPT browser, with navigation and submission serialized; their inboxes, bindings and delivery records remain separate. Rotating the MCP URL creates a new secret and revokes the previous active URL.

MCP tool model: ChatGPT registers a remote MCP schema up front, so the ChatGPT Web Model connector advertises a fixed built-in schema instead of extending that schema with newly discovered capability tools. Explicitly disabling CCCC code mode removes its two code-mode tools; daemon restart timing does not collapse the remaining Web Model schema to the smaller ordinary-actor fallback. Calls are still authorized with the connector-bound actor identity. A Web Model actor cannot bypass that surface by naming an unadvertised tool directly. A group foreman can reach an enabled built-in capability-pack tool through `cccc_capability_use`; a peer cannot use that route to acquire foreman management authority.

## Requirements

- A CCCC group with an attached workspace scope.
- A running actor with runtime `ChatGPT Web Model`.
- Either an OpenAI Tunnel forwarding to `cccc mcp --gateway`, or a public HTTPS URL that reaches the existing actor-bound HTTP connector.
- A ChatGPT account with remote MCP connector support.

ChatGPT developer mode supports remote MCP over SSE or streamable HTTP and does not connect to local MCP servers. Full local development requires the selected ChatGPT conversation to expose the CCCC connector and its write-capable tools. If the selected model cannot see the CCCC connector, that chat has no CCCC local access.

## One Tunnel, multiple ChatGPT conversations

The optional native entry `cccc mcp --gateway` accepts conversation identity from
the trusted transport's `params._meta["openai/session"]`. It is a stdio mode, not
an unauthenticated public HTTP endpoint. Normal `cccc mcp` and existing
actor-specific connectors keep their existing behavior.

Run the Web UI and the MCP process with the **same `CCCC_HOME`**. Register the
native command as the target of an existing OpenAI Tunnel using
`tunnel-client runtimes connect`; credentials belong in the tunnel client's
`file:` or `env:` references, not CCCC source or command literals. No public Web
port is required for this path. Keep the native Web process running when browser
return delivery is needed.

In a ChatGPT conversation that has the App enabled:

- `cccc_group_create(path, title?, chat_url?)` creates the project group and its
  web Foreman, then binds the calling conversation. Repeating the same request
  reuses the group and does not change the installation's global active group.
- `cccc_group_bind(group, chat_url?)` binds an unowned existing group or updates
  this conversation's own return target. It does not take over another Chat or
  silently replace a local Foreman.
- A group already owned by another Chat is never reassigned silently. To move
  such a group to a new Chat, disconnect the existing chat binding in
  `Settings > Global > ChatGPT Web Model` first; the new Chat can then bind the
  group with `cccc_group_bind(group, chat_url?)`.

`inbound_bound` confirms tool access only. `callback_target_ready` confirms a
saved stable `/c/...` URL, not a successful browser delivery. Without a URL the
status is `needs_chat_url`: local member management and dispatch work, but the
browser cannot wake that Chat. After changing Chats, explicitly save the new
return target; the previous Chat's target is no longer eligible for delivery.

The bound Foreman can manage local members of its own group, including their
configuration. It cannot select another group by supplying a different group ID.
Nested capability/code-mode calls preserve the same boundary. The **Change
Foreman** menu uses the existing actor editor: choose Web model or Local model,
then review and save; choosing the menu item alone changes no running actor.

Every delegated turn for a managed local member uses its existing structured
provider connection. Acceptance requires a real native turn receipt; writing
text into an attached terminal is not evidence that a model started working.
The attached terminal remains the visible projection of the same session. When a member ends a
turn, an existing Mail report to its web Foreman is promoted using native message
delivery; an existing direct report is reused. Only a missing report produces a
short deduplicated reminder. This never marks the overall task complete. Paused
or stopped groups are not automatically resumed.

Browser operations share one physical page. A busy page can delay another group's
browser notification, but does not block that group's local tools or local agents.
Completed member reports require an explicit `cccc_coordination(action="decide")`
responsibility decision. `continue` creates the next real task; reading a report
alone does not resolve it. Keep the original report as the human-facing output.

The original actor-bound HTTP setup remains available below and is collapsed in
the Web Model settings panel.

## Zero-to-ready setup

Follow this order. `Settings > Global > ChatGPT Web Model` shows the prerequisite status, then handles only the ChatGPT-specific steps: sign-in, MCP app URL, and delivery target.

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

Avoid putting an interactive login challenge in front of the MCP endpoint. The copied CCCC MCP URL already carries the actor-bound token; ChatGPT should be able to reach that URL directly over HTTPS.

In CCCC Web, open `Settings > Global > Web Access` and set the public Web URL, for example:

   ```text
   https://cccc.example.com/ui/
   ```

Create an Admin Access Token in the same Web Access panel. CCCC uses this public endpoint and access-token setup to generate an actor-bound MCP URL for ChatGPT.

### 2. Create the ChatGPT Web Model actor

In CCCC Web, open the target group and create one actor with runtime `ChatGPT Web Model`. This is the single CCCC actor identity that ChatGPT will use. Start it from the group actor controls if it is stopped, then return to `Settings > Global > ChatGPT Web Model`.

### 3. Sign in to ChatGPT

In `Settings > Global > ChatGPT Web Model`, open the embedded ChatGPT browser and sign in once. CCCC reuses that browser profile for delivery.

### 4. Connect the ChatGPT MCP app

After ChatGPT sign-in, create or copy the actor-bound MCP URL in `Settings > Global > ChatGPT Web Model`. If the page says the URL is local-only or not HTTPS, return to `Web Access` and fix the public Web URL before continuing.

In ChatGPT, open `Settings > Apps > Advanced settings > Create app`. ChatGPT menu names may vary by plan and workspace. If this exact path is not available, look for Apps or Connectors settings, enable Developer Mode if required, then create a custom MCP app/connector. Use these fields:

```text
Name: CCCC
Description: CCCC local workspace connector
MCP Server URL: paste the full CCCC MCP URL copied from Settings > ChatGPT Web Model
Authentication: No Auth
```

Check the custom MCP risk acknowledgement and click `Create`.

Open a GPT-5.x chat, select Developer mode/tools, and enable the CCCC connector. On the first CCCC tool call, ChatGPT may show an app permission approval card. Choose `Always allow` if that option is available and you trust this local CCCC connector; otherwise approve the action manually when ChatGPT asks. CCCC does not automate ChatGPT permission approvals. If CCCC was upgraded after the connector was created, refresh the app/tool list in ChatGPT settings so new tools such as `cccc_code_exec` are visible.

### 5. Choose the delivery target

The delivery target panel separates the saved target from the current browser tab. The current tab is visible for sign-in and inspection only; CCCC does not use it for delivery until you save it as the target.

To use an existing conversation, open it in the embedded browser, choose `Use current browser chat`, then click `Save target`. To start fresh, choose `Start new chat on next delivery`, then click `Save target`; CCCC will deliver the first prompt to ChatGPT and bind the actor once ChatGPT creates the final `/c/...` URL. You can also paste a specific `https://chatgpt.com/c/...` URL and save it.

Browser delivery never guesses between unrelated ChatGPT tabs. An existing chat is bound by saved URL; a new chat is a saved pending target until the first delivery produces a concrete ChatGPT conversation URL. ChatGPT may briefly expose a provisional `/c/WEB:...` route while creating a conversation; CCCC does not persist that route and waits for a stable final `/c/...` URL. Once that first delivery crosses the browser dispatch fence, CCCC only resolves and binds the pending target: a timeout or restart never resubmits the same batch to create another chat. For an existing-chat delivery, CCCC verifies that navigation finishes on the saved conversation before touching the composer; a redirect to the home page or another chat records a definite delivery failure without submitting the prompt. If the final URL cannot be recovered after a submit action, the target remains pending until it is resolved or the user explicitly selects a new target. The diagnostic `last_tab_url` is not a delivery target.

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
- **CCCC still says the MCP app is not connected**: after creating the app in ChatGPT, ask the model to call `cccc_bootstrap` once, or refresh the app/tool list in ChatGPT settings.
- **ChatGPT says `CCCC tool has been disabled`**: first refresh the CCCC app/tool list, enable the connector for the current chat, and approve the trusted call in ChatGPT. That wording is normally ChatGPT-side permission or connector state. Treat it as a CCCC policy failure only when the CCCC connector activity panel records a concrete error such as `code_mode_disabled` or `permission_denied` for the same call.
- **ChatGPT is signed in but CCCC has not confirmed it**: open the embedded browser in `Settings > Global > ChatGPT Web Model` and use `Check status` if needed.
- **Messages go to the wrong ChatGPT chat**: open `Settings > Global > ChatGPT Web Model`, check `Saved target`, then choose `Use current browser chat`, `Start new chat on next delivery`, or paste an explicit `chatgpt.com/c/...` URL and click `Save target`.

### ChatGPT Browser Delivery

Browser delivery is the proactive path for ChatGPT web. CCCC reuses one dedicated Chrome/Edge profile for settings and message delivery. Automatic delivery opens the browser only when the native runtime queue contains work. Delivery submits CCCC message batches into the explicitly bound chat; the web model still uses the CCCC MCP connector for all visible replies and local work. Choose a GPT-5.x model/session that can see and use the CCCC connector for local execution. If the selected model cannot see MCP tools, switch to an MCP-capable GPT-5.x chat before assigning local work.

The actor runtime panel provides two durable delivery modes:

- **Standard** (default): text-only browser delivery. This is the recommended stable path.
- **GPT Pro (experimental compatibility mode)**: CCCC attaches one deterministic 32×32 blank PNG to each delivered batch before invoking Send. The image is transport-only and contributes no task context. CCCC uses the browser file input directly, never the OS clipboard, and treats attachment plus submission as one transaction. A pre-submit upload failure records a retryable failed delivery; a post-click ambiguous result is never automatically duplicated.

The setting is stored per group and actor and applies from the next accepted delivery, including after daemon restarts. It does not select Pro, change the active ChatGPT model, or guarantee that ChatGPT exposes the connector. Select the desired model in ChatGPT itself.

On native Linux, projected headed browsers require `Xvfb`. CCCC starts a private virtual display, removes inherited Wayland display markers, and forces Chrome/Edge onto X11 so the physical desktop never receives the browser window. Missing `Xvfb` is a startup error even when the host has a usable `DISPLAY`; CCCC does not silently expose the projected browser on the host desktop. Install `xvfb` with the distribution package manager, then restart the ChatGPT browser session. `cccc doctor` reports the system browser, required Xvfb isolation, and optional x11vnc viewer separately.

On macOS, the shared ChatGPT browser opens as a normal windowed system Chrome or Edge session. It uses the installed browser, the same persistent login profile, and the same CDP-backed **Page** projection; the window can be sent to the background during warmup or delivery. A fully headless mode is available for troubleshooting via `CCCC_WEB_MODEL_BROWSER_HEADLESS=1`, but it is not the default, and the headless ChatGPT flow has not been verified as a supported setup.

The default submit timeout is 30 seconds and can be changed with `CCCC_WEB_MODEL_BROWSER_DELIVERY_TIMEOUT_SECONDS`. This is the outer delivery hard cap; slow page loads, composer waits, safe `Send prompt` discovery, and new-chat binding share that budget and may not each consume their full internal timeout. Browser startup is handled by the projected browser runtime, which requires a real system Chrome or Edge CDP-capable browser for ChatGPT. Automatic delivery is on demand: claim an original queued batch, open its bound conversation, and submit only when the fresh page is ready. A busy or incompletely loaded page is closed and checked again after five minutes. This deadline is account-wide and persisted; another group, a new message, or a service restart cannot bypass it. The existing supervisor checks local state between visits; it does not keep a page or a temporary confirmation tab open. With no pending batch, Chrome remains closed. Successful delivery retains the existing direct acceptance checks, then releases the automatic browser; a new chat stays open until its stable target URL is recorded. The model's final response is not inferred from transport completion. Manual setup stays open until a real queued report admits a delivery visit; login and unrelated drafts remain protected. Once a usable page is handed to automatic delivery, its normal close-after-visit behavior applies.

An archived conversation blocks only its own target; CCCC never automatically cancels the archive. A visible request/access restriction pauses automatic browsing for the shared account, including other groups. The pause persists across service restarts and new messages. Manually closing the shared browser also pauses automatic reopening; an automatic rest does not. After resolving a restriction, use the existing **Open browser** action; automatic access resumes only when that explicit action observes a ready page. Paused/stopped groups still require their own explicit resumption.

After a submit action, a provisional `request-*` ChatGPT turn is not acceptance evidence, even when it already echoes the message or increases the visible message count. CCCC keeps the existing bounded receipt check until that temporary turn settles or an identified, nonempty assistant response appears after the current user message. Earlier answers and empty placeholders do not qualify. An unconfirmed optimistic echo stays ambiguous and is never resubmitted automatically; later evidence can settle the original receipt by inspecting the already-open page, without another navigation. The sending browser is not closed while that provisional Send is still unconfirmed. A non-provisional exact batch-marker echo or an increase in visible user-message nodes is then used as page-level acceptance evidence. Composer clearing, a conversation URL change, or generation controls alone remain corroborating but insufficient. Persisted ambiguous deliveries that contain direct user-message evidence are reconciled without resending, and a validated observed `/c/...` URL is bound as the conversation target. Once CCCC invokes a submit click, an exception is recorded as `click_dispatch_unknown`; browser timing or a pre-existing running indicator cannot prove that the click did or did not dispatch, so the message is not automatically re-bundled. A pre-submit deferral is different: no submit action was attempted, so the delivery is recorded as failed and remains retryable. CCCC checks for an active response or unrelated user draft before staging the batch; it only reuses staged text when its normalized content exactly matches the current batch, and it does not treat staging as submission. CCCC keeps one single-flight owner during each visit and does not append duplicate submitting events for the same batch. Each attempt snapshots the pending direct-delivery messages that exist at that time, so a newly arrived message may be coalesced into the next batch and produce a new deterministic delivery id. After an unsent visit, the original failed batch remains eligible at the next account-wide deadline; newly arrived events join the native queue without launching extra browsers or resetting that deadline. Inbox unread state is independent throughout this process.

The embedded panel offers **Page** and **Browser** views over the same real browser session. The actor runtime panel defaults to **Page** for a larger, content-focused workspace; ChatGPT setup defaults to **Browser** because sign-in and browser UI can matter there. Switching views does not restart Chrome or change the current chat. **Browser** uses a localhost VNC projection when the session is running on a CCCC-owned Xvfb display and `x11vnc` is installed. Without that capability, **Browser** is unavailable while the isolated browser and built-in CDP page view continue to work. The VNC server binds to localhost and is intended for trusted single-user hosts or containers; remote access still goes through the authenticated CCCC WebSocket bridge. Set `CCCC_PROJECTED_BROWSER_VNC=0` to disable the Browser view while diagnosing viewer issues.

CCCC only reuses a persisted Linux Chrome/CDP process when its saved metadata proves that it belongs to a CCCC-owned Xvfb display. A legacy or externally started process without that proof is closed and rebuilt. After installing Xvfb, use **Restart ChatGPT browser** rather than only refreshing the Web page.

The login and delivery paths share this profile:

```text
CCCC_HOME/state/web_model_browser/_shared/chatgpt_web/chrome_profile
```

Enable browser delivery with:

```bash
export CCCC_WEB_MODEL_DELIVERY_MODE=browser
```

or set the connector/provider to `chatgpt_web` or `browser_web_model`.

For a browser-delivered batch, the injected prompt already contains the messages. The model should not call `cccc_runtime_wait_next_turn` first for that injected batch. It should work from the injected messages, use normal CCCC MCP tools, and call `cccc_help` if the workflow is unclear.

### Prompt and Help Layering

The browser-injected prompt should stay small. Each embedded message uses the same actor-facing format as normal peers: sender, audience, full current `event_id`, an optional short parent correlation marker, and `reply_required` only when it changes the required action. It does not repeat the canonical storage mode or full parent id. `event_id` is the value to pass to `cccc_message_reply` when answering that message. The first injected batch in a bound or newly auto-bound ChatGPT conversation also carries the normal actor system prompt plus a short Web transport note; later batches do not repeat that seed. Durable collaboration rules belong in the shared `cccc_help` path, including the Web Model Transport runtime note appended for `runtime=web_model` actors.

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

## Current Boundaries

- `web_model` does not spawn a local PTY or local headless model process.
- Connector secrets are one-time visible; CCCC stores only a hash.
- Connector activity is best-effort diagnostic state. `Settings > Global > ChatGPT Web Model` shows the latest remote method/tool, wait status, delivery or turn id, error, and last-seen time after ChatGPT calls the connector.
- Unknown or malformed tool calls return JSON-RPC protocol errors. A known tool that fails execution or policy checks returns an MCP tool result with `isError: true`; the native server includes the daemon's machine-readable `code`, `message`, and non-empty `details` in `structuredContent.error` as well as the text content.
- Only tools whose declared operation is read-only are annotated with `readOnlyHint: true`. Mixed-action and mutating tools remain unannotated so a client is not encouraged to bypass approval for a write path.
- The ChatGPT Web Model `tools/list` is intentionally stable for ChatGPT registration. Direct calls remain limited to that advertised surface; hidden built-in capability-pack tools must pass through `cccc_capability_use` and its actor-role checks.
- ChatGPT Web Model local-power tools (`cccc_repo_edit`, `cccc_shell`, `cccc_git`) are bound to the selected group’s ChatGPT Web Model actor identity and constrained to the active workspace scope.
- Local `cccc_shell`, `cccc_git`, and unified-diff `cccc_apply_patch` calls own finite commands. Timeout covers input, output, and process exit; cancelling the call ends its owned process tree. This does not undo changes already made by a command. Run persistent work in the foreground through `cccc_exec_command`, rather than leaving background children behind a completed shell.
- Shell and Git results retain at most 2,000,000 bytes per output stream and report `stdout_truncated` / `stderr_truncated`. Excess output is drained within the same command deadline instead of accumulating in memory.
- Local `cccc_exec_command` sessions belong to the CCCC host process and Home. Host shutdown ends those commands; closing a browser tab does not. Cleanup leaves Actor/Analyst sessions and other hosts alone.
- ChatGPT proactive delivery depends on the shared projected browser session and an active logged-in browser profile.
- New ChatGPT chats are supported through a saved pending target: the first successful browser delivery commits the submitted batch, then CCCC waits for ChatGPT to expose the concrete `chatgpt.com/c/...` URL before binding future deliveries to that conversation. Ordinary browser history such as `last_tab_url` is diagnostic only and is never treated as a saved target.
- GPT-5.x is selected inside ChatGPT. CCCC treats ChatGPT Web Model as one browser-delivery/runtime path, not as a separate provider per model.
- GPT Pro compatibility mode is an experimental browser-transport workaround, not a supported ChatGPT model API. It may stop working when ChatGPT changes, and local access still depends on the selected chat exposing the CCCC connector.
- ChatGPT Web Model prompt/help behavior intentionally reuses the normal CCCC agent help path; only the transport note is runtime-specific.

## References

- OpenAI Apps SDK: Connect from ChatGPT: https://developers.openai.com/apps-sdk/deploy/connect-chatgpt
- OpenAI Apps SDK: Testing and tool refresh guidance: https://developers.openai.com/apps-sdk/deploy/testing
- OpenAI Help: Developer mode and MCP apps in ChatGPT: https://help.openai.com/en/articles/12584461-developer-mode-apps-and-full-mcp-connectors-in-chatgpt-beta
