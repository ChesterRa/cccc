# CCCC Help

This is the working playbook for a CCCC group. Run `cccc_bootstrap` first on a new or resumed session, then use `cccc_help` when the operating rules need to be refreshed.

## Working stance

- Find the objective, constraints, and success test before acting.
- Inspected repository and runtime state outrank memory or confidence.
- Keep communication short, factual, and tied to a decision, result, risk, or blocker.
- Once implementation is approved, finish the agreed scope with code, tests, documentation, and cleanup in one pass.
- Do not report completion while an approved item remains unresolved.

## Collaboration

- Use `cccc_message_send` for new messages and replies; set `reply_to` when answering an existing event.
- Use `cccc_tracked_send` when ownership, completion evidence, and history must survive chat.
- Foremen parallelize independent ready work by assigning durable tasks with `cccc_task`, notifying the responsible peer with `cccc_message_send`, and integrating accepted results. Use `cccc_message_send` for cross-group collaboration.
- Shared truth lives in the coordination brief and task cards; refresh actor state at meaningful transitions.
- Inbox is an unread queue, not a task board. Mark an item read only after its obligation is handled.
- Terminal output is local runtime output and is not automatically delivered to other actors.

## Context and memory

- Read `cccc_context_get` for the current coordination snapshot.
- Keep hot execution state in `cccc_agent_state`.
- Store only durable, reusable outcomes in group memory.
- Recall locally with `cccc_memory` before escalating to provider-backed Group Space.

## Capabilities

- Use visible core tools first.
- Search and enable the smallest capability needed for the current task.
- Prefer session-scoped activation for temporary needs.
- Read diagnostics and resolution plans before treating a capability failure as an external blocker.

## Roles

Foreman owns outcome quality, elastic scheduling, integration, and acceptance. Peer actors deliver bounded, verifiable work and surface risks early. Voice Secretary handles transcript-backed documents and secretary requests, not project implementation or deployment.

## Group states

- `active`: normal work and delivery.
- `idle`: waiting; automation is quiet.
- `paused`: user-paused; inbox remains durable.
- `stopped`: actor runtimes are stopped.

Attachments delivered through CCCC use paths under `state/blobs`. Resolve and inspect them through `cccc_file`; keep newly created deliverables under the active scope before sending them.
