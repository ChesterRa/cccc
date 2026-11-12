# CCCC Help

CCCC routes and shared-state reference, including the peer collaboration contract.

## Core Routes

- Resume with `cccc_bootstrap`.
- Discover other instances and connected Groups with `cccc_connect`; it is a core tool, not a capability to install.
- Reply with `cccc_message_reply`; start with `cccc_message_send`. Terminal output is not delivered.
- Target either `user` alone or one/more agents; never mix domains. Mail is agent-only, so send separate messages to humans.
- Promote/retry with `cccc_message_deliver`; a new claim wakes a paused/stopped Group; confirm `ambiguous` retries.
- `cccc_inbox_read` consumes Mail; `cccc_message_history` inspects messages without changing Mail state.
- Read shared truth with `cccc_context_get(detail="overview")` and open tasks with `cccc_task()`; reserve `cccc_agent_state` for cross-turn recovery.
- Invoke known hidden tools with `cccc_capability_use`; search only when unknown.

If the client blocks a tool call, report the actual tool name and error. A host refusal is not evidence that CCCC executed the operation. Await user guidance before retrying that operation, including through a different tool.

## Canonical Message Delivery

This section is authoritative; group guidance is additive.

- Do not send information that cannot change work. Use `mail` when delay causes no concrete loss; it is non-interrupting and may cause one content-free notice.
- Use `send` when delay would block, misdirect, or miss a window; delivery is best-effort.
- Use `request_reply` only for a concrete reply from explicit agent(s) or `user`, never broadcast.
- When the user's decision or information is needed, send `request_reply` to `user` alone with a `question`, unless the user asked for another channel: 2-4 exclusive options with their consequences when there are alternatives to weigh, or no options when you need a value only the user knows (a path, a number); add a recommendation only when you know enough. Ask only for what you cannot responsibly decide yourself (scope, irreversible or outward actions, spending, real product tradeoffs, conflicting instructions); keep working on what does not depend on the answer, and cancel the request when it is no longer needed.
- Use `cccc_tracked_send` for durable execution/evidence and `cccc_message_reply` for an existing event.

The sender chooses the mode; insight does not change it. Broadcast Mail has no
active notice. Promote rather than duplicate an event. Storage, Mail read,
runtime acceptance, reply, and task completion are separate facts;
per-recipient runtime truth is `runtime.delivery`.

## Collaboration State

### Chat

- Targets are `@all`, `@foreman`, `@peers`, `user`, or actor IDs. Before replying, verify `event_id`; before any message, verify `to`. Avoid broad targets for narrow updates.
- `@chet`, alone, addresses Chet, the user's global assistant, with Send or Request reply, not Mail. A member whose ID or title is `chet` makes it ambiguous, and the send is refused. To reply to a message from Chet, omit `to`.
- Reply to the current message with its `event_id`; `reply_to` is its optional parent.
- Use a tracked task, not a reply request, for durable execution and evidence.

### Other Instances and Connected Groups

The same tools serve same-account instances, cross-member Group connections,
and standalone Direct connections. No Web access token is needed by the Actor.

1. Call `cccc_connect()` in your local Group. Inspect both `instances` and `external_groups`.
2. For a same-account instance, call `cccc_connect(instance_id="...")` to list Groups and Actors; follow `next` with `after` when present. For an `external_groups` entry, pass both its `instance.instance_id` and `group_id` as `instance_id` and `target_group_id`.
3. Start a conversation with `cccc_message_send(dst_instance_id="...", dst_group_id="...", to="@foreman", text="...", insight="...", mode="send")`. Use the target's concrete Actor IDs for a specific recipient. Use `mail` for work that can wait, `send` for an immediate exchange, or `request_reply` when a concrete reply is needed. Include the peer insight required by the collaboration contract.
4. Reply with `cccc_message_reply(event_id="received-local-event-id", text="...", insight="...")`; omit `to` to return to the original participants. Do not substitute the source instance's Event ID. Files use `cccc_file_send(path="...", dst_instance_id="...", dst_group_id="...", to="...")`.

Keep `group_id` as your local working Group. Remote Group IDs are qualified by
instance; `cccc_group` and `cccc_actor` are not remote administration tools.
Catalogs are cached metadata, not proof of connectivity. `catalog=null` means
Actors are not known yet; `fresh=false` means the metadata is stale. A queued
message has not yet been confirmed by the destination; a receipt does not prove
Actor execution or task completion. Reuse an unchanged `idempotency_key` after
an uncertain send, rather than creating another message.

### Shared Context

- The daemon and append-only group ledger are the source of truth.
- `cccc_context_get` reads the current brief, tasks, handoffs, and actor state; `detail="overview"` leaves the task list out.
- `cccc_task()` lists open tasks one row each; list a task by `task_id` for its notes, checklist and outcome before updating them.
- Coordination and task tools are direct; project tools are on demand. Do not mirror every local todo.

### State Layers

