# Actor Native Resume Bindings

Reference for actor-scoped native session recovery bindings in CCCC.

The `codex` PTY path described here is now shipped and validated in Web UI.
Some of the broader runtime-agnostic material still serves as design guidance
for future runtimes, but the actor-bound `codex` resume flow is no longer only
proposed behavior.

## Goal

Allow a user to say:

- this actor should launch by resuming a known native runtime session
- this actor should remember the session identifier even when resume is
  temporarily disabled

The first concrete target is `codex` PTY actors.

## Terminology Alignment

This document follows the local glossary:

- `attach` sets the group's `authoritative_workspace`
- actor runtime `cwd` should be read as `execution_workspace`
- `profile` means reusable actor runtime configuration and launch intent
- `resume` is layered and is not only native session reuse
- `status` is evidence, not automatic proof of later resume success

## Primary use case

A user already worked in a project directly with Codex CLI and has valuable
session context under a known `session_id`.

Later, the user opens CCCC, creates a new group from a template, attaches the
same project path, and wants one or more actors to inherit that native session
continuity while still keeping CCCC's own coordination flow.

In glossary terms, the attached project path remains the authoritative
workspace even if later runtime evidence points to a more specific actor
execution workspace.
Likewise, a stored actor or profile-native resume binding remains launch intent
until live runtime evidence confirms continuity.

## User experience

### Group creation from template

When the user creates a group from a template in Web UI:

1. choose project path
2. choose template file
3. optionally open a `Resume Recovery` section
4. add one or more bindings

Each binding row should contain:

- actor selector
- runtime badge / inferred runtime
- `session_id` input
- `enabled` toggle

This is intentionally actor-based rather than foreman-only. The common case may
still be "bind the first actor / foreman", but the model should not hard-code
that assumption.

### Actor editing after creation

In actor settings, the user should be able to:

- see whether native resume is enabled for launch
- edit or replace the stored `session_id`
- disable resume without deleting the stored `session_id`

This gives users a reversible switch:

- `enabled = true`: try native resume on launch
- `enabled = false`: launch fresh, but keep the saved session identifier for
  later reuse

## Executed validation matrix

The feature was re-validated on March 15, 2026 local time
(`2026-03-16` UTC) with three different chain lengths instead of only one
happy path.

### Short chain: edit existing actor intent only

Validated against existing group `g_414cfed1a68e`
(`cccc-resume-template-e2e-verified`).

Executed path:

1. Stop actor `需求规划专家`.
2. Open `Edit agent configuration`.
3. Clear `Prefer native resume on launch`.
4. Save.
5. Re-open the same dialog.
6. Verify:
   - `Prefer native resume on launch` stays unchecked
   - `Session ID` still stays
     `019cf430-228b-7ea3-bb58-bf2653eea8c2`
7. Re-enable resume and save again to restore the original state.

This proves the actor-level switch is reversible and does not erase the stored
native session target.

### Medium chain: template import plus actor prefill, no launch

Validated against fresh group `g_4fa9ed662c8e`
(`cccc-resume-template-medium-fresh`) attached to:

- `/Users/glennxu/workspace/minion/cccc-resume-e2e-medium-fresh`

Executed path:

1. Create a brand-new group from blueprint.
2. Import `data/cccc-group-template-codex--checklist.yaml`.
3. In `Resume Recovery`, bind actor `需求规划专家`.
4. Save session ID `019cf430-228b-7ea3-bb58-bf2653eea8c2`.
5. Leave `Resume on launch` enabled.
6. Create the group.
7. Open `Edit agent configuration` for `需求规划专家` before first launch.
8. Verify:
   - `Prefer native resume on launch` is checked
   - `Session ID` is prefilled with
     `019cf430-228b-7ea3-bb58-bf2653eea8c2`

The resulting group file persisted:

```yaml
native_resume:
  enabled: true
  session_id: 019cf430-228b-7ea3-bb58-bf2653eea8c2
```

This proves the create-flow binding survives the modal submit, API apply step,
and actor edit preload even before any runtime launch happens.

## Long chain: full end-to-end launch and `/status`

The following path was re-validated on March 15, 2026 local time
(`2026-03-16T01:44Z`) against the feature branch implementation:

1. Open Web UI and create a new group.
2. Attach the existing project path
   `/Users/glennxu/workspace/minion/cccc-resume-e2e-round2`.
3. Import the blueprint file
   `data/cccc-group-template-codex--checklist.yaml`.
4. In `Resume Recovery`, add one binding.
5. Select the first actor, `需求规划专家`.
6. Enter session ID `019cf430-228b-7ea3-bb58-bf2653eea8c2`.
7. Keep `Resume on launch` enabled.
8. Create the group from blueprint.
9. Open that actor's `Edit agent configuration` dialog before launch.
10. Verify both of the following are already populated from the saved binding:
    - `Prefer native resume on launch` is checked
    - `Session ID` is `019cf430-228b-7ea3-bb58-bf2653eea8c2`
