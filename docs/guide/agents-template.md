# A Short AGENTS.md Template

Use this as a starting point for a project with a builder and a reviewer. Adapt
it to the project's rules; merge it into an existing instruction file rather
than replacing that file. Add or link the project's architecture, conventions
and actual build/test commands separately.

```markdown
# Project collaboration

## Goal and scope
- Work toward the user's outcome and agreed completion criteria. State material assumptions.
- Check current files, commands and behavior before changing them; separate facts from inferences.
- Make routine decisions within the agreed scope. Ask when authority, missing information or a real product tradeoff prevents progress.
- Prefer the simplest complete solution; reuse mechanisms whose semantics fit.
- Prioritize work and review findings by user impact and cost; avoid speculative complexity.

## Working together
- Before parallel edits, agree on file/module ownership and check which checkout each member uses.
- Form your own view before reading another member's review. Challenge substantive risks with evidence and alternatives.
- Share useful problems and findings while work is still open, not only at the final handoff.
- Keep the current goal, constraints, durable tasks and handoffs in CCCC; keep project rules in this file.
- When collaboration friction recurs, identify its cause and propose a small improvement. Check its effect the next time it applies.

## Review and handoff
- Review the whole change against the goal, then check correctness and failure paths. Agree who runs checks that share build or service resources.
- Report what changed, the evidence, and what remains uncertain; correct mistaken claims promptly.
- At handoff, name the checkout/branch, changed files, validation and person responsible for integration.
- Preserve unrelated edits and useful history. Permission to edit or commit does not authorize pushing, publishing, deploying or interrupting the user's services.
```

This is project guidance, not a replacement for CCCC's built-in routing and
collaboration protocol. Use the existing brief and task tools for changing work
state instead of turning the instruction file into a chat log.

CCCC's **AGENTS.md** editor saves the file; each runtime controls whether and
when it reads project instructions. Confirm the intended instructions are
loaded, and adapt the filename or native loading configuration when needed.
See [Best Practices](./best-practices) for instruction loading and refresh.

Try it with the existing [Builder + Reviewer Pair](./use-cases#use-case-1-builder-reviewer-pair)
walkthrough. Start with a small real change, inspect the handoff, and adjust the
project guidance from what the team actually needed.
