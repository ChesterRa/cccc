# ChatGPT Web Model Runtime

The `web_model` runtime lets a ChatGPT web chat participate in a CCCC group through browser delivery plus a remote MCP connector. In ChatGPT sessions that expose the CCCC MCP connector, **GPT-5.x** can act as a first-class local development actor: it can receive routed CCCC messages, call CCCC MCP tools, edit the active workspace, run scoped commands, inspect git output, and report back through the same coordination layer as Codex or Claude Code. When the selected GPT-5.x chat exposes the CCCC MCP connector, ChatGPT web capacity can become additional local-development agent capacity and reduce pressure on native Codex usage for work that fits the ChatGPT Web path.

GPT-5.x Pro is different. Current ChatGPT platform restrictions mean GPT-5.x Pro should not be treated as a CCCC local-development runtime: it cannot use the CCCC MCP connector, and its web fetcher may block public/private tunnel URLs before they reach CCCC. In practice this means no reliable local access: no CCCC MCP tools, no repository reads, no shell/git work, and no No-MCP resource fallback. Use a GPT-5.x ChatGPT session that can see the CCCC connector for local development; use Pro only as an external advisory model when you manually provide the needed context.

There are two delivery modes behind the same actor identity:

1. **Browser delivery**: CCCC injects the current unread message batch into a bound ChatGPT web chat through the shared daemon-owned projected browser session. A successful injection commits the actor cursor. A failed injection is recorded as a durable delivery failure for that source message and is not automatically bundled into later deliveries; retry by sending a new message or using an explicit retry action when available.
2. **Remote-MCP pull**: ChatGPT calls `cccc_runtime_wait_next_turn` through MCP and receives a pull-mode turn. Pull mode advances the cursor on `cccc_runtime_complete_turn`.

In both modes, the model uses CCCC tools for visible replies and workspace work. Browser delivery does not depend on a completion call; `cccc_runtime_complete_turn` remains useful for remote-MCP pull and optional evidence, but it is not a browser-delivery gate.

Mental model: the ChatGPT Web Model actor is a normal CCCC agent whose model surface happens to be ChatGPT Web. It reuses the same `cccc_bootstrap`, `cccc_help`, messaging, coordination, capability, memory, and repository tool paths as Codex/Claude actors. Browser delivery and remote-MCP pull are transport adapters, not a separate help system.

Connector model: CCCC currently supports one ChatGPT Web Model actor per CCCC instance. That actor owns one active remote MCP URL and one target ChatGPT conversation. Rotating the MCP URL creates a new secret and revokes the previous active URL.

MCP tool model: ChatGPT registers a remote MCP schema up front, so the ChatGPT Web Model connector advertises a stable built-in CCCC tool schema instead of a role-filtered progressive list. Calls are still authorized with the bound actor identity. The ChatGPT Web Model peer can use the normal peer surface plus local workspace tools (`cccc_repo_edit`, `cccc_shell`, `cccc_git`); control, diagnostics, capability administration, and other management tools require the actor to be the group foreman.

## Requirements

- A CCCC group with an attached workspace scope.
- A running actor with runtime `ChatGPT Web Model`.
- A public HTTPS URL that reaches `cccc web`.
- A ChatGPT account with remote MCP connector support.

ChatGPT developer mode supports remote MCP over SSE or streamable HTTP and does not connect to local MCP servers. Full local development requires the selected ChatGPT conversation to expose the CCCC connector and its write-capable tools. If the selected model cannot see the CCCC connector, that chat has no CCCC local access.

## Zero-to-ready setup

Follow this order. The CCCC settings page shows the same milestones, but this checklist is the full beginner path.

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

Open `Settings > Global > ChatGPT Web Model`. If no actor exists, use `Create actor` there. This creates the single CCCC actor identity that ChatGPT will use.

Start the actor if it is stopped. Then create the MCP URL and copy it. If the page says the URL is local-only or not HTTPS, return to `Web Access` and fix the public Web URL before continuing.

### 3. Create the ChatGPT MCP app

In ChatGPT, open `Settings > Apps > Advanced settings > Create app`. ChatGPT menu names may vary by plan and workspace. If this exact path is not available, look for Apps or Connectors settings, enable Developer Mode if required, then create a custom MCP app/connector. Use these fields:

```text
Name: CCCC
Description: CCCC local workspace connector
MCP Server URL: paste the full CCCC MCP URL copied from Settings > ChatGPT Web Model
Authentication: No Auth
```

Check the custom MCP risk acknowledgement and click `Create`.

Open a GPT-5.x chat, select Developer mode/tools, and enable the CCCC connector. If CCCC was upgraded after the connector was created, refresh the app/tool list in ChatGPT settings so new tools such as `cccc_code_exec` are visible.

### 4. Sign in and choose the delivery target

Back in `Settings > Global > ChatGPT Web Model`, click `Set up ChatGPT` or `Open ChatGPT`. Sign in through the embedded browser. CCCC reuses that browser profile for delivery.

For an existing conversation, open it in the embedded browser and click `Use current ChatGPT chat`. For a new conversation, click `Start a new ChatGPT chat`; CCCC will deliver the first prompt to ChatGPT and automatically bind the actor once ChatGPT creates the final `/c/...` URL.

