# RC19 Audit Method

## 1. First-Principles Checklist

Use this checklist for each reviewed file/symbol:

- Correctness: does behavior match contract and user expectation?
- State model: are transitions legal, explicit, and recoverable?
- Ownership: is source-of-truth single and clear?
- Safety: any silent failure, privilege leak, or data corruption risk?
- Operability: can issues be observed, diagnosed, and recovered fast?
- Simplicity: does design minimize hidden coupling and accidental complexity?
- Consistency: are naming/defaults/semantics consistent across CLI/Web/MCP/docs?

## 2. Tiered Review Depth

## Tier A (deep)

Targets:

- `src/cccc/**`
- `web/src/**`
- `.github/workflows/**`
- release-critical config files (`pyproject.toml`, top-level READMEs)

Procedure:

- per-file architectural review
- per-public-function contract review
- edge-case and failure-path review
- doc/API parity check

## Tier B (standard)

Targets:

- `tests/**`, `scripts/**`, `docker/**`
- `docs/guide/**`, `docs/reference/**`, `docs/standards/**`

Procedure:

- behavior and interface parity
- stale examples/commands/paths check
- release-impact assessment

## Tier C (light)

Targets:

- `docs/vnext/archive/**`, `old_v0.3.28/**`, historical artifacts

Procedure:

- ensure no misleading current-version statements
- ensure links or disclaimers are explicit

## 3. Finding Record Rules

Each finding must include:

- ID, severity, path, symbol
- impact statement
- reproduction or evidence
- proposed fix and owner
- status (`open`, `in_progress`, `fixed`, `accepted_risk`)

No owner => invalid finding.

## 4. Severity Rubric

- `P0`: release blocker (data loss, contract break, security/privacy, hard crash)
- `P1`: major operational or semantic drift
- `P2`: non-blocking quality debt

## 5. Decision Rules

- Fix by default.
- `accepted_risk` allowed only with explicit rationale and expiry.
- Re-open accepted risk if related code changes.

## 6. Daily Execution Cadence

- daily triage: re-rank findings by impact and effort
- daily closure target: all new `P0` same day, `P1` within cycle
- maintain board and matrix in sync with code changes

