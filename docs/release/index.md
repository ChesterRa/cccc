# RC19 Release Program

This section tracks the `v0.4.0rc19` release program from full-system audit to publish.

## Documents

- [RC19 Release Board](./RC19_RELEASE_BOARD)
- [Audit Method](./RC19_AUDIT_METHOD)
- [Owner Map](./RC19_OWNER_MAP)
- [Findings Register](./RC19_FINDINGS_REGISTER)
- [Quality Gates](./RC19_GATES)
- [Execution Checklist](./RC19_EXECUTION_CHECKLIST)
- [Contract/Parity Gap Baseline](./rc19_contract_gap)
- [Core Findings (R3)](./rc19_core_findings)
- [Port Parity Findings (R4)](./rc19_port_parity)
- [Rehearsal Report (R8)](./rc19_rehearsal_report)
- [File Inventory Matrix (generated)](./rc19_file_matrix)

## Quick Start

Generate the latest full-file matrix and assign default owners:

```bash
./scripts/release/gen_rc19_file_matrix.sh
./scripts/release/assign_rc19_matrix_owners.sh
```

Then execute phases in `RC19_RELEASE_BOARD.md` in order (`R0` -> `R9`).
