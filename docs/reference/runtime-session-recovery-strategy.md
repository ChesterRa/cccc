# Runtime Session Recovery Strategy

How CCCC should think about recovery across multiple agent CLIs such as `codex`, `claude`, and future runtimes like `rovodev`.

## Terminology Alignment

This document follows the local glossary:

- `resume` is layered and not limited to native runtime session reuse
- `status` is an evidence-bound observation surface
- `host_surface` means a CCCC-owned readable surface exposing host/runtime truth
- `profile` means reusable actor runtime configuration and launch intent
- `attach` and `authoritative_workspace` remain distinct from actor execution details

This document is intentionally exploratory. It is not a promise that every runtime can recover its own native session. It defines the recovery layers that CCCC should preserve, the boundaries that must stay explicit, and the adapter shape future runtimes should follow.

## Why this matters

Long-running coding agents often accumulate useful working context inside their native CLI session:

- local reasoning already performed
- files already inspected
- constraints already internalized
- short-horizon implementation intent

Losing that context can slow work down.

But blindly restoring old native sessions also creates a different risk:

- the old session may still be anchored to an outdated requirement
- a new request may actually require a full solution replacement, not a gradual migration
- stale native context can silently bias the runtime toward compatibility patches when the user wants a clean redesign

So recovery is not a simple "resume whenever possible" feature. CCCC needs a recovery model that helps preserve useful context without forcing old solution assumptions into new work.

## Two different kinds of recovery

CCCC should explicitly distinguish these two layers:

### 1. Native runtime session recovery

This means recovering the runtime's own session, if the runtime supports it.

Examples:

- `codex resume <session_id>`
- a future `rovodev` resume command, if one exists
- any runtime-specific mechanism that re-enters the same native conversation/thread/session

This is vendor-specific, fragile, and often tied to undocumented local state.

### 2. CCCC work-state recovery

This means recovering the collaboration state that CCCC owns itself.

Examples:

- `cccc_bootstrap`
- `cccc_help`
- `cccc_project_info`
- `cccc_context_get`
- `cccc_task`
- `cccc_coordination`
- `cccc_agent_state`
- `cccc_memory`
- inbox / unread message continuity

This layer is runtime-agnostic and should remain the primary recovery guarantee.

## Core principle

CCCC should treat native session recovery as an enhancement, not the foundation.

The foundation should be:

1. restore CCCC-controlled work state
2. decide whether native session continuity is desirable
3. only then attempt runtime-specific native resume

This keeps CCCC compatible with multiple runtimes and avoids overfitting the architecture to one CLI's private internals.

## Current state in CCCC

Today, CCCC already has a strong generic work-state recovery path:

- `cccc_bootstrap` returns a compact cold-start / resume packet
- `cccc_help` returns role-aware operating guidance
- `cccc_context_get`, `cccc_task`, and `cccc_coordination` restore shared planning state
- `cccc_agent_state` restores actor-owned working state
- `cccc_memory` and `memory_recall_gate` restore durable context

By contrast, native session recovery is currently specialized for `codex` PTY actors.

The next planned layer is actor-scoped native resume intent:

- user config says whether an actor should prefer native resume
- runtime recovery code decides whether that request can be satisfied safely

That separation matters. "User wants this actor to try resume" is a different
fact from "CCCC discovered enough runtime evidence to perform resume now".
That actor-scoped preference may live in direct actor settings or in a linked
reusable `profile`, but either way it remains intent rather than proof.

For the concrete `codex`-first product shape, see
`Actor Native Resume Bindings`.

That specialization is useful, but it should be understood as one runtime adapter, not the universal model.

## The real risk: stale-context contamination

The main failure mode is not only "session failed to resume".

Another important failure mode is:

- requirement `A` existed
- the runtime built up a lot of local context around `A`
- later the user introduces requirement `B`
- `B` is not an incremental extension of `A`
- but the resumed native session keeps trying to gradually migrate `A` toward `B`
- the result becomes a compromise implementation the user never wanted

This is especially dangerous when:

