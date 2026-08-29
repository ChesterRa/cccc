# ChatGPT Web Model Runtime

The `web_model` runtime lets a ChatGPT web chat participate in a CCCC group through browser delivery plus a remote MCP connector. In ChatGPT sessions that expose the CCCC MCP connector, **GPT-5.x** can act as a first-class local development actor: it can receive routed CCCC messages, call CCCC MCP tools, edit the active workspace, run scoped commands, inspect git output, and report back through the same coordination layer as Codex or Claude Code. When the selected GPT-5.x chat exposes the CCCC MCP connector, ChatGPT web capacity can become additional local-development agent capacity and reduce pressure on native Codex usage for work that fits the ChatGPT Web path.

MCP availability is determined by the selected ChatGPT model and account, not by CCCC. Use a ChatGPT session that can actually see the CCCC connector for local development. CCCC also offers an experimental **GPT Pro** delivery mode for accounts where attaching an image makes the connector available to a GPT Pro chat. This is an observed ChatGPT behavior rather than a supported model-selection API, so it can stop working when ChatGPT changes. CCCC never switches the ChatGPT model for you.

There are two delivery transports behind the same actor identity:

1. **Browser delivery**: CCCC claims a pending Send or Send + Reply batch and injects it into a bound ChatGPT web chat through the shared daemon-owned projected browser session. If ChatGPT is already responding, CCCC may use the formal `Send prompt` control inside that composer, but it never treats the visible Stop control or a similarly named control elsewhere on the page as a send target. A confirmed injection records `runtime.delivery=accepted`; an indeterminate post-click result records `ambiguous`, and neither is automatically submitted again. A definite pre-submit failure records `failed` and remains eligible for a later delivery attempt.
2. **Remote-MCP pull**: ChatGPT calls `cccc_runtime_wait_next_turn` through MCP. Returning a turn records that the pull transport accepted it; `cccc_runtime_complete_turn` closes that exact active runtime turn after processing.

In both modes, transport delivery, Inbox reading, and runtime completion are separate
facts. Neither browser submission nor `cccc_runtime_complete_turn` advances the Inbox
Mail cursor; only `cccc_inbox_read` consumes Mail. The model does not make a
completion call for a browser-injected batch because the browser adapter closes that
turn. If the completion response is lost, reconciliation retries only the completion
identity and never replays the browser prompt. A mismatched active turn is rejected
instead of overwriting newer runtime state.

Mental model: the ChatGPT Web Model actor is a normal CCCC agent whose model surface happens to be ChatGPT Web. It reuses the same `cccc_bootstrap`, `cccc_help`, messaging, coordination, capability, memory, and repository tool paths as Codex/Claude actors. Browser delivery and remote-MCP pull are transport adapters, not a separate help system.

Connector model: CCCC currently supports one ChatGPT Web Model actor per CCCC instance. That actor owns one active remote MCP URL and one target ChatGPT conversation. Rotating the MCP URL creates a new secret and revokes the previous active URL.

MCP tool model: ChatGPT registers a remote MCP schema up front, so the ChatGPT Web Model connector advertises a fixed built-in schema instead of extending that schema with newly discovered capability tools. Explicitly disabling CCCC code mode removes its two code-mode tools; daemon restart timing does not collapse the remaining Web Model schema to the smaller ordinary-actor fallback. Calls are still authorized with the connector-bound actor identity. A Web Model actor cannot bypass that surface by naming an unadvertised tool directly. A group foreman can reach an enabled built-in capability-pack tool through `cccc_capability_use`; a peer cannot use that route to acquire foreman management authority.

## Requirements

- A CCCC group with an attached workspace scope.
- A running actor with runtime `ChatGPT Web Model`.
- A public HTTPS URL that reaches `cccc web`.
- A ChatGPT account with remote MCP connector support.

ChatGPT developer mode supports remote MCP over SSE or streamable HTTP and does not connect to local MCP servers. Full local development requires the selected ChatGPT conversation to expose the CCCC connector and its write-capable tools. If the selected model cannot see the CCCC connector, that chat has no CCCC local access.

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

Browser delivery is the proactive path for ChatGPT web. CCCC uses one shared daemon-owned projected Chrome/Edge browser session for settings, runtime inspection, optional manual reload, optional auto-reload recovery, and message delivery. Delivery submits CCCC message batches into the explicitly bound chat; the web model still uses the CCCC MCP connector for all visible replies and local work. Choose a GPT-5.x model/session that can see and use the CCCC connector for local execution. If the selected model cannot see MCP tools, switch to an MCP-capable GPT-5.x chat before assigning local work.

The actor runtime panel provides two durable delivery modes:

- **Standard** (default): text-only browser delivery. This is the recommended stable path.
- **GPT Pro (experimental compatibility mode)**: CCCC attaches one deterministic 32×32 blank PNG to each delivered batch before invoking Send. The image is transport-only and contributes no task context. CCCC uses the browser file input directly, never the OS clipboard, and treats attachment plus submission as one transaction. A pre-submit upload failure records a retryable failed delivery; a post-click ambiguous result is never automatically duplicated.

The setting is stored per group and actor and applies from the next accepted delivery, including after daemon restarts. It does not select Pro, change the active ChatGPT model, or guarantee that ChatGPT exposes the connector. Select the desired model in ChatGPT itself.