11. Launch the actor.
12. In the actor terminal, manually run `/status` and press Enter.
13. Verify the resumed Codex session still reports
    `019cf430-228b-7ea3-bb58-bf2653eea8c2`.

The same validation also confirmed that the binding is durably written into the
group state. The resulting group file contained:

```yaml
native_resume:
  enabled: true
  session_id: 019cf430-228b-7ea3-bb58-bf2653eea8c2
```

For the verified Web API request, `POST /api/v1/groups/from_template` returned:

```json
{
  "group_id": "g_414cfed1a68e",
  "applied": true,
  "resume_bindings_applied": ["需求规划专家"]
}
```

This matters because it proves the full chain is working end to end:

- template-create modal state
- Web API request payload
- daemon template-create apply step
- persisted actor config
- actor edit modal preload
- actual runtime launch via native resume
- live `/status` session continuity

A later Chrome MCP rerun on March 15, 2026 local time also reconfirmed the
runtime continuity on fresh group `g_236e39802004`
(`cccc-resume-template-long-regression`).
In that rerun, typing `/status` into the Web terminal was accepted, but the
session line did not re-render back into the terminal DOM reliably enough for
automation to scrape it.
The runtime PTY state still recorded:

```json
{
  "session_id": "019cf430-228b-7ea3-bb58-bf2653eea8c2"
}
```

from `~/.cccc/groups/g_236e39802004/state/runners/pty/需求规划专家.json`.

Inference: native resume continuity still held; the weaker part in that rerun
was the Web terminal's `/status` rendering as automation evidence, not the
resume binding itself.

## Related Glossary

- [actor](/reference/glossary/actor)
- [profile](/reference/glossary/profile)
- [resume](/reference/glossary/resume)
- [status](/reference/glossary/status)
- [attach](/reference/glossary/attach)
- [authoritative_workspace](/reference/glossary/authoritative_workspace)
- [execution_workspace](/reference/glossary/execution_workspace)

## Change Log

- `2026-03-21`: Added local glossary alignment so actor-scoped native resume docs stop drifting on attach authority, execution workspace, and status-versus-resume meaning.
- `2026-03-23`: Added `profile` alignment so stored native resume intent can be discussed without collapsing reusable config into live runtime truth.

## Additional branch conditions observed during testing

These were not counted as successful medium-chain runs, but they are important
test branches because they change the control flow before resume binding can be
validated.

### Ordinary create-flow duplicate attach path is now guarded

During this round of regression testing, the plain `Create Group` flow
(without blueprint import) exposed a separate bug outside the resume-binding
path:

- the UI first created a new empty group
- it then tried `attach`
- the daemon previously allowed that attach even when the same scope already
  belonged to another group

This produced duplicate groups for the same attached directory.

The feature branch now fixes that ordinary create-flow branch in two layers:

- daemon `attach` rejects `scope_already_attached` when the scope already
  belongs to a different group
- Web UI deletes the just-created empty group and switches to the already
  attached group instead of leaving an orphan behind

After the fix, a Chrome MCP rerun using
`/Users/glennxu/workspace/minion/cccc-resume-e2e-medium-regression` no longer
created a new `duplicate-path-check-fixed-2` group.
The UI switched back to the already attached group and showed:

- `This directory already has a working group. Opening it instead.`

One nuance matters for debugging: if older duplicate groups already exist from
pre-fix runs, the reopened group may be whichever group the current registry
maps to for that scope, not necessarily the earliest or most desirable one.

### Ordinary create-flow nested path now also reopens the normalized parent scope

The same plain `Create Group` branch was re-run with a nested path:

- `/Users/glennxu/workspace/minion/cccc-codex-session-resume/e2e-nested-regression-2`

Because `detect_scope(...)` normalizes that path back to the parent worktree
scope, this is an important separate branch.

Before the fix, this branch could also create a second empty group for the same
normalized parent scope.
After the fix, the UI re-opened the already attached group and did not create a
new `nested-path-check-fixed-2` group.

So nested-path attach is still a distinct regression branch worth testing, but
it should now resolve to the existing normalized parent scope rather than
creating another orphan group.

## UI automation stability note

During Web UI automation, one concrete source of flakiness was identified in the
`Resume Recovery` list itself:

- if a binding row key changes when `actor_id` changes, React remounts the whole
  row
- browser automation then loses its handle on the `Session ID` field right after
  actor selection

The feature branch now keeps each resume-binding row on a stable local ID
instead of deriving the row key from `actor_id`.

Practical effect:

- selecting an actor no longer recreates that row's inputs
- the follow-up `Session ID` fill step is much more reliable for Chrome MCP and
  similar UI automation
- repeated controls also expose stable per-row field ids and clearer labels,
  which makes future browser automation easier to target

## Recommended regression order

After any UI change to group creation, actor editing, or native resume wiring,
re-run the following cases in order:

1. `Short`:
   open an existing actor, disable native resume, save, reopen, and confirm the
   saved `session_id` survives while the checkbox stays off
