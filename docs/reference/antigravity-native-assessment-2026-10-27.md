# Antigravity native-session assessment — 2026-10-27

**Decision: defer native hot attachment for both Antigravity App and CLI.**

Revisit in November–December 2026 when a newer release or documented interface
addresses the gaps below. This is a dated investigation record, not a support
promise or an implementation plan. CCCC's existing managed Antigravity adapter
is a separate integration and is not changed by this decision.

## Scope and admission standard

The investigation used Linux/WSL, official App **2.26.0** and CLI **1.4.7**,
test-owned conversations/processes, and fixed synthetic messages. Some probes
used a local model fixture; others used the existing locally authenticated
account. The final qualification reused the existing login without another
OAuth flow, logout or credential copy. Windows/macOS acceptance is not claimed.

The intended feature connects an independently running native conversation to
CCCC without replacing its process, resuming its history in another process,
injecting terminal keystrokes, or changing its account and permissions. It must:

- Identify the current native conversation and its live host reliably.
- Receive into that same conversation while idle or busy, with visible native
  input and a clear queue/interruption policy.
- Distinguish rejection before admission, actual transport admission, and an
  uncertain outcome. Uncertain or accepted input must not be automatically
  replayed.
- Preserve CCCC's source revision, Actor generation, membership access and
  Group pause/stop boundaries, including reload, exit and configuration disable.
- Install and unload through supported extension mechanisms, preserving user
  configuration and native history.

Transport admission does **not** mean that the model has read the message,
finished its task or replied. The missing boundary here is confirmation of the
input handoff, not confirmation of model execution.

## What worked

| Capability | App | CLI |
| --- | --- | --- |
| Conversation identity | MCP `antigravity.google/conversation_id` matched the corresponding Hook identity. | The same match was observed. `root_conversation_id` was also present, but is supplementary, not authority for other conversations. |
| Existing-conversation input | Official Sidecar `agentapi send-message` woke a real test conversation. | A helper launched by a model tool received native `agentapi` context and woke its original test conversation. |
| Busy input | Under both tested queue and immediate-send settings, input reached the next model invocation after an 18-second tool, before the original task stopped. It did not interrupt that tool. | Same-conversation wake was demonstrated; a complete production busy-input/lifecycle matrix was not qualified. |
| Automatic extension lifetime | An official global Sidecar hot-started inside the running test App. Disabling it stopped the Sidecar while leaving the App running. | The tested plugin did not automatically start its Sidecar; see limitations below. |
| Host cleanup | A tool-launched prototype receiver exited when its owned App host exited. | The corresponding prototype exited when its owned CLI host exited. |

The App's MCP metadata did not contain `root_conversation_id`; requiring that
field would incorrectly reject it. CLI workspace validation only performed a
direct string comparison and did not pass, so no CLI workspace qualification
is claimed.

In an isolated CLI model-request capture, the native message entered model
`contents` as **role `user`**, not `systemInstruction`. Native transcript naming
such as `SYSTEM_MESSAGE` alone is not evidence of elevated instruction priority
and does not justify another message-injection pipeline.

## Why formal support was deferred

### Send success does not establish target admission

The decisive negative control ran inside the official App Sidecar using a
random conversation UUID that did not exist:

1. `agentapi get-conversation-metadata <nonexistent-id>` failed.
2. `agentapi send-message <nonexistent-id> <fixed-synthetic-text>` exited **0**.
3. A second metadata lookup still failed.

The parsed response contained only this shape:

```json
{
  "response": {
    "sendMessage": {
      "recipientId": "<requested-id>",
      "content": "<requested-text>"
    }
  }
}
```

It echoed the request without an admission status or receipt. An isolated CLI
probe returned the same response shape and exit status for valid and invalid
targets. A separate App-to-CLI probe also returned send success even though the
App could not resolve that CLI conversation and no CLI wake was observed.

These observations do not prove that all valid sends fail. They prove that
this return value cannot distinguish a request echo from a successful target
handoff. A metadata preflight checks existence at one instant, but does not
acknowledge the subsequent send. Treating every send as ambiguous would also
leave an unusable collaboration flow.

### CLI automatic receiver startup is not qualified

