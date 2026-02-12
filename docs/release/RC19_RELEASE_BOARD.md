# RC19 Release Board (`v0.4.0rc19`)

## 1. Goal

Ship `rc19` as a **quality-convergence release**:

- close architecture/protocol drift
- close docs/implementation drift
- harden quality gates for reproducible releases
- prepare clean inputs for `0.4.0` GA decision

No large new feature bets in this cycle.

## 2. Scope Rules

### In Scope

- P0/P1 bug fixes
- protocol/contract consistency fixes
- release-critical UX consistency fixes
- docs truth sync and release assets
- CI/release gate hardening

### Out of Scope

- major net-new product surfaces
- speculative refactors without release risk reduction
- broad compatibility migration projects

## 3. Full-System Audit Policy

Every tracked file is reviewed through the matrix (`docs/release/rc19_file_matrix.csv`), with tiered depth:

- `Tier A` (deep): per-file + per-public-function audit, design rationale required
- `Tier B` (standard): per-file behavior and interface consistency audit
- `Tier C` (light): misguidance/staleness/risk-only audit

No file is skipped. Unowned findings are not allowed.

## 4. Phases and Exit Criteria

## R0 Scope Freeze (Day 1)

- Freeze new feature intake.
- Create owner map and communication channel.

Exit:

- scope freeze announced
- owner map complete

## R1 Full Inventory (Day 1)

- Generate matrix from tracked files.
- Assign tier, domain, owner, status.

Exit:

- matrix coverage = 100%
- all `Tier A` files have explicit owner

## R2 Architecture & Contract Audit (Day 2-3)

- Audit `CCCS`, `daemon IPC`, `context ops` contracts vs implementation.
- Audit versioned examples and wire formats.

Exit:

- contract mismatch list complete
- all protocol P0/P1 gaps triaged

## R3 Runtime Core Audit (Day 3-5)

- Audit `kernel`, `daemon`, delivery, automation, inbox, permission boundaries.
- Verify state-machine legality and invariants.

Exit:

- `P0 = 0`
- `P1` all have owner + fix plan

## R4 Port & UX Audit (Day 5-6)

- Audit CLI/MCP/Web/IM consistency (naming, defaults, behavior).
- Audit cross-surface terminology consistency.

Exit:

- no unresolved critical API/UX mismatch

## R5 Quality Gate Upgrade (Day 6)

- Ensure CI/release gate includes test execution (not build-only).
- Lock release rehearsal checklist.

Exit:

- gate policy documented
- CI and release gates green on dry run

## R6 Fix Sprint (Day 7-9)

- Land fixes from R2-R5 by severity.

Exit:

- `P0 = 0`
- `P1` reduced to explicit accepted risks only

## R7 Docs & Messaging Sync (Day 9-10)

- Sync README/docs/standards/reference to code truth.
- Add release assets (`CHANGELOG`, notes, support/security policy).

Exit:

- sampled docs have 0 factual mismatch
- release assets ready

## R8 Release Rehearsal (Day 10)

- End-to-end rehearsal: build, install, smoke, mcp handshake, core workflows.

Exit:

- rehearsal report complete
- go/no-go decision recorded

## R9 Tag and Publish RC19 (Day 11)

- bump version
- tag + publish TestPyPI
- publish release notes

Exit:

- install/upgrade path verified
- rollback path documented

## 5. Required Artifacts

- `docs/release/RC19_OWNER_MAP.md`
- `docs/release/rc19_file_matrix.csv`
- `docs/release/RC19_FINDINGS_REGISTER.md` (instantiated per real findings)
- `docs/release/RC19_GATES.md`
- `docs/release/RC19_EXECUTION_CHECKLIST.md`
- `docs/release/rc19_contract_gap.md`
- `docs/release/rc19_core_findings.md`
- `docs/release/rc19_port_parity.md`
- `docs/release/rc19_strategic_review.md`
- `docs/release/rc19_rehearsal_report.md`

## 6. Severity Policy

- `P0`: correctness/data-loss/security or release blocker
- `P1`: major behavior drift or operational risk
- `P2`: polish/maintainability

Release rule:

- `rc19`: requires `P0=0`
- `GA`: requires `P0=0` and no unowned `P1`

## 7. GA Readiness Gate (post-rc19)

`0.4.0` is allowed only when:

- rc19 soak passes
- protocol/docs/CLI/MCP/Web terminology converged
- release pipeline and rollback are reproducible
