# Grok Bot Web Model

The `grok_web_model` runtime connects a Grok Bot to a CCCC Actor using browser delivery and one remote MCP connector. Each Actor keeps its own browser window and Bot URL. Grok Actors share a dedicated Grok login; ChatGPT uses a separate profile and connector.

## Setup

1. Expose CCCC through a public HTTPS address, using the existing Web Access settings or your reverse proxy. Grok must be able to reach this address.
2. Open **Settings → Global → Web Model → Grok Bot**. Open the login window and sign in to Grok.
3. Create the Grok connector and copy its full MCP URL. Add that URL once at [Grok Connectors](https://grok.com/connectors). The credential-bearing URL is shown only when created or rotated; save it before leaving settings. Rotating Grok credentials does not rotate ChatGPT credentials.
4. Create an Actor with runtime **Grok Bot Web Model** and save it. In its **Grok Bot conversation** settings, paste its `https://grok.com/bot/<UUID>` URL and use the editor’s bottom **Save** button. Use a different Bot URL for each Actor.
5. Start the Actor and send a normal CCCC message. CCCC opens its window, waits for the composer to be available and delivers the task. There is no pairing-code message or separate handshake.

An existing Bot URL is required. CCCC does not create a Grok Bot or use the Grok homepage as an Actor conversation. An unconfigured Actor's viewer points to its **Grok Bot conversation** settings. **Bot configured** means its URL and routing credential are saved; browser readiness and delivery status are shown separately. The shared login window may still open the Grok homepage for signing in.

When changing an existing Actor to **Grok Bot Web Model**, the Bot URL field appears immediately under the runtime selector. Fill it in before saving. Bot URL edits remain local until saved, including when the Actor is running. For a running Actor, **Save & apply** asks for confirmation, stops only that Actor, saves the runtime and URL, then restarts it after success. A stopped Actor stays stopped with **Save**; **Save & Restart** also starts it. If setup fails, the dialog keeps your URL for retry and the Actor is not restarted. Other Actors are unaffected. Cancelling an unsaved URL edit leaves the current route intact. A clean editor shows **Done**; opening a browser or disconnecting is an immediate action and needs no second Save. Unconfirmed delivery or an unsent browser draft can still block applying a new route.

Runtime profiles can reuse the runtime and capability defaults. Bot URLs and credentials belong to individual Actors; profiles and copied Groups do not carry them. Local CLI commands and environment variables do not apply to this browser runtime.

Linked Profile Actors use the same Bot URL field and Save action; detaching the Profile is unnecessary. Saving a changed URL also navigates an existing idle Actor window to that Bot. If a later save step fails, the dialog preserves your draft and compares further edits with the configuration already saved, so you can retry or change back without reopening the editor.

The Actor viewer is available while Grok loads; tasks wait until the Bot composer is ready. **Close login window** in global settings closes only the login window, leaving Actor windows and saved login intact. Signing out of Grok affects all Grok Actors sharing that login.

Opening the login window does not wait for Grok to finish loading. Slow pages, network errors and site verification remain visible in the browser; an open window does not mean login or task delivery is ready. **Check login** inspects the current page without reloading it.

## How calls select an Actor

On the tested Grok MCP path, requests do not include a stable Bot identity. CCCC therefore supplies an opaque `actor_token` with each browser-delivered task. Every top-level CCCC tool call must include it; nested tools inside `cccc_code_exec` inherit the server-validated context. CCCC checks the current binding and Actor generation before dispatching a call and removes the credential from ordinary tool arguments.

This is possession-based authority: sharing an Actor credential with another Bot gives that Bot the same Actor authority. It does not prove which Grok Bot made the request. Do not publish credentials or put them in replies, files or commands. CCCC does not store the injected credential in its task ledger or export it through Group copies.

Credentials survive ordinary restarts and saving the same Bot URL. Disconnecting, changing the URL, changing the runtime/provider, removing the Actor or revoking the connector invalidates the old route. Pending, unconfirmed delivery prevents changing/removing the binding; inspect the saved conversation and resolve that delivery first.

## Delivery and recovery

CCCC preserves unrelated text and attachment drafts. It waits while the Bot is working, even if Grok still shows an enabled Submit button. Delivery is confirmed against the exact batch marker in a **user** message; an assistant echo is not a receipt. Uncertain submission pauses automatic replay to avoid duplicate work. Use the message's recovery action or the Actor conversation panel to inspect and continue.

Grok uses standard text delivery. ChatGPT's experimental GPT Pro blank-image workaround is not offered for Grok. CCCC exposes its existing native MCP text, image and file results; this does not establish that Grok can consume every format that ChatGPT can consume. No PDF, Office, audio or video converter is added.

## Troubleshooting and validation boundary

- **Tool discovery still shows only a test tool:** replace the earlier diagnostic URL with the product MCP URL from CCCC settings, then refresh Grok's connector tool catalog. The temporary probe is not the product connector.
- **“MCP server does not exist”:** ask Grok to discover the actual namespace with `GetDynamicTools` and call the discovered tool name. Do not guess a namespace or rotate CCCC credentials solely because of this host discovery error.
- **Missing or invalid `actor_token`:** use the credential in the current Bot's CCCC task. After reconnecting, start with a new CCCC task; old conversation credentials may be revoked.
- **Browser profile already in use:** close the separately launched setup browser before opening that profile through CCCC. Do not delete the profile or its login cookies.
- **Approval or site verification:** complete Grok's normal approval/sign-in flow in the Actor window. CCCC does not bypass provider safety checks.

Two real Bots each completed two isolated credential-routing calls. Local regressions cover MCP routing, nested tools, credential lifecycle, browser controls and configuration. The complete product delivery/reply loop and Grok's changing busy/approval UI still require live user acceptance; local fixtures cannot guarantee provider behavior. Native Windows/macOS behavior is not established by Linux tests.
