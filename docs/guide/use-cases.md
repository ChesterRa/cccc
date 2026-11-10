# Use Cases

This page focuses on high-ROI, real-world CCCC workflows.

## How to Read This Page

Each scenario includes:
- Goal
- Minimal setup
- Execution flow
- Success criteria
- Common failure points

## Use Case 1: Builder + Reviewer Pair

### Goal

Deliver one bounded change with an independent review and a clear owner for
integration. Start with two agents before adding more roles.

### Minimal Setup

This example uses members that CCCC starts. Install and sign in to the runtimes
you intend to use first; follow the [CLI Quick Start](./getting-started/cli) or
[Web UI Quick Start](./getting-started/web) for the host setup. To connect sessions
you already have open instead, use [Native sessions](./local-sessions).

For a project with no existing CCCC Group, register a pair:

```bash
cd /path/to/repo
cccc attach .
cccc actor add builder --runtime codex
cccc actor add reviewer --runtime claude
cccc actor list
```

These runtime choices are an example, not a required pairing. The first agent
in a new Group is its foreman; here that is `builder`. If the project already
has a Group, inspect its members and choose unused names instead of replacing
existing agents.

Merge the [short AGENTS.md template](./agents-template) into the project's
instructions, including its own commands and constraints. CCCC does not make
every runtime load `AGENTS.md`; check the runtime's instruction loading before
the first task.

Start the new Group's two members:

```bash
cccc group start
```

When adding members to an existing Group, start only those members with their
Web controls instead of starting every member in the Group. Before assigning
work, complete any pending runtime onboarding or trust prompt in the Web
confirmation terminal. See [runtime interaction](./runtimes#runtime-interaction).

Agree where each member will edit. A shared Group or scope does not guarantee
the same checkout: a managed Claude background session may enter a worktree.
See [where managed Claude edits files](./runtimes#managed-claude-edit-location)
for configuration and cleanup. Do not assume a clean main checkout means
another member has made no changes.

### Execution Flow

1. Give the builder one concrete behavior to change, acceptance cases and
   boundaries. Name the reviewer and integration owner; here the builder
   integrates after review. For example: "Remove duplicate IDs from the CSV
   export. Add coverage for repeated IDs and keep the public API unchanged."
2. Ask the reviewer to inspect the requirements and current implementation
   independently. Agree file ownership before either edits; review can start
   with a read of the existing behavior while implementation proceeds.
3. Keep work that needs a lasting owner or completion record in a CCCC task.
   Keep the goal and shared constraints in the brief, rather than copying every
   local todo or chat message onto the board.
4. The builder hands off the actual checkout/branch, changed files, checks and
   remaining uncertainty. The reviewer examines that change and reports
   actionable findings with evidence, or says what was checked and found clear.
5. The integration owner resolves confirmed findings and reruns affected
   checks. Report the result and remaining limits to the user; commits, pushes
   and releases follow the authority the user granted.

For a concrete reply obligation, the human can select **Send + Reply** in the
Web UI, or use the CLI's `--mode request-reply`. Ordinary agent discussion
follows CCCC's built-in Send/Mail guidance.

Use the next similar task to check whether an agreed workflow adjustment helped.

### Success Criteria

- The agreed behavior and acceptance cases are satisfied.
- Review findings refer to the actual change and can be acted on.
- The result is integrated where intended; the handoff links acceptance checks
  and their results, with remaining limits stated.

### Common Failure Points

- Task scope too broad.
- Reviewer lacks explicit acceptance criteria.
- Both agents edit the same files without agreeing ownership.
- A worktree contains the result but the integration owner checks only the main
  checkout.
- Runtime confirmation is still pending, or changed instruction files have not
  been read by the running sessions.
- A concrete review was requested without a reply obligation or clear handoff.

## Use Case 2: Foreman-Led Multi-Agent Delivery

### Goal

Split one medium project into parallel tracks while keeping alignment.

### Minimal Setup

```bash
cccc actor add lead --runtime claude
cccc actor add frontend --runtime codex
cccc actor add backend --runtime grok
cccc actor add qa --runtime kimi
cccc group start
```

### Execution Flow

1. Foreman records the current objective, focus and constraints in the shared
   brief, and uses tasks for work with durable owners and completion criteria.
2. Assign focused tasks via direct recipients.
3. Enforce checkpoint reminders through Automation rules.
4. Foreman integrates and resolves conflicts.
5. QA agent validates key acceptance criteria before handoff.

### Success Criteria

- Parallel execution without major rework churn.
- Clear ownership per track.
- Traceable decision history in ledger.

### Common Failure Points

- Missing shared architecture baseline.
- Agents editing same surfaces without ownership rules.
- No explicit integration checkpoints.

## Use Case 3: Mobile Ops with IM Bridge

### Goal

Operate long-running groups from phone while keeping a reliable audit trail.

### Minimal Setup

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
```

Then `/subscribe` in your IM chat.

### Execution Flow

1. Receive progress/error notifications in IM.
2. Send escalation commands to `@foreman` from mobile.
3. Switch to Web UI for deep debugging only when needed.
4. Keep all critical decisions in CCCC messages (not only in IM thread).

### Success Criteria

- You can intervene without laptop access.
- Critical context remains in ledger.
- Downtime is reduced for overnight or offsite ops.

### Common Failure Points

- Exposing Web UI without proper token/gateway.
- Using IM as the only source of truth.
- No restart/recovery playbook.

## Use Case 4: Repeatable Agent Benchmark Harness

### Goal

Run comparable multi-agent sessions with stable logging and replayability.

### Minimal Setup

1. Define fixed task prompts and evaluation criteria.
2. Use the same Copy Groups package and runtime setup per run.
3. Keep automation policies deterministic.

### Execution Flow

1. Create a baseline group and export a Copy Groups package.
2. Run multiple sessions with different runtime combinations.
3. Collect ledger and terminal evidence.
4. Evaluate outcome quality and operational stability.

### Success Criteria

- Comparable runs with low setup variance.
- Reproducible evidence set (`ledger`, state artifacts, logs).
- Clear model/runtime tradeoff signals.

### Common Failure Points

- Hidden prompt drift between runs.
- Uncontrolled environment differences.
- Missing run metadata in messages.

## Recommended Next Reads

- `docs/guide/operations.md`
- `docs/reference/positioning.md`
- `docs/reference/features.md`