2. `Medium`:
   create a fresh group from blueprint on a brand-new attach path, bind one
   actor, create the group, then confirm the actor edit dialog preloads the
   saved binding before first launch
3. `Long`:
   repeat the medium case and also launch the actor, manually run `/status`, and
   confirm the runtime session still matches the saved `session_id`
4. `Duplicate path branch`:
   submit a path that already belongs to an existing group and confirm the UI
   reopens that group instead of silently creating a second one
5. `Nested path branch`:
   submit a subdirectory under an already attached group root and confirm
   whether it still coalesces to the parent group
6. `Daemon mismatch guard`:
   verify `uv run cccc daemon status` reports the expected feature-branch daemon
   version before trusting any failed UI result

This order intentionally goes from cheapest validation to most expensive
validation. If the short or medium cases fail, the long case is usually not yet
worth running.

## Operational pitfall: stale daemon false negatives

During validation, a serious false-negative case appeared:

- the Web UI on port `8848` was serving the current feature branch
- but the daemon behind `~/.cccc` had silently fallen back to version `0.4.2`

In that stale-daemon state:

- the request body still included `resume_bindings_json`
- the server response omitted `resume_bindings_applied`
- the created group's `group.yaml` kept `native_resume_policy: 'off'`
- the Edit Actor dialog showed no saved session ID

That failure mode is not a UI regression in the feature itself. It means the
current Web bundle is talking to an older daemon process.

Practical rule for debugging:

- always verify daemon version before trusting template-import resume results
- the expected fixed path should report `ccccd: running pid=... version=0.4.4`
- if the daemon reports `0.4.2`, stop it and restart from the current worktree
  before drawing any product conclusion

## Why this should not live inside the template file

Group templates are meant to stay portable across:

- machines
- repositories
- users
- clean-room project setup flows

`session_id` values are not portable in that same way. They point at local CLI
history under a specific runtime home such as `~/.codex`.

So the template should continue to describe:

- actor order
- runtime
- runner
- command
- delivery / automation defaults

But it should not directly carry machine-local native session bindings.

## Data model direction

CCCC should distinguish two different classes of state.

### 1. Actor configuration: user intent

This is the durable configuration that answers:

- should this actor try native resume on launch?
- if yes, what native session identifier should it prefer?

Conceptually, this looks like:

```yaml
native_resume:
  enabled: true
  session_id: sess_abc123
```

The exact field names may change, but the semantics should stay stable.

### 2. Runner state: runtime-discovered evidence

This is the PTY/headless runtime state CCCC already records, such as:

- `session_id`
- `session_log_path`
- `runtime`
- `cwd`

This state is evidence gathered from launch and recovery, not the user's source
of truth for desired behavior.

## Launch behavior

For the first supported runtime, the launch rule should be:

- if actor runtime is `codex`
- and actor runner is `pty`
- and native resume is enabled
- and a `session_id` is configured

Then:

1. pass that `session_id` into the current `codex` recovery pipeline
2. let discovery logic try to resolve `session_log_path`
3. build a proper `codex resume <session_id>` command when possible
4. otherwise fall back safely to a fresh launch

That final fallback matters in real use.

A live session ID captured from `codex /status` is useful actor-local evidence,
but it does not always mean Codex itself can reopen that session through
`codex resume <session_id>`.

So the launch contract should be:

- preserve the user's configured `session_id`
- prefer native resume when recovery evidence is strong enough
- avoid turning an unavailable resume target into a broken actor start

If native resume is disabled:

- do not attempt `codex resume`
- do not delete the configured `session_id`

## Relationship to current Codex resume implementation

This design does **not** replace the existing `codex` resume work.

Instead, it adds a better configuration surface on top of:

- `codex` session discovery
- `session_log_path` recovery
- PTY state persistence
- `experimental_resume` command injection

In other words:

- current implementation solves "how to resume"
- actor native resume bindings solve "which actor should prefer resume, and when"

## Prompt and coordination behavior

Native resume must not bypass CCCC's own coordination model.

Required invariant:

- native runtime resume may change the launch command
- it must **not** disable or short-circuit CCCC prompt delivery, preamble/help,
  inbox continuity, or work-state recovery

So the target behavior is:

- actor resumes native `codex` session when configured
- CCCC still injects its own system guidance through the normal startup path

## Safety boundaries

The initial scope should stay conservative.

Supported first:

- `codex`
- `pty`
- manual `session_id` entry
- actor-scoped enable/disable switch
- safe fallback to fresh launch when native resume target is unavailable

Not required for the first iteration:

- auto-discover-and-bind from Web UI directly
- runtime-agnostic generic editor for every future CLI
- auto-probing live runtime `/status` from Web
- embedding session bindings into exported group templates

## Recommended rollout order

1. Add actor-config fields for native resume intent.
2. Accept actor resume bindings during "create group from template".
3. Thread configured `session_id` into the existing `codex` launch pipeline.
4. Add actor edit controls for enable/disable and session ID retention.
5. Only after that, consider richer runtime-generic abstractions.
