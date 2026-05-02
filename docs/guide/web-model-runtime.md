# ChatGPT Web Model Runtime

The `web_model` runtime lets a ChatGPT web chat participate in a CCCC group through browser delivery plus a remote MCP connector. In ChatGPT sessions that expose the CCCC MCP connector, **GPT-5.x** can act as a first-class local development actor: it can receive routed CCCC messages, call CCCC MCP tools, edit the active workspace, run scoped commands, inspect git output, and report back through the same coordination layer as Codex or Claude Code.

GPT-5.x Pro is different. Current ChatGPT platform restrictions may prevent GPT-5.x Pro from seeing or using full third-party MCP/local write tools. In CCCC, GPT-5.x Pro should be treated as an advisory/review surface for planning, architecture critique, debugging hypotheses, and implementation review based on conversation context, pasted diffs, test output, and project summaries. Local execution should remain with MCP-capable runtimes.

There are two delivery modes behind the same actor identity:

1. **Browser delivery**: CCCC injects the current unread message batch into a bound ChatGPT web chat through a CCCC-owned browser sidecar. A successful injection commits the actor cursor.
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

ChatGPT developer mode supports remote MCP over SSE or streamable HTTP and does not connect to local MCP servers. Full local development requires the selected ChatGPT conversation to expose the CCCC connector and its write-capable tools. GPT-5.x Pro may be unavailable for that path; use it for advisory/review workflows when it cannot see the connector.

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

4. Add an actor with runtime `ChatGPT Web Model`.

5. Start that actor.

6. Open `Settings > Global > ChatGPT Web Model`.

7. In `ChatGPT Web Model`, create or rotate the actor's MCP URL, then copy it into ChatGPT's custom MCP connector settings.

8. Click `Manage chat` for that actor. For an existing conversation, bind its explicit `https://chatgpt.com/c/...` URL. For a new conversation, choose `Start new chat`; CCCC will deliver the first prompt to ChatGPT and automatically bind the actor once ChatGPT creates the final `/c/...` URL.

9. Browser delivery never guesses between unrelated ChatGPT tabs. An existing chat is bound by URL; a new chat is temporarily marked pending and becomes bound only after the first delivery produces a concrete ChatGPT conversation URL.

## ChatGPT Web Setup

Use the current ChatGPT web settings for custom MCP apps/connectors:

1. Enable Developer mode in ChatGPT settings.
2. Create an app/connector for a remote MCP server.
3. Choose streaming HTTP if the UI asks for a protocol.
4. Paste the single CCCC MCP URL copied from `Settings > ChatGPT Web Model` into `MCP Server URL`.
5. Set `Authentication` to `No Auth`; the copied URL already carries the actor-bound connector token.
6. Check the custom MCP risk acknowledgement and click `Create`.
7. Open a new chat, select Developer mode/tools, and enable the CCCC connector. If CCCC was upgraded after the connector was created, refresh the app/tool list in ChatGPT settings so new tools such as `cccc_code_exec` are visible.
8. For remote-MCP pull mode, prompt the model to use CCCC explicitly:

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

### ChatGPT Browser Delivery

Browser delivery is the proactive path for ChatGPT web. The bundled sidecar controls an already logged-in Chrome/Edge profile and submits CCCC message batches into the explicitly bound chat. The web model still uses the CCCC MCP connector for all visible replies and local work. Choose a GPT-5.x model/session that can see and use the CCCC connector for local execution. If GPT-5.x Pro cannot see that connector, use it outside the runtime path for advisory review instead.

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

For Linux background delivery without showing a browser window, install `xvfb`. Browser delivery defaults to background mode when `xvfb-run` is available. You can also force it on the ChatGPT Web Model actor environment:

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

The browser-injected prompt should stay small. It identifies the actor and delivered event ids, embeds messages rendered in the same actor-facing format used by normal peers, and includes the same compact MCP reply reminder used by ordinary actors. The first injected batch in a bound or newly auto-bound ChatGPT conversation also carries the normal actor system prompt plus a short Web transport note; later batches do not repeat that seed. Durable collaboration rules belong in the shared `cccc_help` path, including the Web Model Transport runtime note appended for `runtime=web_model` actors.

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
- ChatGPT proactive delivery depends on the browser sidecar command and an active logged-in browser profile.
- New ChatGPT chats are supported through a pending auto-bind state: the first successful browser delivery must return a concrete `chatgpt.com/c/...` URL before CCCC commits the actor cursor.
- GPT-5.x is selected inside ChatGPT. CCCC treats ChatGPT Web Model as one browser-delivery/runtime path, not as a separate provider per model.
- GPT-5.x Pro currently should be documented as advisory/review support unless the ChatGPT session actually exposes the CCCC connector and write-capable tools.
- ChatGPT Web Model prompt/help behavior intentionally reuses the normal CCCC agent help path; only the transport note is runtime-specific.

## References

- OpenAI Apps SDK: Connect from ChatGPT: https://developers.openai.com/apps-sdk/deploy/connect-chatgpt
- OpenAI Apps SDK: Testing and tool refresh guidance: https://developers.openai.com/apps-sdk/deploy/testing
- OpenAI Help: Developer mode and MCP apps in ChatGPT: https://help.openai.com/en/articles/12584461-developer-mode-apps-and-full-mcp-connectors-in-chatgpt-beta
