# Codex Session Resume

How CCCC restores a `codex` actor back into the right long-running session, and
how actor-bound resume intent interacts with runtime evidence and safe fallback.

## Terminology Alignment

This document follows the local glossary:

- `resume` is layered; it does not mean only native vendor session recovery
- `status` means an evidence-bound observation surface
- `profile` means reusable actor runtime configuration and launch intent
- `attach` defines the `authoritative_workspace`
- actor runtime `cwd` is better described as an `execution_workspace`

If older wording in this document sounds narrower than the glossary, treat it
as compatibility wording.

## Current implementation status

Today, the shipped recovery path combines actor-bound resume intent with
PTY-state-driven runtime evidence.

For PTY actors with runtime `codex`, CCCC persists and reuses:

- `session_id`
- `session_log_path`
- `runtime`
- `cwd`

In glossary terms, the recovered `cwd` is runtime evidence about execution
workspace. It should not be read as replacing the group's authoritative
workspace.

When a `codex` PTY actor starts or restarts, CCCC can rebuild a real
`codex resume ...` command instead of always starting a brand-new empty session.

The current Web UI also lets a user:

- enable or disable native resume per actor
- keep a stored `session_id` even while resume is disabled
- read the actor's current live session ID from `/status` output that is already
  visible in the PTY transcript
- save that live session ID back into actor configuration

That stored native-resume intent may live in direct actor configuration or, in
future flows, in reusable actor-linked `profile` semantics. In either case, it
remains launch intent rather than proof.

Important: a live `/status` session ID is useful evidence, but it is **not** by
itself proof that `codex resume <session_id>` will succeed later.

This is an intentional local distinction between `status` and `resume`.

This currently applies across the normal actor launch paths:

- `cccc actor start`
- `cccc actor restart`
- `cccc group start`
- template/bootstrap flows that eventually start PTY actors

## Safe fallback behavior

CCCC now treats native resume as an optimization, not a hard requirement.

If a `codex` actor has native resume enabled and a configured `session_id`, CCCC
tries to build a real `codex resume ...` command only when it also has concrete
resume artifacts such as a `session_log_path`.

If those artifacts are incomplete, stale, or missing, CCCC now falls back
safely to a normal fresh `codex` launch instead of letting the actor crash on
startup.

That means:

- the actor stays launchable even when the saved resume target is no longer
  present in Codex's own saved-session store
- the configured `session_id` is still preserved in actor config
- the running actor can continue from a fresh session, and the user can inspect
  a new live session ID later if needed

## Discovery order

CCCC now resolves prior `codex` session metadata from multiple sources, in this order:

1. persisted PTY state under the group runner state
2. `~/.codex/sessions/**/rollout-*.jsonl`
3. `~/.codex/active/as-*.json`
4. `~/.codex/state_*.sqlite` `threads` rows
5. `~/.codex/session_index.jsonl`

The goal is to avoid depending on only one weak signal.

## How actor matching works

When CCCC needs to recover a `codex` session for a specific actor, it scores candidates using:

- attached workspace path / `cwd`
- `group_id`
- actor `title`
- actor `id`
- recent `threads.updated_at`

For `sqlite` thread recovery, CCCC currently matches against the `first_user_message` content that the `codex` CLI stores for each thread. In practice, CCCC benefits from prompts that contain lines like:

- `group_id: ...`
- `You are <actor_title>`
- `You are <actor_id>`

## Resume command shape

When both `session_id` and `session_log_path` are known, CCCC upgrades the actor command to:

```bash
codex \
  ...existing global flags... \
  -C /absolute/workspace/path \
  -c 'experimental_resume="/absolute/path/to/rollout-....jsonl"' \
  resume <session_id>
```

This keeps the restored session aligned with the actor scope and gives `codex` both:

- the logical session identifier
- the rollout log needed by `experimental_resume`

## Current user flow

