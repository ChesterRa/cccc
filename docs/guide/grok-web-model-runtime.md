# Grok Bot Web Model

The `grok_web_model` runtime connects an existing Grok Bot to a CCCC Actor through a native Webhook Routine and the CCCC remote MCP connector. There is one delivery path: CCCC wakes the Routine, the Bot claims its task, replies through MCP, and completes that task. No browser injection, CCCC-managed Grok login, model API key or separate bridge service is needed. This is separate from Grok Build CLI (`grok`) and Grok Voice.

## Setup

1. Expose CCCC through a public HTTPS address using Web Access settings or your reverse proxy. Grok must be able to reach the MCP connector.
2. Open **Settings → Global → Web Model → Grok Bot**. Create the connector and copy its full MCP URL. Add it once at [Grok Connectors](https://grok.com/connectors). The private URL is available only when created or rotated; do not publish it. Grok and ChatGPT connector credentials are separate.
3. Create and save a **Grok Bot Web Model** Actor. In its **Grok Bot** connection settings, enter an existing `https://grok.com/bot/<UUID>` URL and use the Actor editor's bottom **Save** button. Each Actor needs a different Bot URL. CCCC does not create Bots.
4. Choose **Get Routine setup instructions** and paste them into that Bot with the CCCC connector enabled. Let it create or update one native Webhook Routine named CCCC. Verify the saved handler; a chat confirmation alone is insufficient. No literal credentials belong in the handler. Setup does not claim queued work.
5. Open that Routine in the **Grok desktop app** and find **POST to** and **Key**. If Details/Routines navigation fails, open the Routine chip on its creation message. The webpage does not expose these credentials.
6. Enter both fields in CCCC and choose **Save Routine credentials**, or **Save & apply credentials** if access is enabled. You can edit while a task runs; apply after it completes or is explicitly resolved. Applying while idle preserves the Actor's enabled state without a process restart. Supported endpoints are native Bot Routine URLs at `https://api2.cursor.sh/automations/webhook/<id>` with Bearer authentication. The separate signed Grok Automations API is not interchangeable. Credentials stay private under `CCCC_HOME` and are not returned by status, copied with Groups, or written to the task ledger.
7. Start the Actor and send a CCCC message. Its pane shows the Routine configuration, pending messages, task progress and last completion. **Open Grok Bot** opens the provider page manually; it is not used for message delivery.

Global connection settings require administrator access. Users without it see an administrator-managed connection notice instead of unusable controls. ChatGPT Dots shares the status-first layout, but Grok's Routine and task credentials use their own protocol; Dots pairing codes and event subscriptions do not apply.

## Task order and completion

Webhook events carry a temporary **claim-only** credential and event ID, not the business message. Only the first successful claimant receives the task and a different execution credential. The daemon preserves ledger order, permits one active turn per Actor, and makes the next wake eligible after exact completion. A completed task's credential can only retry its original completion; it cannot acknowledge a newer task. Duplicate or out-of-order Routine runs cannot claim the same inputs twice. Nested `cccc_code_exec` tools inherit the validated execution context.

HTTP 200 means Grok accepted a wake, not that it claimed or completed the task. Transient network/429/5xx failures retry the **same wake** at most three times with 2s/4s backoff. An acknowledged wake is not automatically resent. After two minutes without a claim, **Retry wake** becomes available; first check the saved Routine, connector permissions and provider limits. Duplicate cloud runs remain possible after uncertain network delivery, but CCCC still admits only one task.

Routine runs use your existing subscription and remain subject to Grok's shared Routine limits, approval requirements and provider-side pauses. CCCC does not bypass them. See the [official Routine guide](https://cursor.com/help/grok-bot/routines).

## Stop and recover

Stopping CCCC access or pausing the Group revokes old claim/execution credentials. It does **not** cancel work already running in Grok. Accepted unfinished work remains visible and blocks new task admission rather than being silently replayed. A claimed turn means CCCC is waiting for a completion receipt, not that Grok is necessarily still processing it. If the turn cannot finish, review the Bot's work, then choose **End this turn** in **Task delivery** and confirm. Pausing the Actor first is not required. This revokes that turn's CCCC authority and preserves an existing completion or records cancellation without replaying its inputs. It does not stop Grok cloud work. Later queued messages may continue while CCCC access is enabled; rerunning the original task requires a new, explicit message after checking for earlier side effects. A slow task is not ended automatically.

Changing the Bot URL, disconnecting, removing the Actor or revoking its connector retires the associated private configuration and authority. An unchanged URL save retains its configuration. Unfinished canonical tasks remain recoverable even after connector revocation.

Existing Grok Actors without a configured Routine show **Setup required**. Browser delivery is removed; there is no mode selector or automatic browser fallback. Old unowned browser claims with an uncertain handoff are marked ambiguous rather than replayed or allowed to cause endless empty wakes. Review their original Bot history separately. Disable obsolete provider Routines yourself when they are no longer needed; CCCC does not delete cloud configuration.

## Editing and authority

Bot URL edits remain local until the Actor editor is saved. Applying a changed URL to an enabled Grok Actor asks for confirmation and updates the idle route without stopping its access. The old Routine credentials are cleared when the Bot changes; configure the new Bot's Routine before it can receive work. A stopped Actor stays stopped with **Save**; **Save & Restart** also starts it. A failed save retains the draft for retry. An unfinished task must complete or be explicitly resolved before replacing its route or Routine credentials. The daemon rechecks this at apply time, so a task claimed after the page's latest refresh is not interrupted. Switching from another runtime to Grok still stops the previous backend. Linked Profiles can supply runtime defaults, but Bot URLs and private credentials belong to individual Actors and are excluded from Profile/Group copies.

Grok's tested MCP path supplies no stable host Bot identity. Each top-level business call therefore requires the execution `actor_token` returned by the current claim. The server validates its connector, Group, Actor generation, binding revision, epoch and active task. Model-supplied selectors cannot replace it. Possession grants that Actor's authority; deliberately sharing a credential shares that authority. Never put credentials in replies, saved instructions, files or commands. `cccc_connector_status` can check connector connectivity without selecting an Actor.

Keep the successful claim result together as temporary context for that run.
Host JavaScript orchestration may pass its credential as the CCCC tool's
`actor_token` argument, including a literal argument value if needed; this does
not permit putting it in `cccc_code_exec` `source`/`code`, business code or logs.
If a variable reference is lost, the same execution may use its original claim
tool result to populate arguments. Do not claim again or use a previous run,
conversation history or saved memory to recover credentials. Discard temporary
context after confirmed completion. An unavailable original result leaves the
turn unfinished for the existing explicit recovery flow.

## Troubleshooting and validation boundary

- **Setup required:** save the Bot URL and configure its native Routine credentials; starting an unconfigured Actor does not deliver through a browser.
- **Wake accepted, no claim:** verify the saved handler and CCCC tool permissions in Grok, check provider limits, then use the explicit retry when available. Do not repeat business inputs.
- **“MCP server does not exist”:** discover the actual connector namespace with `GetDynamicTools` and use the discovered name. Do not guess a namespace or rotate credentials solely for this host discovery error.
- **Missing or invalid `actor_token`:** use the credential from the current Webhook event for its claim and the new credential from that claim for business tools. Historical conversation credentials may be revoked.
- **Interrupted task:** review the provider history and use the stopped Actor's exact resolution action. Ask for normal provider approval if required.

Feasibility probes delivered three exact Webhook events to the same Bot, including a burst, and observed a native Routine calling CCCC MCP status. Burst results arrived out of order, confirming the need for daemon-owned sequencing. Isolated local regressions cover admission, scoped nested tools, exact completion, backlog continuation, stop/recovery, configuration privacy and the native UI. These are synthetic business tasks, not live production acceptance or long-term provider reliability evidence. Linux validation does not establish native Windows/macOS behavior.
