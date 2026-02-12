# RC19 Execution Checklist

## R0 Scope Freeze

- [x] Freeze new feature intake
- [x] Assign owners for Tier A domains
- [x] Create issue labels (`rc19`, `rc19-p0`, `rc19-p1`) or track equivalently via release register in local-first workflow

## R1 Full Inventory

- [x] Run `./scripts/release/gen_rc19_file_matrix.sh`
- [x] Confirm matrix coverage = tracked file count
- [x] Fill owner for all Tier A rows

## R2 Contract Audit

- [x] CCCS vs implementation review complete
- [x] IPC examples match current behavior/version
- [x] Daemon IPC op index fully documented (impl parity)
- [x] Automation/MCP contract parity checked

## R3 Runtime Core Audit

- [x] Kernel state machine checks complete
- [x] Delivery/automation/inbox edge-cases reviewed
- [x] Permission boundaries reviewed

## R4 Port & UX Audit

- [x] CLI/docs parity
- [x] MCP/docs parity
- [x] Web/docs parity
- [x] IM bridge docs/config parity

## R5 Gate Upgrade

- [x] CI includes test execution gate
- [x] release workflow gate policy confirmed
- [x] `RC19_GATES.md` validated

## R6 Fix Sprint

- [x] All P0 fixed
- [x] P1 triaged to fixed or accepted_risk
- [x] Findings register updated

## R7 Docs Sync

- [x] README (EN/ZH/JA) synced to implementation
- [x] Reference/Guide/Standards synced
- [x] release assets prepared (`CHANGELOG`, notes, policy docs)

## R8 Rehearsal

- [x] Rehearsal report written
- [x] Go/No-Go decision recorded

## R9 Publish RC19

- [x] Version bump complete
- [ ] Tag + publish TestPyPI complete
- [ ] Install/upgrade verified from notes