Browser delivery never guesses between unrelated ChatGPT tabs. An existing chat is bound by URL; a new chat is temporarily marked pending and becomes bound only after the first delivery produces a concrete ChatGPT conversation URL.

### 5. Run a smoke test

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
   read-only workspace inspection, cccc_repo_edit or cccc_apply_patch for edits,
   cccc_exec_command/cccc_write_stdin for commands/tests, cccc_git for
   status/diff/add/commit, cccc_message_send for visible replies, then
   cccc_runtime_complete_turn.
   Do not use built-in browsing or unrelated tools for CCCC work.
   ```

## Common setup blockers

- **MCP URL is localhost or HTTP**: ChatGPT cannot reach local URLs. Set a public HTTPS URL in `Settings > Global > Web Access`, then rotate/copy the MCP URL again.
- **ChatGPT cannot see the CCCC connector**: use a GPT-5.x chat with Developer mode/tools enabled. GPT-5.x Pro does not expose the CCCC MCP connector for local development.
- **CCCC still says the MCP app is not connected**: after creating the app in ChatGPT, ask the model to call `cccc_bootstrap` once, or refresh the app/tool list in ChatGPT settings.
- **ChatGPT is signed in but CCCC has not confirmed it**: open the embedded browser in `Settings > Global > ChatGPT Web Model` and use `Check status` if needed.
- **Messages go to the wrong ChatGPT chat**: bind the explicit `chatgpt.com/c/...` conversation, or choose `Start a new ChatGPT chat` before sending work.

### ChatGPT Browser Delivery

Browser delivery is the proactive path for ChatGPT web. CCCC uses one shared daemon-owned projected Chrome/Edge browser session for settings, runtime inspection, tool-confirm approval, auto-reload, and message delivery. Delivery submits CCCC message batches into the explicitly bound chat; the web model still uses the CCCC MCP connector for all visible replies and local work. Choose a GPT-5.x model/session that can see and use the CCCC connector for local execution. If the selected model cannot see MCP tools, switch to an MCP-capable GPT-5.x chat before assigning local work.

The default submit timeout is 30 seconds and can be changed with `CCCC_WEB_MODEL_BROWSER_DELIVERY_TIMEOUT_SECONDS`. This is the outer delivery hard cap; slow page loads, composer waits, and new-chat binding share that budget and may not each consume their full internal timeout. Browser startup is handled by the projected browser runtime, which requires a real system Chrome or Edge CDP-capable browser for ChatGPT.

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

The browser-injected prompt should stay small. It identifies the actor and delivered event ids, embeds messages rendered in the same actor-facing format used by normal peers, and includes the same compact MCP reply reminder used by ordinary actors. The first injected batch in a bound or newly auto-bound ChatGPT conversation also carries the normal actor system prompt plus a short Web transport note; later batches do not repeat that seed. Durable collaboration rules belong in the shared `cccc_help` path, including the Web Model Transport runtime note appended for `runtime=web_model` actors.

Use this split to avoid duplicate or drifting instructions:

- Shared agent behavior: `cccc_bootstrap`, `cccc_help`, role notes, capability state, context, memory, and messaging rules.
- Web transport behavior: do not pull a browser-injected batch again; do pull when operating in remote-MCP mode without an injected batch; visible communication must use CCCC MCP tools; browser delivery commits on successful injection rather than completion. Failed browser deliveries stay visible as failed and are not automatically redelivered in a later batch.

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
- `cccc_repo` is read-only and annotated as read-only for MCP clients.
- The ChatGPT Web Model `tools/list` is intentionally stable for ChatGPT registration. Seeing a management tool in ChatGPT does not grant permission; role checks happen on `tools/call`.
- ChatGPT Web Model local-power tools (`cccc_repo_edit`, `cccc_shell`, `cccc_git`) are actor-bound to the single ChatGPT Web Model actor identity and constrained to the active workspace scope.
- ChatGPT proactive delivery depends on the shared projected browser session and an active logged-in browser profile.
- New ChatGPT chats are supported through a pending auto-bind state: the first successful browser delivery commits the submitted batch, then CCCC waits for ChatGPT to expose the concrete `chatgpt.com/c/...` URL before binding future deliveries to that conversation.
- GPT-5.x is selected inside ChatGPT. CCCC treats ChatGPT Web Model as one browser-delivery/runtime path, not as a separate provider per model.
- GPT-5.x Pro currently has no reliable CCCC local access. Do not document it as a local-development runtime or No-MCP fallback path.
- ChatGPT Web Model prompt/help behavior intentionally reuses the normal CCCC agent help path; only the transport note is runtime-specific.

## References

- OpenAI Apps SDK: Connect from ChatGPT: https://developers.openai.com/apps-sdk/deploy/connect-chatgpt
- OpenAI Apps SDK: Testing and tool refresh guidance: https://developers.openai.com/apps-sdk/deploy/testing
- OpenAI Help: Developer mode and MCP apps in ChatGPT: https://help.openai.com/en/articles/12584461-developer-mode-apps-and-full-mcp-connectors-in-chatgpt-beta