The official Sidecar documentation describes App support. In the tested CLI
workspace custom-agent plugin, strict plugin validation passed and its unique
MCP tool was called successfully, but the enabled Sidecar did not start. Earlier
isolated plugin probes also did not start it. This is not a claim about every
possible global plugin installation or future CLI version.

The tested lifecycle/tool Hooks and native `!` shell mode did not receive
`agentapi` control context. Model-tool subprocesses did receive it, enabling a
prototype, but requiring the model to launch a receiver for each session is
not the intended automatic native attachment. CCCC will not extract private
credentials from unrelated processes or silently wrap the user's CLI command
to work around this gap.

### A wake–Hook–acknowledgement protocol is not justified

A fixture proved that an App Sidecar can send a neutral wake signal and a
`PreInvocation` Hook can later inject a user message. When the user stopped
the task, the payload remained unconsumed; an explicit new wake subsequently
injected it once.

That is an experiment, not a CCCC daemon/ledger acceptance result. Making it
production-ready would require handling cancellation, expired handoffs, lost
acknowledgements, duplicate or delayed wake signals, and shared configuration
without replaying uncertain input. No simple complete solution was verified.
Adding that second protocol solely to force this runtime into the supported
list has uncertain ROI, so it is not implemented.

## Lifecycle and installation findings to retain

- App archive/navigation did not trigger an end Hook or make the conversation
  unaddressable. An archived test conversation still woke on input. Metadata
  exposed no archive flag in the probe. Do not equate UI selection with CCCC
  membership authorization or scan private state to guess that selection.
- In an isolated CLI `/resume` probe, the previous conversation remained
  addressable and no immediate new `SessionStart` was observed. Any eventual
  adapter must define per-conversation lifetime separately from navigation.
- A control context was able to address another known conversation in an
  isolated cross-session probe. Possession of that context does not replace
  CCCC's source and Actor binding checks.
- The App adds `projectId` and `schemaVersion` to enabled Sidecar configuration.
  Cleanup must identify its own registration without deleting another entry
  or requiring the native-normalized value to remain byte-for-byte identical.
- Consumer-plan applicability to this exact third-party integration was not
  established. Official plugins/Sidecars exist, but neither a blanket policy
  prohibition nor blanket approval was inferred.

No Antigravity native adapter was added. Test-owned processes, profiles,
Sidecar registrations/data and temporary files were cleaned up. The three
provider-global configuration files matched their pre-test hashes; existing
native history, including synthetic test conversations, was retained. The
user's running CCCC daemon, Web and Actors were not restarted.

## Reassessment procedure

For a newer release, first check release notes and official APIs. Do not repeat
the entire investigation or require account login just because time has passed.

1. Record the current official App/CLI versions and platform. Look specifically
   for a target admission receipt, receive callback or supported in-conversation
   queue; for CLI, also look for automatic receiver startup and disposal.
2. Repeat valid-target and nonexistent-target controls. Require a verifiable
   distinction between admitted, rejected and unknown; an echoed request or
   exit code alone is insufficient.
3. If those primitives pass, validate idle/busy input, user cancellation,
   reload/new/resume/exit, source identity and configuration ownership using
   isolated fixtures and the existing login. Do not send to current user work.
4. Only then implement against CCCC's existing receiver, ledger and daemon
   authority, and validate access pause, Group stop, stale callbacks and lost
   acknowledgements. Do not add another queue or delivery authority.
5. If the key primitives are unchanged, retain this deferral. If implementation
   proceeds, update contracts, all affected ports, tests and operating guidance
   together before claiming support.

## Primary sources

- [Official Sidecars documentation](https://antigravity.google/docs/sidecars/)
- [Official Hooks documentation](https://antigravity.google/docs/hooks/)
- [Official MCP documentation](https://antigravity.google/docs/mcp/)
- [Official Remote Control documentation](https://antigravity.google/docs/remote-control/)
- [Official downloads](https://antigravity.google/download)

The behavior above is dated local probe evidence, not a promise made by those
documents. The retained sanitized evidence bundles are named
`cccc-antigravity-native-qualification-20261027`,
`cccc-antigravity-followup-20261027` and
`cccc-antigravity-decision-20261027`; their summaries are reproduced here so
this decision does not depend on temporary probe files or one workstation.