In the normal case, you do not need to manually paste the long shell wrapper.

Recommended flow:

```bash
cccc attach .
cccc actor add planner --runtime codex --title "需求规划专家"
cccc actor sessions
cccc actor sessions planner --probe-status
cccc actor restart planner
```

If that actor had previous PTY state, or CCCC can find its prior `codex` thread
from `~/.codex`, it will now try to resume automatically.

You can also inspect what CCCC currently believes is recoverable:

```bash
cccc actor sessions --group <group_id>
cccc actor sessions <actor_id> --group <group_id>
cccc actor sessions <actor_id> --group <group_id> --probe-status
```

`--probe-status` is intentionally safety-first:

- it does not inject `/status` into the live `codex` PTY
- it reminds you that `/status` should be queried manually after you enter the actor's terminal
- automatic PTY injection is disabled because it can disrupt or terminate the live session

The matching Web UI flow is also safety-first:

- CCCC reads the current session ID only from PTY output that already exists
- it does not auto-send `/status` into the terminal for you
- the captured live session ID can be saved even if it is not yet resumable by
  Codex itself

## Current Web UI flow

The shipped Web UI behavior is:

- templates stay portable and do not embed machine-local `session_id` values
- "Create Group from Blueprint" accepts per-actor resume bindings
- actor settings expose:
  - an enable/disable toggle for native resume
  - a stored `session_id`
  - a `Use Current Session` action that reads session diagnostics
- if native resume is disabled, launch always starts fresh but keeps the saved
  `session_id`
- if native resume is enabled, launch prefers native resume when recovery
  evidence is strong enough, otherwise it falls back to a fresh launch

## Launch prompt behavior

When an actor already has `native_resume.enabled = true` and a saved
`session_id`, Web launch paths now use an explicit three-way modal instead of a
browser `OK / Cancel` confirm.

The modal shows the saved session ID and offers three distinct actions:

- `Resume saved session`
- `Fresh start and clear saved session`
- `Close`

`Launch All Agents` now follows the same decision model. Instead of silently
using saved native-resume state for every actor, the Web UI launches actors
sequentially and reuses the same resume prompt for each actor that already has a
saved `session_id`. Foreman is launched first, then peers.

Semantics matter here:

- `Resume saved session` launches with native resume intent
- resuming can continue an unfinished prior Codex task; if that old session was
  still busy, the actor may not consume fresh CCCC messages immediately after
  launch
- `Fresh start and clear saved session` starts a brand-new session and deletes
  the saved `session_id` for that actor
- `Close` performs no launch, restart, or configuration change

This change prevents the previous ambiguity where a plain `Cancel` action could
be misread as "close the dialog" even though it actually meant "discard the
saved session and launch fresh".

That gives CCCC two separate layers of state:

- actor configuration stores the user's resume intent
- PTY runner state stores the runtime-discovered recovery evidence

This distinction is intentional. User intent should not be lost just because a
runtime process stopped, and runtime evidence should not overwrite the user's
configuration model.

## Related Glossary

- [actor](/reference/glossary/actor)
- [profile](/reference/glossary/profile)
- [resume](/reference/glossary/resume)
- [status](/reference/glossary/status)
- [attach](/reference/glossary/attach)
- [authoritative_workspace](/reference/glossary/authoritative_workspace)
- [execution_workspace](/reference/glossary/execution_workspace)

## Change Log

- `2026-03-21`: Added local glossary alignment so `resume`, `status`, and workspace-boundary wording stay consistent with repo-local semantics.
- `2026-03-23`: Added `profile` alignment so stored native-resume intent is kept distinct from live runtime evidence and actor identity.

## Resume into a busy session

During March 16, 2026 validation on group `g_e87bca3c7a4d`
(`resume-fallback-flow-5`), actor `peer-impl` confirmed an important edge case:

- `/status` still showed the expected saved session ID
- actor edit state and PTY runner state agreed on the same `session_id`
- but the resumed Codex terminal was still inside an older in-progress task
  (`Summarize recent commits`)
- because that native session was still busy, the actor did not promptly read or
  visibly reply to fresh CCCC chat instructions

This is not the same failure mode as "resume lost the session". It means:

- session continuity is intact
- Web/API persistence is intact
- the resumed runtime can still be unsuitable for immediate coordination if the
  old Codex task is not idle yet

That is why the launch prompt must clearly explain the consequence of choosing
resume, and why `Launch All Agents` should not silently auto-resume every saved
actor.

## Launch persistence guard

During March 2026 end-to-end validation, CCCC hit a daemon-side regression:

- a PTY launch could correctly capture a new Codex `session_id`
- the daemon would persist it into actor config
- but a later stale in-memory `group.save()` during the same launch flow could
  overwrite that actor update back to an empty `session_id`

CCCC now avoids that overwrite by reloading the latest group document before it
persists `group.running = true` in both:

- `actor.start`
- `group.start`

This matters most for Codex session recovery because session capture and group
launch bookkeeping can happen within the same startup window.

## Verified template-import resume flow

The template-import path was re-validated on March 15, 2026 local time
(`2026-03-16T01:44Z`) with a real previously used Codex session.

Validated inputs:

- attached project path:
  `/Users/glennxu/workspace/minion/cccc-resume-e2e-round2`
- blueprint file:
  `data/cccc-group-template-codex--checklist.yaml`
- actor bound during create:
  `需求规划专家`
- saved session ID:
  `019cf430-228b-7ea3-bb58-bf2653eea8c2`

Validated outcomes:

1. `POST /api/v1/groups/from_template` accepted the actor binding and returned
   `resume_bindings_applied: ["需求规划专家"]`.
2. The created group persisted:

   ```yaml
   native_resume:
     enabled: true
     session_id: 019cf430-228b-7ea3-bb58-bf2653eea8c2
   ```

3. Before launch, `Edit Agent: 需求规划专家` already showed:
   - `Prefer native resume on launch` checked
   - `Session ID` prefilled with the same saved value
4. After launch, manually entering `/status` in the actor terminal still showed
   Codex session
   `019cf430-228b-7ea3-bb58-bf2653eea8c2`.

That sequence proves the session ID is not only saved in config; it also
survives the actual actor start and remains the runtime's active Codex session.

## Executed test matrix

The validation set now explicitly covers short, medium, and long chains instead
of relying on a single happy path.

### Short chain

Existing-group edit-only validation was run on group `g_414cfed1a68e`
(`cccc-resume-template-e2e-verified`):

1. Stop actor `需求规划专家`.
2. Disable `Prefer native resume on launch`.
3. Save and reopen the actor edit dialog.
4. Verify the checkbox stays off while `Session ID` still remains
   `019cf430-228b-7ea3-bb58-bf2653eea8c2`.
5. Re-enable resume and save again.

Result: actor launch intent is reversible without losing the stored native
session target.

### Medium chain

Fresh-group pre-launch validation was run on group `g_4fa9ed662c8e`
(`cccc-resume-template-medium-fresh`) attached to:

- `/Users/glennxu/workspace/minion/cccc-resume-e2e-medium-fresh`

Executed path:

1. Create a new group from
   `data/cccc-group-template-codex--checklist.yaml`.
2. In `Resume Recovery`, bind actor `需求规划专家` to session
   `019cf430-228b-7ea3-bb58-bf2653eea8c2`.
3. Create the group.
4. Open `Edit Agent: 需求规划专家` before first launch.
5. Verify:
   - `Prefer native resume on launch` is checked
   - `Session ID` is already prefilled with the same saved value

Result: template-import binding survives persistence and actor settings preload
even before runtime launch.

### Long chain