- the core architecture is being replaced
- the data model is changing
- a previous implementation path should be abandoned completely
- the user explicitly does not want compatibility baggage

So a strong recovery design must also include a strong **non-resume** path.

## Recovery should be policy-driven

CCCC should not have only one recovery mode.

It should think in terms of recovery policies:

### Policy A: `native_resume_preferred`

Use when:

- the same requirement is continuing
- the runtime session is likely still healthy
- local native context is valuable and not misleading

Behavior:

- restore CCCC work state
- try native runtime resume
- if that fails, fall back to fresh session + CCCC recovery

### Policy B: `fresh_native_session_with_cccc_recovery`

Use when:

- the runtime session is unavailable or unreliable
- the runtime has no stable native resume mechanism
- CCCC context is more important than native continuity

Behavior:

- do not attempt native resume
- start a fresh runtime session
- inject CCCC recovery context through `cccc_bootstrap`, `cccc_help`, project info, tasks, memory, and inbox

This should be the default cross-runtime fallback.

### Policy C: `strict_replan`

Use when:

- requirement drift is high
- the old implementation path should be discarded
- the user wants a new architecture or a clean replacement

Behavior:

- do not resume native session
- restore only the minimum shared context needed to understand goals and constraints
- explicitly tell the actor to treat prior implementation direction as non-binding
- require a fresh plan / design checkpoint before implementation

This policy is important for avoiding the "A slowly migrates into B" contamination pattern.

## What should count as the source of truth

CCCC should treat these sources differently:

### First-hand session truth

The strongest source is the runtime reporting its own current session identity from inside the live session.

For example, if a runtime supports an in-session command such as `/status`, that output is the strongest direct evidence of:

- current session identifier
- current working directory
- current runtime mode

This is useful because it reflects the session the runtime itself believes is active.

### CCCC-owned recovery truth

The strongest cross-runtime recovery truth is the state CCCC already owns:

- group / actor identity
- scope attachment
- coordination
- tasks
- inbox
- memory
- actor state

This is the system-level continuity CCCC can reliably preserve across runtimes.

This class of readable CCCC-owned truth is also what the local glossary calls a
`host_surface`.

### Actor-configured recovery intent

There is also a third useful class of truth: explicit actor launch preference.

Examples:

- resume enabled for this actor
- stored `session_id` for this actor
- runtime-specific resume preference retained even while disabled

This is not the same as a runtime artifact and not the same as PTY runner
evidence. It is product configuration supplied by the user.

### Local artifact discovery

Searching local runtime files is useful, but should be treated as best-effort only.

Examples:
- runtime-specific session files
- active-presence files
- sqlite state stores
- session indexes

These may change at any time if the vendor CLI changes its private format or layout.

## Related Glossary

- [profile](/reference/glossary/profile)
- [resume](/reference/glossary/resume)
- [status](/reference/glossary/status)
- [host_surface](/reference/glossary/host_surface)
- [attach](/reference/glossary/attach)
- [authoritative_workspace](/reference/glossary/authoritative_workspace)

## Change Log

- `2026-03-21`: Added local glossary alignment so recovery strategy prose uses repo-local meanings for `resume`, `status`, `host_surface`, and workspace authority.
- `2026-03-23`: Added `profile` alignment so actor-configured recovery intent is kept separate from live runtime evidence.

So local artifact discovery should help recovery, but should not define the architecture.

## Design rule for future runtimes like `rovodev`

When adding a new runtime, CCCC should avoid cloning the `codex` recovery mechanism directly.

Instead, each runtime should answer these questions explicitly:

1. Does the runtime expose a stable native session identifier?
2. Can that identifier be queried safely from inside the session?
3. Does the runtime provide an official resume command?
4. Is there a stable local state file or API, or only private internals?
5. Can CCCC start a fresh session and still recover enough work context through MCP?

If the answer to `4` is "private internals only", CCCC should label that path as best-effort and optional.

## Proposed runtime recovery adapter contract

Future runtimes should conceptually implement an adapter with fields like:

