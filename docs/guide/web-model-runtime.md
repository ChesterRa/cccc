# Web Model Runtime

The `web_model` runtime lets a ChatGPT web chat participate in a CCCC group through browser delivery plus a remote MCP connector.

There are two delivery modes behind the same actor identity:

1. **Browser delivery**: CCCC injects the current unread message batch into a bound ChatGPT web chat through a CCCC-owned browser sidecar. A successful injection commits the actor cursor.
2. **Remote-MCP pull**: ChatGPT calls `cccc_runtime_wait_next_turn` through MCP and receives a pull-mode turn. Pull mode advances the cursor on `cccc_runtime_complete_turn`.

In both modes, the model uses CCCC tools for visible replies and workspace work. Browser delivery does not depend on a completion call; `cccc_runtime_complete_turn` remains useful for remote-MCP pull and optional evidence, but it is not a browser-delivery gate.

Mental model: a Web Model actor is a normal CCCC agent whose model surface happens to be a website. It reuses the same `cccc_bootstrap`, `cccc_help`, messaging, coordination, capability, memory, and repository tool paths as Codex/Claude actors. Browser delivery and remote-MCP pull are transport adapters, not a separate help system.

Connector model: a connector is an actor-bound remote credential, not a global shared pool. One active connector represents one `group_id + actor_id` identity. Create another Web Model actor for another browser chat/model. Rotating an actor connector creates a new secret and revokes the actor's previous active connector.

MCP tool model: ChatGPT registers a remote MCP schema up front, so Web Model connectors advertise a stable built-in CCCC tool schema instead of a role-filtered progressive list. Calls are still authorized with the bound actor identity. A Web Model peer can use the normal peer surface plus local workspace tools (`cccc_repo_edit`, `cccc_shell`, `cccc_git`); control, diagnostics, capability administration, and other management tools require the Web Model actor to be the group foreman.

## Requirements

- A CCCC group with an attached workspace scope.
- A running actor with runtime `Browser Web Model`.
- A public HTTPS URL that reaches `cccc web`.
- A ChatGPT account with remote MCP connector support.

ChatGPT developer mode supports remote MCP over SSE or streamable HTTP and does not connect to local MCP servers. GPT Pro is not a separate CCCC runtime or provider; select it inside the bound ChatGPT conversation when that model is desired.

## CCCC Setup

1. Start CCCC:

   ```bash
   cccc daemon start
   cccc web --port 8848
   ```

2. Expose Web through a public HTTPS tunnel or reverse proxy.

3. In CCCC Web, open `Settings > Global > Web Access` and set the public Web URL, for example:

   ```text
   https://cccc.example.com/ui/
   ```

4. Add an actor with runtime `Browser Web Model`.

5. Start that actor.

6. Open `Settings > Global > Web Models`.

7. In the `Web Model actors` list, create or rotate the actor's connector, then copy its MCP URL into the website connector settings.

8. Click `Manage chat` for that actor and bind an explicit `https://chatgpt.com/c/...` conversation URL. Pasting the URL is the normal path; the embedded browser is only needed for sign-in or to bind the currently visible chat.

9. Browser delivery requires this explicit bound target chat. CCCC will not guess the first open ChatGPT tab.

## ChatGPT Web Setup

Use the current ChatGPT web settings for custom MCP apps/connectors:

1. Enable Developer mode in ChatGPT settings.
2. Create an app/connector for a remote MCP server.
3. Choose streaming HTTP if the UI asks for a protocol.
4. Paste the single CCCC MCP URL copied from `Settings > Web Models` into `MCP Server URL`.
5. Set `Authentication` to `No Auth`; the copied URL already carries the actor-bound connector token.
6. Check the custom MCP risk acknowledgement and click `Create`.
7. Open a new chat, select Developer mode/tools, and enable the CCCC connector.
8. For remote-MCP pull mode, prompt the model to use CCCC explicitly:

   ```text
   Use the CCCC connector. First call cccc_runtime_wait_next_turn.
   Use cccc_repo for read-only workspace inspection, cccc_repo_edit for edits,
   cccc_shell for local commands/tests, cccc_git for git status/diff/add/commit,
   cccc_message_send for visible replies, then cccc_runtime_complete_turn.
   Do not use built-in browsing or unrelated tools for CCCC work.
   ```

### ChatGPT Browser Delivery