- `coordination.brief` and shared tasks hold durable truth; keep runtime-local todo private.
- `cccc_agent_state` is per-actor recovery state, not chat status. Refresh it only at real transitions.
- Keep unfinished work in `open_loops`, promises in `commitments`, and colder context out until needed.

### Durable Coordination

- `cccc_coordination` holds shared objectives, constraints, decisions, and handoffs.
- Use `cccc_task` for user requests and commitments that need multiple steps, coordination or follow-up; `cccc_tracked_send` adds owner, scope, done criteria, and evidence. Use `cccc_backlog` for an observed issue or a user's idea deferred and worth keeping: reuse a matching entry, record why it waits and when to revisit; recording it does not approve the work. If the Group has no backlog file yet, propose one to the user instead of creating it. Otherwise reply/send.
- Task lifecycle uses `move`; use `update` only when changing other task fields too.
- Record what a task waits on in `waiting_on`: `user` when its next step needs the user (hand over the action in a message; the mark notifies no one), `external` when it waits outside the Group; set it back, usually to `none`, when the wait ends.
- A coordination interrupt is not automatically a task switch; resume the recorded task unless priority actually changed; do not replace active state with the interrupt itself.

### Recovery

- `cccc_bootstrap` returns recovery state, tasks, recent decisions, and a Mail preview.
- It is a snapshot, not proof of current external state; verify at the execution boundary.

### Inbox

- Inbox is the unread Mail queue, not chat history or a task board; bootstrap shows a preview.
- Mail does not prompt immediately; one later notice may request an inbox read. Send and Send + Reply never enter this queue.
- Answer `request_reply` with `cccc_message_reply` and its source event id unless cancelled. Send is the reply default; Mail is valid only for an agent when delay is harmless. Either fulfills the request.

### Files

- Read text, images, PDF and PPTX attachments with `cccc_file(action="read", rel_path="...")`; `info` and `blob_path` return metadata only.
- Send deliverables with `cccc_file_send(...)`; local paths alone are not delivered.

## Capabilities

- `cccc_capability_use(tool_name="...", tool_arguments={...})` invokes hidden tools without exposing the full pack.
- Hidden tools include `cccc_tracked_send`; pass their arguments in `tool_arguments`.
- `cccc_capability_use(capability_id="skill:...")` returns that skill's rules in `capsule_text`, ready to follow.
- Use `cccc_capability_search` only when the capability or tool name is unknown.
- When a capability that adds tools reports `activation_pending` or `refresh_required=true`, relist or reconnect. On failure, inspect `diagnostics` and `resolution_plan`.
- State, enablement, installation, cleanup, and governance are on demand; expose them only when needed.

## Actor Notes

Role and actor sections below are additive overlays selected by `cccc_help`.

## @role: foreman

- Do not become the group's only thinking center. Make room for peers to think with one another before open judgments harden into assignments, then integrate what the team actually learned.
- Own integration and acceptance; a peer report is evidence to inspect, not closure by itself.
- Keep outcome, acceptance basis, and owner explicit for durable delegated work. If evidence is insufficient, choose a concrete control action: continue, request evidence, hand off, or block.
- Use durable tasks or tracked sends only when owner, scope, done criteria, and evidence must survive chat.
- Own the team's fit as well as its output. On assigning work, a block or a handoff, check who owns what and whether each member's runtime, model and effort suit that work. Adjust tasks within your authority; an Actor tool's existence is not the user's consent to interrupt a session or spend more. Take the rest to the user privately.
- Actor lifecycle, runtime, capability administration, and detailed diagnostics are on-demand tools.

## @role: peer

- Act as a thinking colleague. When another independent mind could change an unsettled decision, initiate the discussion before it hardens into a handoff; contribute your own judgment rather than only status or compliance.
- Surface useful evidence, risks, and better routes directly.
- When a teammate or the team setup materially helps or hurts the work, tell the user privately, without going through the foreman: what you saw, its effect, what else could explain it, and your suggestion. Do this for good work too.
- For task-linked work, claim or update the durable task, keep `active_task_id` accurate, and report evidence plus residual risk; keep quick solo work lightweight.
- Request a handoff instead of assigning peers as authority.

## Appendix

### Group State

| State | Meaning | Automation | Delivery to PTY |
| --- | --- | --- | --- |
| `active` | normal work | configured policy | chat + notifications |
| `idle` | waiting or done for now | configured policy, except scheduled `standup` runs | chat + notifications |
| `paused` | user paused group | disabled | no runtime delivery; direct work stays pending |
| `stopped` | runtimes stopped | disabled | no actor runtime delivery |

### Permissions

| Action | user | foreman | peer |
| --- | --- | --- | --- |
| actor_add | yes | yes | no |
| actor_start | yes | yes (any) | no |
| actor_stop | yes | yes (any) | yes (self) |
| actor_restart | yes | yes (any) | yes (any) |
| actor_remove | yes | yes (peer only) | yes (self) |

The foreman cannot remove itself. The user can remove it; the next visible
Actor then becomes foreman, or the Group has no foreman if none remain.