- `runtime_id`
- `supports_native_resume`
- `supports_in_session_identity_probe`
- `identity_probe_mode`
- `identity_probe_command`
- `parse_identity_output(...)`
- `build_resume_command(...)`
- `discover_local_resume_artifacts(...)`
- `recommended_default_policy`
- `supports_strict_replan_hint`

This keeps the common architecture stable while allowing runtime-specific behavior to vary.

## Why actor-configured intent should survive disabled state

If a user temporarily disables native resume for an actor, CCCC should not have
to forget the session identifier they entered.

Otherwise the product forces users into an awkward cycle:

1. disable resume to test a clean launch
2. later re-enable resume
3. manually re-enter the same session identifier again

That is avoidable friction. A better model is:

- `enabled` controls launch behavior now
- `session_id` remains stored until the user clears it explicitly

This is especially useful for long-running projects where the same native
session may be valuable over multiple days of work.

## Safe use of manual session reporting

If a runtime supports a safe in-session status command, CCCC should prefer this flow:

1. `foreman` asks peers to enter their own runtime terminal
2. each peer manually runs the runtime's self-status command
3. each peer reports back through `cccc_message_reply`
4. CCCC records or summarizes the reported session identity

Important:

- do not inject the command automatically into a live PTY unless the runtime explicitly guarantees that doing so is safe
- terminal output is not delivery; the peer must still report through CCCC messaging

This pattern is more robust than trying to infer live session identity only from local files.

## Do not overfit recovery to native sessions

A future runtime may have:

- no official resume command
- no stable session file
- unstable or changing hook behavior
- no queryable local session database

CCCC should still work well in that case.

The minimum supported path should always remain:

- start a fresh runtime process
- run `cccc_bootstrap`
- restore project and coordination state
- recover memory selectively
- continue from CCCC-owned context

That gives CCCC a viable multi-runtime story even when native vendor session recovery is weak.

## Recommended implementation order

### Phase 1: make CCCC recovery first-class

Prioritize:

- clearer resume guidance around `cccc_bootstrap`
- better summary of recovered tasks / blockers / focus
- better role-note and help refresh flow
- better memory recall gating
- better review of unread obligations after restart

This benefits every runtime.

### Phase 2: make session inventory explicit

CCCC should surface a structured inventory that distinguishes:

- `runtime_reported_session_id`
- `cccc_persisted_session_id`
- `local_artifact_session_id`
- `native_resume_available`
- `recommended_recovery_policy`

This avoids pretending all evidence sources have the same trust level.

### Phase 3: add runtime-specific adapters

For each runtime such as `codex`, `claude`, or `rovodev`:

- implement only the adapter pieces that the runtime can support safely
- do not force unsupported runtimes into fake parity
- prefer honest degradation over brittle heuristics

## Recommended stance for `codex` today

For `codex`, the practical stance should be:

- first-hand in-session identity is valuable
- `session_id` is worth capturing
- local file discovery can help
- `session_log_path` discovery is best-effort, not a guaranteed interface
- `CCCC` recovery must still work when native resume artifacts are incomplete

And for the next product step:

- actor-bound `session_id` input is a reasonable user-facing primitive
- group-template flows should accept actor resume bindings outside the template
  file itself
- actor settings should allow disabling native resume without deleting the saved
  `session_id`

So `codex` should be treated as:

- **good candidate for native resume**
- **not the template that every future runtime must imitate**

## Recommended stance for future runtimes

For future runtimes such as `rovodev`, CCCC should assume:

- native session recovery may be unavailable, incomplete, or unstable at first
- work-state recovery must still be complete enough to continue useful work
- strict replan must be easy when the user wants a true replacement instead of an incremental migration

This gives CCCC a path that is both practical and honest.

## Working conclusion

CCCC should not define recovery as:

> "restore the exact native vendor session for every runtime"

CCCC should define recovery as:

> "restore collaboration continuity reliably, then use native runtime resume only when it is safe, valuable, and supported"

That definition is much more compatible with:

- multiple runtimes
- evolving vendor CLIs
- future runtimes like `rovodev`
- users who sometimes want continuity
- users who sometimes want a clean break from stale implementation context