Browser delivery is the proactive path for ChatGPT web. The bundled sidecar controls an already logged-in Chrome/Edge profile and submits CCCC message batches into the explicitly bound chat. The web model still uses the CCCC MCP connector for all visible replies and local work. If the ChatGPT account has GPT Pro available, choose it in that same ChatGPT conversation.

The default sidecar command is bundled as:

```bash
cccc-web-model-browser-sidecar
```

Override it only when testing a custom sidecar:

```bash
export CCCC_WEB_MODEL_BROWSER_DELIVERY_COMMAND="/path/to/custom-sidecar"
```

The daemon passes one JSON payload on stdin and expects JSON on stdout. The default submit timeout is 120 seconds and can be changed with `CCCC_WEB_MODEL_BROWSER_DELIVERY_TIMEOUT_SECONDS`. The sidecar looks for Chrome or Edge automatically; set `CCCC_WEB_MODEL_BROWSER_BINARY` if the browser is in a custom location.

The login and delivery paths share this profile:

```text
CCCC_HOME/state/web_model_browser/<group_id>/<actor_id>/chrome_profile
```

For Linux background delivery without showing a browser window, install `xvfb`. Browser delivery defaults to background mode when `xvfb-run` is available. You can also force it on the Web Model actor environment:

```bash
CCCC_WEB_MODEL_BROWSER_VISIBILITY=background
```

This runs normal Chrome in a virtual display. True Chrome headless remains an explicit experimental opt-in:

```bash
CCCC_WEB_MODEL_BROWSER_VISIBILITY=headless
```

Then choose one of these opt-ins:

```bash
export CCCC_WEB_MODEL_DELIVERY_MODE=browser
```

or set the connector/provider to `chatgpt_web` or `browser_web_model`.

For a browser-delivered batch, the injected prompt already contains the messages. The model should not call `cccc_runtime_wait_next_turn` first for that injected batch. It should work from the injected messages, use normal CCCC MCP tools, and call `cccc_help` if the workflow is unclear.

### Prompt and Help Layering

The browser-injected prompt should stay small. It identifies the actor and delivered event ids, embeds messages rendered in the same actor-facing format used by normal peers, and includes the same compact MCP reply reminder used by ordinary actors. The first injected batch in a bound ChatGPT conversation also carries the normal actor system prompt plus a short Web transport note; later batches do not repeat that seed. Durable collaboration rules belong in the shared `cccc_help` path, including the Web Model Transport runtime note appended for `runtime=web_model` actors.

Use this split to avoid duplicate or drifting instructions:

- Shared agent behavior: `cccc_bootstrap`, `cccc_help`, role notes, capability state, context, memory, and messaging rules.
- Web transport behavior: do not pull a browser-injected batch again; do pull when operating in remote-MCP mode without an injected batch; visible communication must use CCCC MCP tools; browser delivery commits on successful injection rather than completion.

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
- `cccc_repo`
- `cccc_repo_edit`
- `cccc_shell`
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
- Connector activity is best-effort diagnostic state. `Settings > Global > Web Models` shows the latest remote method/tool, wait status, delivery or turn id, error, and last-seen time after the browser model calls the connector.
- `cccc_repo` is read-only and annotated as read-only for MCP clients.
- Web Model `tools/list` is intentionally stable for ChatGPT registration. Seeing a management tool in ChatGPT does not grant permission; role checks happen on `tools/call`.
- Web Model local-power tools (`cccc_repo_edit`, `cccc_shell`, `cccc_git`) are actor-bound to the connector's group/actor identity and constrained to the active workspace scope.
- ChatGPT proactive delivery depends on the browser sidecar command and an active logged-in browser profile.
- GPT Pro is selected inside ChatGPT. CCCC treats it as the same ChatGPT browser-delivery runtime path, not as a separate provider.
- Web Model prompt/help behavior intentionally reuses the normal CCCC agent help path; only the transport note is runtime-specific.

## References

- OpenAI Apps SDK: Connect from ChatGPT: https://developers.openai.com/apps-sdk/deploy/connect-chatgpt
- OpenAI Apps SDK: Testing and tool refresh guidance: https://developers.openai.com/apps-sdk/deploy/testing
- OpenAI Help: Developer mode and MCP apps in ChatGPT: https://help.openai.com/en/articles/12584461-developer-mode-apps-and-full-mcp-connectors-in-chatgpt-beta
