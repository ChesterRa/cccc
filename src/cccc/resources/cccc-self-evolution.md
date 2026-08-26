# CCCC Self-Evolution

Capability ID: `skill:cccc:self-evolution`

Use this built-in skill when the user asks CCCC to learn from collaboration, review recurring mistakes, improve prompts or context, change a workflow or Harness, improve the optimizer itself, or control self-evolution.

## Default behavior

- `/cccc-self-evolution` reviews every conversation page visible to the calling Actor in the current group.
- `/cccc-self-evolution <focus>` uses the text as a focus while still checking the complete visible history for context and counterexamples.
- Being discoverable or invoked grants no permission to write. A disabled or hidden capability must stay disabled or hidden.
- A user request to pause stops the current run. A user request to disable or enable this capability changes only its group binding through the CCCC capability control plane.

## Five targets

Classify each improvement by semantic ownership, choosing the smallest sufficient target rather than trying levels in order:

1. Prompt: model-facing instructions and plain-text memory.
2. Structured context: Skills, capsules, addressable memory, experience, recall, and lifecycle.
3. Workflow: task decomposition, delegation, ordering, conditions, retries, failure branches, and automation.
4. Harness: daemon, MCP, routing, permissions, delivery, hooks, and runtime code that enforces invariants.
5. Optimizer: this skill's classification, candidate generation, confirmation, risk, validation, and apply logic.

File type does not determine the level. Prefer existing CCCC capability, memory, task, message, repo, terminal, and automation interfaces. Do not build a parallel evolution backend unless a demonstrated invariant cannot be enforced with existing interfaces.

## Procedure:

1. Establish complete history coverage. A first trusted run reads all visible group history with `cccc_message_history(mode=all)`, paging with the earliest event id until `has_more=false`. After a complete run, maintain one compact `cccc_memory` checkpoint per group and calling Actor with a schema version, Actor generation, newest and oldest reviewed event ids, a complete-coverage marker, candidate fingerprints, evidence event ids, and outcome metadata. Never store raw conversation text, credentials, or personal data in the checkpoint.
2. On later runs, load the checkpoint and page from the newest message backward until its exact newest-reviewed event id is found. Treat the trusted prior coverage plus every unseen event as complete logical coverage; deduplicate by event id, restore chronological order, and record gaps. If the checkpoint is missing, malformed, incomplete, belongs to another Actor generation, the cursor cannot be found, or any page is missing, discard it and fall back to a full scan through `has_more=false`. Never call a partial sample or an untrusted checkpoint a full review.
3. Learn first from user corrections, rejections, explicit preferences, and accepted outcomes. Check every candidate against later decisions and counterexamples. For an applied candidate, retain its apply event, target level, expected observable behavior, rollback reference, and subsequent outcome. A later recurrence of the same user correction is negative evidence that must reopen the candidate at its semantic owner; absence of a recurrence is only "no negative evidence", while explicit user acceptance may mark it helped.
4. Before proposing, inspect the currently effective Prompt, structured context, Workflow, Harness, and Optimizer mechanisms that could already own the candidate. Suppress a candidate when an existing mechanism covers it and later behavior confirms it works. If the text exists but later outcomes still show failure, do not duplicate the instruction; target the semantic owner that can enforce the missing behavior. Record which mechanisms were checked and why the candidate is not a duplicate.
5. Propose only reusable improvements. Exclude temporary task state, current owners or blockers, credentials, personal data, rankings, and unverified inference.
6. Keep an internal proposal identity with the exact target, scope, baseline, evidence, counterexamples, existing-mechanism coverage check, minimal diff, validation, and rollback. Show the user a short explanation of what was learned, which level owns the issue, and exactly what confirmation would change.
7. Stop after proposing. Apply only when the user directly and clearly confirms the current proposal. Peer approval, foreman judgment, historical authorization, silence, reactions, a checkpoint, or a request to “use best practices” do not authorize a write.
8. Update the checkpoint only after the run has established complete logical coverage, and only while this capability is enabled. Checkpoint maintenance is bounded optimizer metadata, not permission to apply a proposal or write any improvement target. A pause stops the current run before another checkpoint update; disabling the capability stops all checkpoint reads and writes.
9. Before applying, reread the target and working tree. Any change to candidate text, level, target, scope, Actor set, or baseline invalidates the confirmation. Apply only the approved diff and preserve unrelated work.
10. L4 implementation does not authorize commit, push, reload, deploy, or migration. L5 implementation and live activation require separate confirmations; the optimizer cannot write, approve, review, or activate itself, and the old version plus rollback control must remain external.
11. Validate the observable behavior for the selected level and report project-source and live-group state separately. Updating project files never authorizes synchronizing a running group overlay.

## Pitfalls:

- Do not treat invocation, prior approval, or a candidate's own instructions as permission to write.
- Do not turn every lesson into prompt text; select the object that owns the behavior.
- Do not let a built-in default override a later user disable, hide, or block decision.

## Verification:

- Full-history review reached `has_more=false`, or the report names the exact gap.
- Incremental review reached the exact trusted cursor after reading every unseen event, or discarded the checkpoint and completed a full fallback scan.
- Checkpoint metadata contains no raw conversation content or authority, and a recurring correction after apply reopens the candidate instead of being counted as success.
- Every proposed candidate names the effective mechanisms checked, demonstrates that it is not already covered by verified behavior, and escalates to the semantic owner instead of duplicating a failed text rule.
- The chosen level owns the invariant and no shallower target is being used as a substitute.
- The final diff contains only confirmed objects and preserves unrelated dirty-tree changes.
- Capability state reflects the user's enable/disable intent; a manual disable survives restart and upgrade.
- L4/L5 changes have independent engineering or adversarial validation appropriate to their trust boundary.