Full create -> persist -> preload -> launch -> `/status` continuity was run on
group `g_414cfed1a68e` (`cccc-resume-template-e2e-verified`), and the actor
terminal still reported session
`019cf430-228b-7ea3-bb58-bf2653eea8c2` after launch.

Result: runtime continuity matches the saved actor config, not just the form
state.

A later rerun on fresh group `g_236e39802004`
(`cccc-resume-template-long-regression`) confirmed the same runtime continuity
via persisted PTY state even though the Web terminal did not reliably repaint
the `/status` session line for automation scraping.
The runner state file still stored the same resumed `session_id`, so the
resume behavior remained correct.

## Extra control-flow branches observed

Two non-happy-path branches were also observed during testing and should stay
in the regression set:

1. Plain `Create Group` previously had its own duplicate-scope bug: it could
   create an empty new group and then attach an already attached scope to it.
   The current feature branch now blocks that attach in the daemon and cleans up
   the just-created empty group in Web UI, so re-submitting the same attach path
   reopens the existing group instead of leaving another orphan behind.
2. Nested paths under an already attached worktree are still normalized back to
   the parent scope, but that branch now also reopens the existing group rather
   than creating a second empty one.

## Operational pitfall: Web/UI current, daemon stale

One important debugging lesson from the same validation:

- a current Web UI alone is not enough
- if the daemon under `~/.cccc` is stale, resume behavior will look broken even
  though the feature branch code is correct

The concrete false-negative symptom was:

- Web UI served the current feature worktree
- daemon reported `ccccd: running pid=... version=0.4.2`
- template-create requests still carried `resume_bindings_json`
- response body did not include `resume_bindings_applied`
- the created group did not persist `native_resume`

After restarting the daemon from the feature worktree so it reported version
`0.4.4`, the same UI path succeeded immediately.

So the first debugging step for any "template resume binding did not stick"
report should be:

```bash
uv run cccc daemon status
```

If the daemon version does not match the worktree being tested, restart the
daemon first and only then trust the UI result.

## When recovery works best

Recovery is strongest when all of these are true:

- the group is attached to the correct project path
- the actor keeps a stable `actor_id`
- the actor keeps a stable `title`
- the original `codex` session still exists under `~/.codex`
- the original thread prompt included the actor and group identity

## Current boundaries

The current implementation is intentionally conservative.

Implemented:

- actor-config-level `resume_enabled` / `session_id` controls
- Web UI group-creation bindings for actor-specific native resume
- actor edit controls that preserve `session_id` while disabling resume
- one-click UI actions for per-actor session diagnostics / current-session fill
- persisted `session_log_path` in PTY runner state
- multi-source `codex` session discovery
- automatic `experimental_resume` injection
- automatic `resume <session_id>` rebuild during actor launch paths
- automatic fallback to a fresh launch when the configured resume target is not
  actually recoverable

Not yet implemented:

- querying live `codex` sessions by sending `/status`
- baking the external `agent-sessions-shim` heartbeat wrapper directly into CCCC
- proving Codex resumability from a live session ID alone when Codex itself has
  not persisted the corresponding saved-session artifacts

## Prompt injection remains unchanged

Native resume does not replace CCCC's own system prompt flow.

The intended model is:

- `codex` resumes its native session when enabled and recoverable
- CCCC still injects its own preamble/help/system guidance through the existing
  PTY startup and delivery pipeline

In other words, native resume is an enhancement to the launch command, not a
reason to skip CCCC-owned coordination and prompt delivery.

## Why this is safer than only using `session_id`

Using only `session_id` is not enough in real cases where:

- session index is incomplete
- multiple actors share the same workspace
- the CLI history exists in `sqlite` or active presence but not in the index
- `/status` reports a live session that Codex still does not list under
  resumable saved sessions
- `codex resume <id>` needs the rollout file to restore properly

By keeping both `session_id` and `session_log_path`, CCCC can recover much closer to the hand-crafted command that has already been proven to work locally.