On native Linux, projected headed browsers require `Xvfb`. CCCC starts a private virtual display, removes inherited Wayland display markers, and forces Chrome/Edge onto X11 so the physical desktop never receives the browser window. Missing `Xvfb` is a startup error even when the host has a usable `DISPLAY`; CCCC does not silently expose the projected browser on the host desktop. Install `xvfb` with the distribution package manager, then restart the ChatGPT browser session. `cccc doctor` reports the system browser, required Xvfb isolation, and optional x11vnc viewer separately.

On macOS, the shared ChatGPT browser runs headless by default. The daemon still uses the installed system Chrome or Edge, the same persistent login profile, and the same CDP-backed **Page** projection, but it does not open or focus a separate desktop window during warmup or delivery. Sign-in and normal interaction happen through the embedded **Page** view. For temporary compatibility troubleshooting only, set `CCCC_WEB_MODEL_BROWSER_HEADLESS=0` and restart the ChatGPT browser session to restore the visible system-browser window.

The default submit timeout is 30 seconds and can be changed with `CCCC_WEB_MODEL_BROWSER_DELIVERY_TIMEOUT_SECONDS`. This is the outer delivery hard cap; slow page loads, composer waits, safe `Send prompt` discovery, and new-chat binding share that budget and may not each consume their full internal timeout. Browser startup is handled by the projected browser runtime, which requires a real system Chrome or Edge CDP-capable browser for ChatGPT. Automatic page reload recovery is disabled by default. To opt into the legacy recovery behavior for a fragile ChatGPT browser session, set `CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD=1`; the inactivity threshold is controlled by `CCCC_WEB_MODEL_BROWSER_AUTO_RELOAD_INACTIVITY_SECONDS`. After a submit action, either an exact batch-marker echo or an increase in visible ChatGPT user-message nodes is direct acceptance evidence. Composer clearing, a conversation URL change, or generation controls alone remain corroborating but insufficient. Persisted ambiguous deliveries that contain direct user-message evidence are reconciled without resending, and a validated observed `/c/...` URL is bound as the conversation target. Once CCCC invokes a submit click, an exception is recorded as `click_dispatch_unknown`; browser timing or a pre-existing running indicator cannot prove that the click did or did not dispatch, so the message is not automatically re-bundled. A pre-submit deferral is different: no submit action was attempted, so the delivery is recorded as failed and remains retryable. CCCC may stage the batch in its daemon-owned composer before discovering that no safe submit control is currently available; it only reuses staged text when its normalized content exactly matches the current batch, and it does not treat staging as submission. CCCC keeps one single-flight worker, retries with bounded backoff, and does not append duplicate submitting events for the same batch. Each attempt snapshots the pending direct-delivery messages that exist at that time, so a newly arrived message may be coalesced into the next batch and produce a new deterministic delivery id. If the retry budget is exhausted, a failed batch remains eligible for the next normal trigger; direct-delivery events that arrived after the final snapshot produce a different delivery id and receive one fresh worker after single-flight is released. Inbox unread state is independent throughout this process.

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

The browser-injected prompt should stay small. Each embedded message uses the same actor-facing format as normal peers, including its current `event_id`, canonical `message_mode`, and a distinct parent `reply_to` when present. `event_id` is the value to pass to `cccc_message_reply` when answering that message. The first injected batch in a bound or newly auto-bound ChatGPT conversation also carries the normal actor system prompt plus a short Web transport note; later batches do not repeat that seed. Durable collaboration rules belong in the shared `cccc_help` path, including the Web Model Transport runtime note appended for `runtime=web_model` actors.

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
- Unknown or malformed tool calls return JSON-RPC protocol errors. A known tool that fails execution or policy checks returns an MCP tool result with `isError: true`; the native server includes a machine-readable payload in `structuredContent` and the text content.
- Only tools whose declared operation is read-only are annotated with `readOnlyHint: true`. Mixed-action and mutating tools remain unannotated so a client is not encouraged to bypass approval for a write path.
- The ChatGPT Web Model `tools/list` is intentionally stable for ChatGPT registration. Direct calls remain limited to that advertised surface; hidden built-in capability-pack tools must pass through `cccc_capability_use` and its actor-role checks.
- ChatGPT Web Model local-power tools (`cccc_repo_edit`, `cccc_shell`, `cccc_git`) are actor-bound to the single ChatGPT Web Model actor identity and constrained to the active workspace scope.
- ChatGPT proactive delivery depends on the shared projected browser session and an active logged-in browser profile.
- New ChatGPT chats are supported through a saved pending target: the first successful browser delivery commits the submitted batch, then CCCC waits for ChatGPT to expose the concrete `chatgpt.com/c/...` URL before binding future deliveries to that conversation. Ordinary browser history such as `last_tab_url` is diagnostic only and is never treated as a saved target.
- GPT-5.x is selected inside ChatGPT. CCCC treats ChatGPT Web Model as one browser-delivery/runtime path, not as a separate provider per model.
- GPT Pro compatibility mode is an experimental browser-transport workaround, not a supported ChatGPT model API. It may stop working when ChatGPT changes, and local access still depends on the selected chat exposing the CCCC connector.
- ChatGPT Web Model prompt/help behavior intentionally reuses the normal CCCC agent help path; only the transport note is runtime-specific.

## References

- OpenAI Apps SDK: Connect from ChatGPT: https://developers.openai.com/apps-sdk/deploy/connect-chatgpt
- OpenAI Apps SDK: Testing and tool refresh guidance: https://developers.openai.com/apps-sdk/deploy/testing
- OpenAI Help: Developer mode and MCP apps in ChatGPT: https://help.openai.com/en/articles/12584461-developer-mode-apps-and-full-mcp-connectors-in-chatgpt-beta
